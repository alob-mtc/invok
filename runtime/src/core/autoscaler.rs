use crate::core::container_manager::{ContainerPool, MonitoringConfig};
use crate::core::metrics_client::MetricsClient;
use crate::core::runner::{runner, ContainerDetails};
use crate::shared::error::{AppResult, RuntimeError};
use crate::shared::utils::{random_container_name, random_port};
use bollard::Docker;
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::time::{interval, sleep};
use tracing::{debug, error, info, warn};

/// Autoscaler configuration
#[derive(Debug, Clone)]
pub struct AutoscalerConfig {
    pub monitoring: MonitoringConfig,
    pub min_containers_per_function: usize,
    pub max_containers_per_function: usize,
    pub scale_check_interval: Duration,
}

/// Main autoscaler that manages container pools for all functions
pub struct Autoscaler {
    /// Container pools indexed by function key (function_name-user_hash)
    pools: Arc<DashMap<String, Arc<ContainerPool>>>,
    /// Docker client
    docker: Docker,
    /// Configuration
    config: AutoscalerConfig,
    /// Network host for containers
    docker_compose_network_host: String,
    /// Optional metrics client for Prometheus
    metrics_client: Arc<MetricsClient>,
}

impl Autoscaler {
    pub fn new(
        docker: Docker,
        config: AutoscalerConfig,
        docker_compose_network_host: String,
        metrics_client: MetricsClient,
    ) -> Self {
        Self {
            pools: Arc::new(DashMap::new()),
            docker,
            config,
            docker_compose_network_host,
            metrics_client: Arc::new(metrics_client),
        }
    }

