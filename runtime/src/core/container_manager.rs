use crate::core::metrics_client::{MetricsClient, MetricsConfig};
use crate::core::runner::{clean_up, runner, ContainerDetails};
use crate::shared::error::{AppResult, RuntimeError};
use crate::shared::utils::{random_container_name, random_port};
use bollard::container::{RemoveContainerOptions, StatsOptions};
use bollard::Docker;
use futures_util::StreamExt;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

/// Container status enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum ContainerStatus {
    /// Container is healthy and can accept new requests
    Healthy,
    /// Container is overloaded and should not receive new requests
    Overloaded,
    /// Container is idle and candidate for scale-down
    Idle,
}

/// Information about a running container
#[derive(Debug, Clone)]
pub struct ContainerInfo {
    /// Container ID
    pub id: String,
    /// Container name
    pub name: String,
    /// Container port
    pub container_port: u32,
    /// Current CPU usage percentage (0.0-100.0)
    pub cpu_usage: f64,
    /// Current memory usage percentage (0.0-100.0)
    pub memory_usage: f64,
    /// Container status
    pub status: ContainerStatus,
    /// Last time this container handled a request
    pub last_active: Instant,
    /// Time when container became idle (for cooldown tracking)
    pub idle_since: Option<Instant>,
}

impl ContainerInfo {
    pub fn new(id: String, name: String, container_port: u32) -> Self {
        Self {
            id,
            name,
            container_port,
            cpu_usage: 0.0,
            memory_usage: 0.0,
            status: ContainerStatus::Healthy,
            last_active: Instant::now(),
            idle_since: None,
        }
    }

    /// Update container metrics and status
    pub fn update_metrics(
        &mut self,
        cpu_usage: f64,
        memory_usage: f64,
        cpu_threshold: f64,
        memory_threshold: f64,
        cooldown_threshold: f64,
    ) {
        self.cpu_usage = cpu_usage;
        self.memory_usage = memory_usage;

        let old_status = self.status.clone();

        // Determine new status based on thresholds
        if cpu_usage > cpu_threshold || memory_usage > memory_threshold {
            self.status = ContainerStatus::Overloaded;
            self.idle_since = None;
        } else if cpu_usage < cooldown_threshold {
            if self.status != ContainerStatus::Idle {
                self.idle_since = Some(Instant::now());
            }
            self.status = ContainerStatus::Idle;
        } else {
            self.status = ContainerStatus::Healthy;
            self.idle_since = None;
        }

        if old_status != self.status {
            debug!(
                "Container {} status changed from {:?} to {:?}",
                self.name, old_status, self.status
            );
        }
    }

    /// Mark container as recently active
    pub fn mark_active(&mut self) {
        self.last_active = Instant::now();
        if self.status == ContainerStatus::Idle {
            self.status = ContainerStatus::Healthy;
            self.idle_since = None;
        }
    }

    /// Check if container is eligible for scale-down
    pub fn is_eligible_for_scaledown(&self, cooldown_duration: Duration) -> bool {
        if let Some(idle_since) = self.idle_since {
            self.status == ContainerStatus::Idle && idle_since.elapsed() >= cooldown_duration
        } else {
            false
        }
    }
}

/// Configuration for container monitoring
#[derive(Debug, Clone)]
pub struct MonitoringConfig {
    pub cpu_overload_threshold: f64,
    pub memory_overload_threshold: f64,
    pub cooldown_cpu_threshold: f64,
    pub cooldown_duration: Duration,
    pub poll_interval: Duration,
    pub prometheus_url: String,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            cpu_overload_threshold: 70.0,
            memory_overload_threshold: 70.0,
            cooldown_cpu_threshold: 10.0,
            cooldown_duration: Duration::from_secs(30),
            poll_interval: Duration::from_secs(2),
            prometheus_url: "http://prometheus:9090".to_string(),
        }
    }
}

/// Container pool manager for a specific function
pub struct ContainerPool {
    /// Function name this pool manages
    function_name: String,
    /// List of containers in this pool
    containers: Arc<RwLock<Vec<ContainerInfo>>>,
    /// Docker client for container operations
    docker: Docker,
    /// Docker network
    network_host: String,
    /// Monitoring configuration
    config: MonitoringConfig,
    /// Minimum containers to maintain
    min_containers: usize,
    /// Maximum containers allowed
    max_containers: usize,
    /// Optional metrics client for Prometheus
    metrics_client: Arc<MetricsClient>,
}

impl ContainerPool {
    pub fn new(
        function_name: String,
        docker: Docker,
        network_host: String,
        config: MonitoringConfig,
        min_containers: usize,
        max_containers: usize,
        metrics_client: Arc<MetricsClient>,
    ) -> Self {
        Self {
            function_name,
            containers: Arc::new(RwLock::new(Vec::new())),
            docker,
            network_host,
            config,
            min_containers,
            max_containers,
            metrics_client,
        }
    }

