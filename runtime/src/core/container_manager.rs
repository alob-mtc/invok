use crate::core::metrics_client::MetricsClient;
use crate::core::runner::{clean_up, runner, ContainerDetails};
use crate::shared::error::AppResult;
use crate::shared::utils::{random_container_name, random_port};
use bollard::Docker;
use dashmap::DashMap;
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::task::JoinError;
use tracing::{debug, error, info, warn};

/// Container status enumeration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        cooldown_cpu_threshold: f64,
    ) {
        let old_status = self.status.clone();

        // Determine new status based on thresholds
        if cpu_usage > cpu_threshold || memory_usage > memory_threshold {
            self.status = ContainerStatus::Overloaded;
            self.idle_since = None;
        } else if cpu_usage <= cooldown_cpu_threshold {
            if self.status != ContainerStatus::Idle {
                self.idle_since = Some(Instant::now());
                self.status = ContainerStatus::Idle;
            }
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

    /// Check if container is within safe window
    pub fn is_within_safe_window(&self, cooldown_duration: Duration) -> bool {
        let safe_window = Duration::from_secs(5);
        if let Some(idle_since) = self.idle_since {
            self.status == ContainerStatus::Idle
                && idle_since.elapsed() <= cooldown_duration - safe_window
        } else {
            false
        }
    }
}

/// Configuration for container monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    pub cpu_overload_threshold: f64,
    pub memory_overload_threshold: f64,
    pub cooldown_cpu_threshold: f64,
    pub cooldown_duration: Duration,
    pub poll_interval: Duration,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            cpu_overload_threshold: 70.0,
            memory_overload_threshold: 70.0,
            cooldown_cpu_threshold: 10.0,
            cooldown_duration: Duration::from_secs(30),
            poll_interval: Duration::from_secs(2),
        }
    }
}

/// Container pool manager for a specific function
pub struct ContainerPool {
    /// Function name this pool manages
    function_name: String,
    /// List of containers in this pool
    containers: Arc<DashMap<String, ContainerInfo>>,
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
        // TODO: fetch from cache if already existing and build the pool

        Self {
            function_name,
            containers: Arc::new(DashMap::new()),
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
            container_details.container_port,
        );

        self.containers
            .insert(container_info.id.clone(), container_info.clone());

        info!(
            "Added container {} to pool for function {}",
            container_details.container_name, self.function_name
        );

