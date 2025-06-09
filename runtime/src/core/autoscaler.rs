use crate::core::container_manager::{ContainerPool, MonitoringConfig};
use crate::core::metrics_client::MetricsClient;
use crate::core::persistence::{AutoscalerPersistence, PersistenceConfig, PersistenceMetadata};
use crate::core::runner::ContainerDetails;
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
    /// Redis persistence handler
    persistence: Option<Arc<AutoscalerPersistence>>,
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
            persistence: None,
        }
    }

    /// Add Redis persistence to the autoscaler
    pub fn with_persistence(mut self, persistence_config: PersistenceConfig) -> AppResult<Self> {
        if persistence_config.enabled {
            let persistence = AutoscalerPersistence::new(persistence_config)?;
            self.persistence = Some(Arc::new(persistence));
            info!("Autoscaler persistence enabled");
        } else {
            info!("Autoscaler persistence disabled");
        }
        Ok(self)
    }

    /// Restore autoscaler state from Redis using individual pool loading
    pub async fn restore_from_redis(&self) -> AppResult<()> {
        let persistence = match &self.persistence {
            Some(p) => p,
            None => {
                debug!("Persistence not enabled, skipping state restoration");
                return Ok(());
            }
        };

        // Load metadata first (optional)
        if let Ok(Some(metadata)) = persistence.load_metadata().await {
            info!(
                "Found persistence metadata: version={}, total_pools={}",
                metadata.version, metadata.total_pools
            );
        }

        // Load all pool states in parallel batches
        let persisted_pools = match persistence.load_all_pool_states().await {
            Ok(pools) => pools,
            Err(e) => {
                error!("Failed to load pool states from Redis: {}", e);
                return Err(e);
            }
        };

        if persisted_pools.is_empty() {
            info!("No pool states to restore from Redis, starting fresh");
            return Ok(());
        }

        info!("Restoring {} pools from Redis", persisted_pools.len());

        let mut restored_count = 0;
        let mut failed_count = 0;

        for (function_key, persisted_pool) in persisted_pools {
            match ContainerPool::from_persisted_state(
                persisted_pool,
                self.docker.clone(),
                self.docker_compose_network_host.clone(),
                self.metrics_client.clone(),
            )
            .await
            {
                Ok(pool) => {
                    // Validate containers are still running
                    if let Err(e) = pool.validate_and_sync_containers().await {
                        warn!("Failed to validate containers for {}: {}", function_key, e);
                    }

                    // Only insert if we still have containers after validation
                    if pool.container_count() > 0 {
                        self.pools.insert(function_key.clone(), Arc::new(pool));
                        restored_count += 1;
                        info!(
                            "Restored pool for {} with {} containers",
                            function_key,
                            self.pools.get(&function_key).unwrap().container_count()
                        );
                    } else {
                        warn!("Pool for {} had no valid containers after validation, removing from Redis", function_key);
                        // Clean up the empty pool from Redis
                        if let Err(e) = persistence.delete_pool_state(&function_key).await {
                            warn!(
                                "Failed to delete empty pool state for {}: {}",
                                function_key, e
                            );
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to restore pool for {}: {}", function_key, e);
                    failed_count += 1;
                }
            }
        }

        info!(
            "State restoration complete: {} pools restored, {} failed",
            restored_count, failed_count
        );

        // Update metadata with current state
        let metadata = PersistenceMetadata::new(self.pools.len());
        if let Err(e) = persistence.save_metadata(&metadata).await {
            warn!("Failed to update persistence metadata: {}", e);
        }

        // Clean up any stale pool states in Redis
        let active_keys: Vec<String> = self.pools.iter().map(|e| e.key().clone()).collect();
        if let Err(e) = persistence.cleanup_stale_pools(&active_keys).await {
            warn!("Failed to cleanup stale pools: {}", e);
        }

        Ok(())
    }

    /// Save individual pool state to Redis
    async fn save_pool_state(
        &self,
        function_key: &str,
        pool: &Arc<ContainerPool>,
    ) -> AppResult<()> {
        let persistence = match &self.persistence {
            Some(p) => p,
            None => return Ok(()),
        };

        let persisted_pool = pool.to_persisted_state();
        persistence
            .save_pool_state(function_key, &persisted_pool)
            .await
    }

    /// Start the autoscaler background tasks (scaling only, no periodic snapshots)
    pub async fn start(&self) -> AppResult<()> {
        info!("Starting autoscaler with config: {:?}", self.config);

        // Restore state from Redis if persistence is enabled
        self.restore_from_redis().await?;

        let pools = self.pools.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let mut scale_interval = interval(config.scale_check_interval);

            loop {
                scale_interval.tick().await;
                debug!("Autoscaler scan start...\n");
                // Get a snapshot of current pools to avoid holding the lock across await
                let pool_snapshot: Vec<_> = pools
                    .iter()
                    .map(|entry| (entry.key().clone(), entry.value().clone()))
                    .collect();
                // Process each pool without holding the main lock
                for (function_key, pool) in pool_snapshot {
                    // Update pool metrics
                    let _ = pool.update_containers_metrics().await;
                    info!("Autoscaler state: {:?} \n\n", pool.get_status());

                    // Check for scale-up needs
                    if pool.needs_scale_up() {
                        if let Err(e) = Self::scale_up_function(&function_key, pool.clone()).await {
                            error!("Failed to scale up pool for {}: {}", function_key, e);
                        }
                    }

                    // Check and scale down if needed
                    let _ =
                        Self::check_and_scale_down_pool(function_key.as_str(), pool, &config).await;
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

        // Save new pool state to Redis
        if let Err(e) = self.save_pool_state(function_key, &pool).await {
            warn!("Failed to save new pool state for {}: {}", function_key, e);
        }

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

            // Save updated pool state after marking container active
            if let Err(e) = self.save_pool_state(function_key, &pool).await {
                warn!(
                    "Failed to save pool state after container activation for {}: {}",
                    function_key, e
                );
            }

            return Some(container);
        }

        // If no containers available, try to scale up immediately
        if pool.container_count() < self.config.max_containers_per_function {
            match Self::scale_up_function(function_key, Arc::clone(&pool)).await {
                Ok(container) => {
                    pool.mark_container_active(&container.container_id);

                    // Save updated pool state after scaling up
                    if let Err(e) = self.save_pool_state(function_key, &pool).await {
                        warn!(
                            "Failed to save pool state after scale up for {}: {}",
                            function_key, e
                        );
                    }

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