    /// Add a container to the pool
    pub async fn add_container(&self, function_key: &str) -> AppResult<ContainerDetails> {
        // Generate container details
        let mut container_details = ContainerDetails {
            container_id: "".to_string(),
            container_port: 8080,
            bind_port: random_port(),
            container_name: random_container_name(),
            timeout: 0,
            docker_compose_network_host: self.network_host.to_string(),
        };

        let container_id = runner(
            Some(self.docker.clone()),
            function_key,
            container_details.clone(),
        )
        .await?;
        container_details.container_id = container_id.clone();

        let container_info = ContainerInfo::new(
            container_id.clone(),
            container_details.container_name.clone(),
            container_details.container_port.clone(),
        );

        {
            let mut containers = self.containers.write().unwrap();
            containers.push(container_info);
        }

        info!(
            "Added container {} to pool for function {}",
            container_details.container_name, self.function_name
        );

        // Start monitoring this container
        let config = self.config.clone();
        let containers = self.containers.clone();
        let metrics_client = self.metrics_client.clone();

        tokio::spawn(async move {
            if let Err(e) =
                monitor_container_resources(container_id, config, containers, &metrics_client).await
            {
                error!("Failed to monitor container resources: {}", e);
            }
        });

        Ok(container_details)
    }

    /// Get the healthiest container for load balancing
    pub fn get_healthiest_container(&self) -> Option<ContainerDetails> {
        let containers = self.containers.read().unwrap();

        // Filter healthy containers and sort by CPU usage
        let mut healthy_containers: Vec<&ContainerInfo> = containers
            .iter()
            // TODO: pick idle within safe window or lock the container to it doesn't get cleaned up
            .filter(|c| c.status == ContainerStatus::Healthy || c.status == ContainerStatus::Idle)
            .collect();

        if healthy_containers.is_empty() {
            // If no healthy containers, try overloaded ones as last resort
            let overloaded: Vec<&ContainerInfo> = containers
                .iter()
                .filter(|c| c.status == ContainerStatus::Overloaded)
                .collect();

            if !overloaded.is_empty() {
                warn!(
                    "No healthy containers available for {}, using overloaded container",
                    self.function_name
                );
                return Some(toContainerDetails(overloaded[0]));
            }
            return None;
        }

        // Sort by CPU usage (ascending) and last active time
        healthy_containers.sort_by(|a, b| {
            a.cpu_usage
                .partial_cmp(&b.cpu_usage)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.last_active.cmp(&b.last_active))
        });

        Some(toContainerDetails(healthy_containers[0]))
    }

    /// Mark a container as active (just handled a request)
    pub fn mark_container_active(&self, container_id: &str) {
        let mut containers = self.containers.write().unwrap();
        if let Some(container) = containers.iter_mut().find(|c| c.id == container_id) {
            container.mark_active();
        }
    }

    /// Check if we need to scale up (all containers overloaded)
    pub fn needs_scale_up(&self) -> bool {
        let containers = self.containers.read().unwrap();

        if containers.len() >= self.max_containers {
            return false;
        }

        // Scale up if all containers are overloaded
        !containers.is_empty()
            && containers
                .iter()
                .all(|c| c.status == ContainerStatus::Overloaded)
    }

    /// Get containers eligible for scale-down
    pub fn get_scaledown_candidates(&self) -> Vec<String> {
        let containers = self.containers.read().unwrap();

        if containers.len() <= self.min_containers {
            return Vec::new();
        }

        containers
            .iter()
            .filter(|c| c.is_eligible_for_scaledown(self.config.cooldown_duration))
            .map(|c| c.id.clone())
            .collect()
    }

    /// Remove a container from the pool
    pub async fn remove_container(&self, container_id: &str) -> AppResult<()> {
        // Remove from Docker
        clean_up(&self.docker, container_id).await?;

        // Remove from our tracking
        {
            let mut containers = self.containers.write().unwrap();
            containers.retain(|c| c.id != container_id);
        }

        info!(
            "Removed container {} from pool for function {}",
            container_id, self.function_name
        );
        Ok(())
    }

    /// Get current container count
    pub fn container_count(&self) -> usize {
        self.containers.read().unwrap().len()
    }

    /// Get function name
    pub fn get_function_name(&self) -> &str {
        &self.function_name
    }

    /// Get all container IDs (for cleanup purposes)
    pub fn get_all_container_ids(&self) -> Vec<String> {
        let containers = self.containers.read().unwrap();
        containers.iter().map(|c| c.id.clone()).collect()
    }

    /// Get pool status for debugging
    pub fn get_status(&self) -> HashMap<String, Value> {
        let containers = self.containers.read().unwrap();
        let mut status = HashMap::new();

        status.insert(
            "function_name".to_string(),
            Value::String(self.function_name.clone()),
        );
        status.insert(
            "container_count".to_string(),
            Value::Number(containers.len().into()),
        );

        let healthy_count = containers
            .iter()
            .filter(|c| c.status == ContainerStatus::Healthy)
            .count();
        let overloaded_count = containers
            .iter()
            .filter(|c| c.status == ContainerStatus::Overloaded)
            .count();
        let idle_count = containers
            .iter()
            .filter(|c| c.status == ContainerStatus::Idle)
            .count();

        status.insert(
            "healthy_containers".to_string(),
            Value::Number(healthy_count.into()),
        );
        status.insert(
            "overloaded_containers".to_string(),
            Value::Number(overloaded_count.into()),
        );
        status.insert(
            "idle_containers".to_string(),
            Value::Number(idle_count.into()),
        );

        status
    }
}