    /// Start the autoscaler background tasks
    pub async fn start(&self) -> AppResult<()> {
        info!("Starting autoscaler with config: {:?}", self.config);

        let pools = self.pools.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let mut interval = interval(config.scale_check_interval);
            loop {
                interval.tick().await;
                debug!("Autoscaler scan start...\n");
                // Get a snapshot of current pools to avoid holding the lock across await
                let pool_snapshot: Vec<_> = pools
                    .iter()
                    .map(|entry| (entry.key().clone(), entry.value().clone()))
                    .collect();
                // Process each pool without holding the main lock
                for (function_key, pool) in pool_snapshot {
                    let _ = pool.update_containers_metrics().await;
                    info!("Autoscaler state: {:?} \n\n", pool.get_status());
                    // Update pool metrics
                    if let Err(e) =
                        Self::check_and_scale_down_pool(function_key.as_str(), pool, &config).await
                    {
                        error!("Failed to check/scale pool for {}: {}", function_key, e);
                    }
                }
                debug!("Autoscaler scan end\n");
            }
        });

        Ok(())
    }

    /// Get or create a container pool for a function
    pub async fn get_or_create_pool(&self, function_key: &str) -> Arc<ContainerPool> {
        if let Some(pool) = self.pools.get(function_key) {
            debug!(
                "Using existing container pool for function: {}",
                function_key
            );
            return pool.clone();
        }

        // Create new pool
        let pool = ContainerPool::new(
            function_key.to_string(),
            self.docker.clone(),
            self.docker_compose_network_host.clone(),
            self.config.monitoring.clone(),
            self.config.min_containers_per_function,
            self.config.max_containers_per_function,
            self.metrics_client.clone(),
        );

        debug!("Creating new container pool for function: {}", function_key);
        let pool = Arc::new(pool);
        self.pools.insert(function_key.to_string(), pool.clone());

        info!("Created new container pool for function: {}", function_key);
        pool
    }

    /// Get the best container for a function invocation
    pub async fn get_container_for_invocation(
        &self,
        function_key: &str,
    ) -> Option<ContainerDetails> {
        let pool = self.get_or_create_pool(function_key).await;

        // Try to get a healthy container
        if let Some(container) = pool.get_healthiest_container() {
            pool.mark_container_active(&container.container_id);
            return Some(container);
        }

        debug!(
            "No healthy containers available for function {}, checking for scale-up",
            function_key
        );
        // If no containers available, try to scale up immediately
        if pool.container_count() < self.config.max_containers_per_function {
            match Self::scale_up_function(function_key, Arc::clone(&pool)).await {
                Ok(container) => {
                    pool.mark_container_active(&container.container_id);
                    Some(container)
                }
                Err(e) => {
                    error!(
                        "Failed to scale up function {} for immediate request: {}",
                        function_key, e
                    );
                    None
                }
            }
        } else {
            warn!(
                "No available containers for function {} and max capacity reached",
                function_key
            );
            None
        }
    }

    /// Get status of all pools for monitoring/debugging
    pub fn get_all_pool_status(&self) -> HashMap<String, serde_json::Value> {
        self.pools
            .iter()
            .map(|entry| {
                (
                    entry.key().clone(),
                    serde_json::json!(entry.value().get_status()),
                )
            })
            .collect()
    }

    /// Get the autoscaler configuration
    pub fn get_config(&self) -> &AutoscalerConfig {
        &self.config
    }

    /// Check and scale a specific pool
    async fn check_and_scale_down_pool(
        function_key: &str,
        pool: Arc<ContainerPool>,
        config: &AutoscalerConfig,
    ) -> AppResult<()> {
        // Check for scale-down opportunities
        let candidates = pool.get_scaledown_candidates();
        for container_id in candidates {
            if pool.container_count() > config.min_containers_per_function {
                if let Err(e) = pool.remove_container(&container_id).await {
                    error!("Failed to scale down container {}: {}", container_id, e);
                } else {
                    info!(
                        "Scaled down container {} for function {}",
                        container_id, function_key
                    );
                }
            }
        }

        Ok(())
    }

    /// Scale up a function by adding a new container
    async fn scale_up_function(
        function_key: &str,
        pool: Arc<ContainerPool>,
    ) -> AppResult<ContainerDetails> {
        info!("Scaling up function: {}", function_key);
        // Add the container to the pool
        let container_details = pool.add_container(function_key).await?;

        info!(
            "Successfully scaled up function {} with container {}",
            function_key, container_details.container_name
        );

        Ok(container_details)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::metrics_client::MetricsConfig;
    use std::time::Duration;

    fn create_test_config() -> AutoscalerConfig {
        AutoscalerConfig {
            monitoring: MonitoringConfig {
                cpu_overload_threshold: 70.0,
                memory_overload_threshold: 70.0,
                cooldown_cpu_threshold: 0.1,
                cooldown_duration: Duration::from_secs(30),
                poll_interval: Duration::from_secs(2),
                prometheus_url: "http://prometheus:9090".to_string(),
            },
            min_containers_per_function: 1,
            max_containers_per_function: 5,
            scale_check_interval: Duration::from_secs(10),
        }
    }

    #[tokio::test]
    async fn test_autoscaler_creation() {
        let docker = Docker::connect_with_http_defaults().unwrap();
        let config = create_test_config();
        let autoscaler = Autoscaler::new(
            docker,
            config,
            "test-network".to_string(),
            MetricsClient::new(MetricsConfig::default()),
        );

        assert_eq!(autoscaler.pools.len(), 0);
    }

    #[tokio::test]
    async fn test_pool_creation() {
        let docker = Docker::connect_with_http_defaults().unwrap();
        let config = create_test_config();
        let autoscaler = Autoscaler::new(
            docker,
            config,
            "test-network".to_string(),
            MetricsClient::new(MetricsConfig::default()),
        );

        let pool = autoscaler.get_or_create_pool("test-function").await;
        assert_eq!(pool.get_function_name(), "test-function");
        assert_eq!(autoscaler.pools.len(), 1);

        // Getting the same pool should return the existing one
        let pool2 = autoscaler.get_or_create_pool("test-function").await;
        assert_eq!(autoscaler.pools.len(), 1);
    }
}
