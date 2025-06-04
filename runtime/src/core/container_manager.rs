use crate::core::runner::{clean_up, runner, ContainerDetails};
use crate::shared::error::{AppResult, RuntimeError};
use bollard::container::{RemoveContainerOptions, StatsOptions};
use bollard::Docker;
use futures_util::StreamExt;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};
use crate::shared::utils::{random_container_name, random_port};

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
    /// Current CPU usage (0.0-1.0)
    pub cpu_usage: f64,
    /// Current memory usage in bytes
    pub memory_usage: u64,
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
            memory_usage: 0,
            status: ContainerStatus::Healthy,
            last_active: Instant::now(),
            idle_since: None,
        }
    }

    /// Update container metrics and status
    pub fn update_metrics(
        &mut self,
        cpu_usage: f64,
        memory_usage: u64,
        cpu_threshold: f64,
        memory_threshold: u64,
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
    pub memory_overload_threshold: u64,
    pub cooldown_cpu_threshold: f64,
    pub cooldown_duration: Duration,
    pub poll_interval: Duration,
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
}

impl ContainerPool {
    pub fn new(
        function_name: String,
        docker: Docker,
        network_host: String,
        config: MonitoringConfig,
        min_containers: usize,
        max_containers: usize,
    ) -> Self {
        Self {
            function_name,
            containers: Arc::new(RwLock::new(Vec::new())),
            docker,
            network_host,
            config,
            min_containers,
            max_containers,
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
            timeout: 300,
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
        let docker = self.docker.clone();
        let config = self.config.clone();
        let containers = self.containers.clone();

        tokio::spawn(async move {
            if let Err(e) =
                monitor_container_resources(container_id, docker, config, containers).await
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

/// Fetch container statistics from Docker
async fn fetch_container_stats(container_id: &str, docker: &Docker) -> AppResult<(f64, u64)> {
    let mut stats_stream = docker.stats(
        container_id,
        Some(StatsOptions {
            stream: false,
            one_shot: true,
        }),
    );

    if let Some(stats_result) = stats_stream.next().await {
        let stats = stats_result
            .map_err(|e| RuntimeError::System(format!("Failed to get container stats: {e}")))?;

        // Convert stats to JSON for easier parsing
        let stats_json = serde_json::to_value(&stats)
            .map_err(|e| RuntimeError::System(format!("Failed to serialize stats: {e}")))?;

        // Parse CPU and memory from the stats response
        let cpu_usage = extract_cpu_percentage(&stats_json)?;
        let memory_usage = extract_memory_usage(&stats_json)?;

        Ok((cpu_usage, memory_usage))
    } else {
        Err(RuntimeError::System(
            "No stats available for container".to_string(),
        ))
    }
}

/// Extract CPU percentage from stats response
fn extract_cpu_percentage(stats: &Value) -> AppResult<f64> {
    // Try to extract CPU stats from the JSON response
    let cpu_stats = stats
        .get("cpu_stats")
        .ok_or_else(|| RuntimeError::System("No CPU stats available".to_string()))?;

    let precpu_stats = stats
        .get("precpu_stats")
        .ok_or_else(|| RuntimeError::System("No previous CPU stats available".to_string()))?;

    let cpu_total = cpu_stats
        .get("cpu_usage")
        .and_then(|usage| usage.get("total_usage"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as f64;

    let precpu_total = precpu_stats
        .get("cpu_usage")
        .and_then(|usage| usage.get("total_usage"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as f64;

    let cpu_delta = cpu_total - precpu_total;

    let system_delta = cpu_stats
        .get("system_cpu_usage")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as f64
        - precpu_stats
            .get("system_cpu_usage")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as f64;

    let number_cpus = cpu_stats
        .get("online_cpus")
        .and_then(|v| v.as_u64())
        .unwrap_or(1) as f64;

    if system_delta > 0.0 && cpu_delta > 0.0 {
        Ok((cpu_delta / system_delta) * number_cpus)
    } else {
        Ok(0.0)
    }
}

/// Extract memory usage from stats response
fn extract_memory_usage(stats: &Value) -> AppResult<u64> {
    stats
        .get("memory_stats")
        .and_then(|mem| mem.get("usage"))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RuntimeError::System("No memory usage available".to_string()))
}

/// Monitor container resources in a background task
async fn monitor_container_resources(
    container_id: String,
    docker: Docker,
    config: MonitoringConfig,
    containers: Arc<RwLock<Vec<ContainerInfo>>>,
) -> AppResult<()> {
    loop {
        sleep(config.poll_interval).await;

        // Fetch container stats
        match fetch_container_stats(&container_id, &docker).await {
            Ok((cpu_usage, memory_usage)) => {
                // Update the container info
                {
                    let mut containers_guard = containers.write().unwrap();
                    if let Some(container) =
                        containers_guard.iter_mut().find(|c| c.id == container_id)
                    {
                        container.update_metrics(
                            cpu_usage,
                            memory_usage,
                            config.cpu_overload_threshold,
                            config.memory_overload_threshold,
                            config.cooldown_cpu_threshold,
                        );
                    }
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
        let mut container = ContainerInfo::new(
            "test-id".to_string(),
            "test-name".to_string(),
            0,
        );

        // Test overload detection
        container.update_metrics(0.8, 400_000_000, 0.7, 300_000_000, 0.1);
        assert_eq!(container.status, ContainerStatus::Overloaded);

        // Test return to healthy
        container.update_metrics(0.5, 200_000_000, 0.7, 300_000_000, 0.1);
        assert_eq!(container.status, ContainerStatus::Healthy);

        // Test idle detection
        container.update_metrics(0.05, 100_000_000, 0.7, 300_000_000, 0.1);
        assert_eq!(container.status, ContainerStatus::Idle);
        assert!(container.idle_since.is_some());
    }

    #[test]
    fn test_container_active_marking() {
        let mut container = ContainerInfo::new(
            "test-id".to_string(),
            "test-name".to_string(),
            0,
        );

        // Make container idle
        container.update_metrics(0.05, 100_000_000, 0.7, 300_000_000, 0.1);
        assert_eq!(container.status, ContainerStatus::Idle);

        // Mark as active should change status back to healthy
        container.mark_active();
        assert_eq!(container.status, ContainerStatus::Healthy);
        assert!(container.idle_since.is_none());
    }
}