/// Fetch container statistics from Prometheus
async fn fetch_container_stats(
    container_id: &str,
    metrics_client: &Arc<MetricsClient>,
) -> AppResult<(f64, f64)> {
    let cpu_percentage = metrics_client.get_container_cpu_usage(container_id).await?;
    let memory_percentage = metrics_client
        .get_container_memory_usage(container_id)
        .await?;
    Ok((cpu_percentage, memory_percentage))
}

/// Monitor container resources in a background task
async fn monitor_container_resources(
    container_id: String,
    config: MonitoringConfig,
    containers: Arc<RwLock<Vec<ContainerInfo>>>,
    metrics_client: &Arc<MetricsClient>,
) -> AppResult<()> {
    loop {
        sleep(config.poll_interval).await;
        // Fetch container stats
        match fetch_container_stats(&container_id, metrics_client).await {
            Ok((cpu_percentage, memory_percentage)) => {
                let mut containers_guard = containers.write().unwrap();
                if let Some(container) = containers_guard.iter_mut().find(|c| c.id == container_id)
                {
                    info!("=============>>>>>>>>>>> Updating container {} with CPU: {:.2}%, Memory: {:.2}% (source: Prometheus)",
                                 container.name, cpu_percentage, memory_percentage);
                    debug!("=============>>>>>>>>>>> Docker stats comparison for {}: check `docker stats --no-stream {}`",
                                 container.name, &container_id[0..12]);
                    container.update_metrics(
                        cpu_percentage,
                        memory_percentage,
                        config.cpu_overload_threshold,
                        config.memory_overload_threshold,
                        config.cooldown_cpu_threshold,
                    );
                }
            }
            Err(e) => {
                warn!("Failed to get stats for container {}: {}", container_id, e);
                // Container might be stopped, break the monitoring loop
                break;
            }
        }
    }

    Ok(())
}

fn toContainerDetails(container_info: &ContainerInfo) -> ContainerDetails {
    ContainerDetails {
        container_id: container_info.id.clone(),
        container_port: container_info.container_port,
        bind_port: "".to_string(),
        container_name: container_info.name.clone(),
        timeout: 0,
        docker_compose_network_host: "".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_info_status_transitions() {
        let mut container = ContainerInfo::new("test-id".to_string(), "test-name".to_string(), 0);

        // Test overload detection (80% CPU, 75% memory vs 70% thresholds)
        container.update_metrics(80.0, 75.0, 70.0, 70.0, 10.0);
        assert_eq!(container.status, ContainerStatus::Overloaded);

        // Test return to healthy (50% CPU, 50% memory vs 70% thresholds)
        container.update_metrics(50.0, 50.0, 70.0, 70.0, 10.0);
        assert_eq!(container.status, ContainerStatus::Healthy);

        // Test idle detection (5% CPU vs 10% cooldown threshold)
        container.update_metrics(5.0, 30.0, 70.0, 70.0, 10.0);
        assert_eq!(container.status, ContainerStatus::Idle);
        assert!(container.idle_since.is_some());
    }

    #[test]
    fn test_container_active_marking() {
        let mut container = ContainerInfo::new("test-id".to_string(), "test-name".to_string(), 0);

        // Make container idle (5% CPU vs 10% cooldown threshold)
        container.update_metrics(5.0, 30.0, 70.0, 70.0, 10.0);
        assert_eq!(container.status, ContainerStatus::Idle);

        // Mark as active should change status back to healthy
        container.mark_active();
        assert_eq!(container.status, ContainerStatus::Healthy);
        assert!(container.idle_since.is_none());
    }
}