        Ok(container_details)
    }

    /// Update container metrics
    pub async fn update_containers_metrics(&self) -> AppResult<()> {
        if self.containers.is_empty() {
            return Ok(());
        }

        let fn_name = &self.function_name;
        let total = self.containers.len();
        debug!(
            "Updating metrics for {} containers in pool for function {}",
            total, fn_name
        );

        // Snapshot all entries so we drop DashMap locks before .await
        let entries: Vec<(String, ContainerInfo)> = self
            .containers
            .iter()
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect();

        let handles: Vec<_> = entries
            .into_iter()
            .map(|(id, mut info)| {
                let containers = Arc::clone(&self.containers);
                let cfg = self.config.clone();
                let metrics_client = self.metrics_client.clone();

                tokio::spawn(async move {
                    if let Err(e) =
                        update_container_resources(id.clone(), cfg, &mut info, &metrics_client)
                            .await
                    {
                        error!("Failed to monitor container {}: {}", id, e);
                    }

                    debug!(
                        "Updating container {} with status {:?}",
                        info.name, info.status
                    );
                    containers.insert(id, info);
                })
            })
            .collect();

        let results: Vec<Result<(), JoinError>> = join_all(handles).await;
        for result in results {
            if let Err(join_err) = result {
                error!(
                    "Container‐update task panicked or was cancelled: {}",
                    join_err
                );
            }
        }

        debug!(
            "Finished updating metrics for pool for function {}",
            fn_name
        );
        Ok(())
    }

    /// Get the healthiest container for load balancing
    pub fn get_healthiest_container(&self) -> Option<ContainerDetails> {
        // Filter healthy containers and sort by last active time
        let mut healthy_containers: Vec<_> = self
            .containers
            .iter()
            .filter(|entry| {
                let container = entry.value();
                container.status == ContainerStatus::Healthy
                    || (container.status == ContainerStatus::Idle
                        && container.is_within_safe_window(self.config.cooldown_duration))
            })
            .map(|entry| entry.value().clone())
            .collect();

        if healthy_containers.is_empty() {
            // If no healthy containers, try overloaded ones as last resort
            let overloaded: Vec<_> = self
                .containers
                .iter()
                .filter(|entry| entry.value().status == ContainerStatus::Overloaded)
                .map(|entry| entry.value().clone())
                .collect();

            if !overloaded.is_empty() {
                warn!(
                    "No healthy containers available for {}, using overloaded container",
                    self.function_name
                );
                return Some(to_container_details(&overloaded[0]));
            }
            return None;
        }

        // Sort by last active time (oldest first for round-robin)
        healthy_containers.sort_by(|a, b| a.last_active.cmp(&b.last_active));

        Some(to_container_details(&healthy_containers[0]))
    }

    /// Mark a container as active (just handled a request)
    pub fn mark_container_active(&self, container_id: &str) {
        if let Some(mut entry) = self.containers.get_mut(container_id) {
            entry.mark_active();
        }
    }

    /// Check if we need to scale up (all containers overloaded)
    pub fn needs_scale_up(&self) -> bool {
        if self.containers.len() >= self.max_containers {
            return false;
        }

        // Scale up if all containers are overloaded
        !self.containers.is_empty()
            && self
                .containers
                .iter()
                .all(|entry| entry.value().status == ContainerStatus::Overloaded)
    }

    /// Get containers eligible for scale-down
    pub fn get_scaledown_candidates(&self) -> Vec<String> {
        if self.containers.is_empty() {
            return Vec::new();
        }

        self.containers
            .iter()
            .filter(|entry| {
                entry
                    .value()
                    .is_eligible_for_scaledown(self.config.cooldown_duration)
            })
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Remove a container from the pool
    pub async fn remove_container(&self, container_id: &str) -> AppResult<()> {
        self.containers.remove(container_id);

        // Remove from Docker (now safe to await without holding lock)
        clean_up(&self.docker, container_id).await?;

        info!(
            "Removed container {} from pool for function {}",
            container_id, self.function_name
        );

        Ok(())
    }

    /// Get current container count
    pub fn container_count(&self) -> usize {
        self.containers.len()
    }

    /// Get function name
    pub fn get_function_name(&self) -> &str {
        &self.function_name
    }

    /// Get pool status for debugging
    pub fn get_status(&self) -> HashMap<String, Value> {
        let mut status = HashMap::new();

        let containers_snapshot: Vec<_> = self
            .containers
            .iter()
            .map(|entry| entry.value().clone())
            .collect();

        let total_containers = containers_snapshot.len();
        let healthy_count = containers_snapshot
            .iter()
            .filter(|c| c.status == ContainerStatus::Healthy)
            .count();
        let overloaded_count = containers_snapshot
            .iter()
            .filter(|c| c.status == ContainerStatus::Overloaded)
            .count();
        let idle_count = containers_snapshot
            .iter()
            .filter(|c| c.status == ContainerStatus::Idle)
            .count();

        status.insert(
            "function_name".to_string(),
            Value::String(self.function_name.clone()),
        );
        status.insert(
            "total_containers".to_string(),
            Value::Number(serde_json::Number::from(total_containers)),
        );
        status.insert(
            "healthy_containers".to_string(),
            Value::Number(serde_json::Number::from(healthy_count)),
        );
        status.insert(
            "overloaded_containers".to_string(),
            Value::Number(serde_json::Number::from(overloaded_count)),
        );
        status.insert(
            "idle_containers".to_string(),
            Value::Number(serde_json::Number::from(idle_count)),
        );
        status.insert(
            "min_containers".to_string(),
            Value::Number(serde_json::Number::from(self.min_containers)),
        );
        status.insert(
            "max_containers".to_string(),
            Value::Number(serde_json::Number::from(self.max_containers)),
        );

        let containers_detail: Vec<Value> = containers_snapshot
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "name": c.name,
                    "port": c.container_port,
                    "status": format!("{:?}", c.status),
                    "last_active_ago_secs": c.last_active.elapsed().as_secs(),
                    "idle_since_secs": c.idle_since.map(|i| i.elapsed().as_secs()),
                })
            })
            .collect();

        status.insert("containers".to_string(), Value::Array(containers_detail));

        // Pool utilization metrics
        let capacity_utilization = if self.max_containers > 0 {
            (total_containers as f64 / self.max_containers as f64) * 100.0
        } else {
            0.0
        };

        status.insert(
            "capacity_utilization_percentage".to_string(),
            Value::Number(
                serde_json::Number::from_f64(capacity_utilization)
                    .unwrap_or_else(|| serde_json::Number::from(0)),
            ),
        );

        // Scale recommendations
        let needs_scale_up = healthy_count == 0 && total_containers < self.max_containers;
        let can_scale_down = idle_count > 0 && total_containers > self.min_containers;

        status.insert("needs_scale_up".to_string(), Value::Bool(needs_scale_up));
        status.insert("can_scale_down".to_string(), Value::Bool(can_scale_down));

        status
    }

    /// Convert current pool state to persistable format
    pub fn to_persisted_state(&self) -> crate::core::persistence::PersistedPoolState {
        use crate::core::persistence::{PersistedContainerInfo, PersistedPoolState};

        let containers: Vec<PersistedContainerInfo> = self
            .containers
            .iter()
            .map(|entry| PersistedContainerInfo::from_container_info(entry.value()))
            .collect();

        PersistedPoolState {
            function_name: self.function_name.clone(),
            containers,
            min_containers: self.min_containers,
            max_containers: self.max_containers,
            config: self.config.clone(),
            last_updated: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        }
    }

    /// Create pool from persisted state
    pub async fn from_persisted_state(
        persisted: crate::core::persistence::PersistedPoolState,
        docker: Docker,
        network_host: String,
        metrics_client: Arc<MetricsClient>,
    ) -> AppResult<Self> {
        let pool = Self {
            function_name: persisted.function_name,
            containers: Arc::new(DashMap::new()),
            docker,
            network_host,
            config: persisted.config,
            min_containers: persisted.min_containers,
            max_containers: persisted.max_containers,
            metrics_client,
        };

        // Restore containers from persisted state
        for persisted_container in persisted.containers {
            let container_info = persisted_container.to_container_info();
            pool.containers
                .insert(container_info.id.clone(), container_info);
        }

        info!(
            "Restored pool for {} with {} containers from persisted state",
            pool.function_name,
            pool.containers.len()
        );

        Ok(pool)
    }

    /// Validate that containers are still running and sync with Docker reality
    pub async fn validate_and_sync_containers(&self) -> AppResult<()> {
        let container_ids: Vec<String> = self
            .containers
            .iter()
            .map(|entry| entry.key().clone())
            .collect();

        let mut invalid_containers = Vec::new();

        for container_id in container_ids {
            // Check if container exists and is running
            match self.docker.inspect_container(&container_id, None).await {
                Ok(inspect_response) => {
                    let is_running = inspect_response
                        .state
                        .as_ref()
                        .and_then(|state| state.running)
                        .unwrap_or(false);

                    if !is_running {
                        warn!(
                            "Container {} for function {} is not running, removing from pool",
                            container_id, self.function_name
                        );
                        invalid_containers.push(container_id);
                    } else {
                        debug!("Container {} validated as running", container_id);
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to inspect container {} for function {}: {}, removing from pool",
                        container_id, self.function_name, e
                    );
                    invalid_containers.push(container_id);
                }
            }
        }

        // Remove invalid containers from pool
        for container_id in invalid_containers {
            self.containers.remove(&container_id);
        }

        info!(
            "Container validation complete for {}: {} containers remain",
            self.function_name,
            self.containers.len()
        );

        Ok(())
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

/// update container resources
async fn update_container_resources(
    container_id: String,
    config: MonitoringConfig,
    container: &mut ContainerInfo,
    metrics_client: &Arc<MetricsClient>,
) -> AppResult<()> {
    // Fetch container stats
    match fetch_container_stats(&container_id, metrics_client).await {
        Ok((cpu_percentage, memory_percentage)) => {
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
        Err(e) => {
            warn!("Failed to get stats for container {}: {}", container_id, e);
            // Container might be stopped, break the monitoring loop
        }
    }
    Ok(())
}

fn to_container_details(container_info: &ContainerInfo) -> ContainerDetails {
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
        container.update_metrics(0.00, 30.0, 70.0, 70.0, 0.0);
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
