use crate::core::container_manager::{ContainerInfo, ContainerStatus, MonitoringConfig};
use crate::shared::error::{AppResult, RuntimeError};
use futures_util::future::join_all;
use redis::{aio::MultiplexedConnection, AsyncCommands, Client};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};

/// Configuration for autoscaler persistence
#[derive(Debug, Clone)]
pub struct PersistenceConfig {
    pub enabled: bool,
    pub redis_url: String,
    pub key_prefix: String,
    pub batch_size: usize, // Number of pools to load in parallel during recovery
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            redis_url: "redis://localhost:6379".to_string(),
            key_prefix: "autoscaler".to_string(),
            batch_size: 50, // Load 50 pools at a time during recovery
        }
    }
}

/// Serializable version of ContainerInfo for Redis storage
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PersistedContainerInfo {
    pub id: String,
    pub name: String,
    pub container_port: u32,
    pub status: ContainerStatus,
    pub last_active_unix: i64,
    pub idle_since_unix: Option<i64>,
}

impl PersistedContainerInfo {
    /// Convert from ContainerInfo to persistable format
    pub fn from_container_info(container: &ContainerInfo) -> Self {
        let last_active_unix = container.last_active.elapsed().as_secs().saturating_sub(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        ) as i64;

        let idle_since_unix = container.idle_since.map(|instant| {
            instant.elapsed().as_secs().saturating_sub(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            ) as i64
        });

        Self {
            id: container.id.clone(),
            name: container.name.clone(),
            container_port: container.container_port,
            status: container.status.clone(),
            last_active_unix,
            idle_since_unix,
        }
    }

    /// Convert to ContainerInfo with current timestamps
    pub fn to_container_info(&self) -> ContainerInfo {
        let now = std::time::Instant::now();
        let last_active = now
            .checked_sub(Duration::from_secs(
                (SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64
                    - self.last_active_unix)
                    .max(0) as u64,
            ))
            .unwrap_or(now);

        let idle_since = self.idle_since_unix.and_then(|unix_time| {
            now.checked_sub(Duration::from_secs(
                (SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64
                    - unix_time)
                    .max(0) as u64,
            ))
        });

        ContainerInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            container_port: self.container_port,
            status: self.status.clone(),
            last_active,
            idle_since,
        }
    }
}

/// Serializable version of container pool state
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PersistedPoolState {
    pub function_name: String,
    pub containers: Vec<PersistedContainerInfo>,
    pub min_containers: usize,
    pub max_containers: usize,
    pub config: MonitoringConfig,
    pub last_updated: i64, // When this pool was last updated
}

/// Lightweight metadata for the persistence system
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PersistenceMetadata {
    pub version: String,
    pub last_cleanup: i64,
    pub total_pools: usize,
}

impl PersistenceMetadata {
    pub fn new(total_pools: usize) -> Self {
        Self {
            version: "1.0".to_string(),
            last_cleanup: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
            total_pools,
        }
    }
}

/// Redis persistence handler for autoscaler state using individual pool storage
pub struct AutoscalerPersistence {
    redis_client: Client,
    config: PersistenceConfig,
}

impl AutoscalerPersistence {
    /// Create new persistence handler
    pub fn new(config: PersistenceConfig) -> AppResult<Self> {
        let redis_client = Client::open(config.redis_url.clone()).map_err(|e| {
            error!("Failed to create Redis client: {}", e);
            RuntimeError::RedisError(format!("Failed to create Redis client: {}", e))
        })?;

        Ok(Self {
            redis_client,
            config,
        })
    }

    /// Get Redis connection
    async fn get_connection(&self) -> AppResult<MultiplexedConnection> {
        self.redis_client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| {
                error!("Failed to get Redis connection: {}", e);
                RuntimeError::RedisError(format!("Failed to get Redis connection: {}", e))
            })
    }

    /// Generate Redis key with prefix
    fn pool_key(&self, function_key: &str) -> String {
        format!("{}:pool:{}", self.config.key_prefix, function_key)
    }

    /// Generate metadata key
    fn metadata_key(&self) -> String {
        format!("{}:metadata", self.config.key_prefix)
    }

    /// Save individual pool state to Redis
    pub async fn save_pool_state(
        &self,
        function_key: &str,
        pool_state: &PersistedPoolState,
    ) -> AppResult<()> {
        if !self.config.enabled {
            return Ok(());
        }

        let mut conn = self.get_connection().await?;
        let key = self.pool_key(function_key);

        let serialized = serde_json::to_string(pool_state).map_err(|e| {
            error!("Failed to serialize pool state for {}: {}", function_key, e);
            RuntimeError::SerializationError(format!("Failed to serialize pool state: {}", e))
        })?;

        conn.set(&key, &serialized).await.map_err(|e| {
            error!(
                "Failed to save pool state for {} to Redis: {}",
                function_key, e
            );
            RuntimeError::RedisError(format!("Failed to save pool state: {}", e))
        })?;

        // Set expiration (24 hours)
        conn.expire(&key, 24 * 60 * 60).await.map_err(|e| {
            warn!(
                "Failed to set expiration on pool state for {}: {}",
                function_key, e
            );
            RuntimeError::RedisError(format!("Failed to set expiration: {}", e))
        })?;

        debug!(
            "Saved pool state for {} with {} containers",
            function_key,
            pool_state.containers.len()
        );
        Ok(())
    }

    /// Load individual pool state from Redis
    pub async fn load_pool_state(
        &self,
        function_key: &str,
    ) -> AppResult<Option<PersistedPoolState>> {
        if !self.config.enabled {
            return Ok(None);
        }

        let mut conn = self.get_connection().await?;
        let key = self.pool_key(function_key);

        let serialized: Option<String> = conn.get(&key).await.map_err(|e| {
            error!(
                "Failed to load pool state for {} from Redis: {}",
                function_key, e
            );
            RuntimeError::RedisError(format!("Failed to load pool state: {}", e))
        })?;

        match serialized {
            Some(data) => {
                let pool_state: PersistedPoolState = serde_json::from_str(&data).map_err(|e| {
                    error!(
                        "Failed to deserialize pool state for {}: {}",
                        function_key, e
                    );
                    RuntimeError::SerializationError(format!(
                        "Failed to deserialize pool state: {}",
                        e
                    ))
                })?;

                debug!(
                    "Loaded pool state for {} with {} containers",
                    function_key,
                    pool_state.containers.len()
                );
                Ok(Some(pool_state))
            }
            None => {
                debug!("No pool state found for {} in Redis", function_key);
                Ok(None)
            }
        }
    }

    /// Get all function keys that have persisted pool state
    pub async fn get_all_pool_keys(&self) -> AppResult<Vec<String>> {
        if !self.config.enabled {
            return Ok(Vec::new());
        }

        let mut conn = self.get_connection().await?;
        let pattern = format!("{}:pool:*", self.config.key_prefix);

        let keys: Vec<String> = conn.keys(&pattern).await.map_err(|e| {
            error!("Failed to get pool keys from Redis: {}", e);
            RuntimeError::RedisError(format!("Failed to get pool keys: {}", e))
        })?;

        // Extract function keys from Redis keys
        let function_keys: Vec<String> = keys
            .into_iter()
            .filter_map(|key| {
                let pool_prefix = format!("{}:pool:", self.config.key_prefix);
                key.strip_prefix(&pool_prefix).map(|s| s.to_string())
            })
            .collect();

        info!(
            "Found {} persisted pool states in Redis",
            function_keys.len()
        );
        Ok(function_keys)
    }

    /// Load all pool states in parallel batches for efficient recovery
    pub async fn load_all_pool_states(&self) -> AppResult<HashMap<String, PersistedPoolState>> {
        if !self.config.enabled {
            return Ok(HashMap::new());
        }

        let function_keys = self.get_all_pool_keys().await?;
        if function_keys.is_empty() {
            info!("No pool states to restore from Redis");
            return Ok(HashMap::new());
        }

        info!(
            "Loading {} pool states from Redis in batches of {}",
            function_keys.len(),
            self.config.batch_size
        );

        let mut all_pools = HashMap::new();
        let mut successful_loads = 0;
        let mut failed_loads = 0;

        // Process in batches for better performance and memory usage
        for chunk in function_keys.chunks(self.config.batch_size) {
            let load_tasks: Vec<_> = chunk
                .iter()
                .map(|function_key| {
                    let function_key = function_key.clone();
                    let persistence = self;
                    async move {
                        match persistence.load_pool_state(&function_key).await {
                            Ok(Some(pool_state)) => Some((function_key, pool_state)),
                            Ok(None) => {
                                warn!("Pool state not found for {}", function_key);
                                None
                            }
                            Err(e) => {
                                error!("Failed to load pool state for {}: {}", function_key, e);
                                None
                            }
                        }
                    }
                })
                .collect();

            let results = join_all(load_tasks).await;

            for result in results {
                if let Some((function_key, pool_state)) = result {
                    all_pools.insert(function_key, pool_state);
                    successful_loads += 1;
                } else {
                    failed_loads += 1;
                }
            }

            debug!("Loaded batch of {} pools", chunk.len());
        }

        info!(
            "Pool state loading complete: {} successful, {} failed",
            successful_loads, failed_loads
        );

        Ok(all_pools)
    }

    /// Delete individual pool state
    pub async fn delete_pool_state(&self, function_key: &str) -> AppResult<()> {
        if !self.config.enabled {
            return Ok(());
        }

        let mut conn = self.get_connection().await?;
        let key = self.pool_key(function_key);

        conn.del(&key).await.map_err(|e| {
            error!("Failed to delete pool state for {}: {}", function_key, e);
            RuntimeError::RedisError(format!("Failed to delete pool state: {}", e))
        })?;

        debug!("Deleted pool state for {}", function_key);
        Ok(())
    }

    /// Clean up stale pool states from Redis
    pub async fn cleanup_stale_pools(&self, active_function_keys: &[String]) -> AppResult<()> {
        if !self.config.enabled {
            return Ok(());
        }

        let all_persisted_keys = self.get_all_pool_keys().await?;
        let mut deleted_count = 0;

        for persisted_key in all_persisted_keys {
            if !active_function_keys.contains(&persisted_key) {
                match self.delete_pool_state(&persisted_key).await {
                    Ok(_) => {
                        deleted_count += 1;
                        debug!("Deleted stale pool state for {}", persisted_key);
                    }
                    Err(e) => {
                        warn!(
                            "Failed to delete stale pool state for {}: {}",
                            persisted_key, e
                        );
                    }
                }
            }
        }

        if deleted_count > 0 {
            info!("Cleaned up {} stale pool states from Redis", deleted_count);
        }

        Ok(())
    }

    /// Save metadata about the persistence system
    pub async fn save_metadata(&self, metadata: &PersistenceMetadata) -> AppResult<()> {
        if !self.config.enabled {
            return Ok(());
        }

        let mut conn = self.get_connection().await?;
        let key = self.metadata_key();

        let serialized = serde_json::to_string(metadata).map_err(|e| {
            error!("Failed to serialize metadata: {}", e);
            RuntimeError::SerializationError(format!("Failed to serialize metadata: {}", e))
        })?;

        conn.set(&key, &serialized).await.map_err(|e| {
            error!("Failed to save metadata to Redis: {}", e);
            RuntimeError::RedisError(format!("Failed to save metadata: {}", e))
        })?;

        // Set expiration (24 hours)
        conn.expire(&key, 24 * 60 * 60).await.map_err(|e| {
            warn!("Failed to set expiration on metadata: {}", e);
            RuntimeError::RedisError(format!("Failed to set expiration: {}", e))
        })?;

        debug!("Saved persistence metadata");
        Ok(())
    }

    /// Load metadata about the persistence system
    pub async fn load_metadata(&self) -> AppResult<Option<PersistenceMetadata>> {
        if !self.config.enabled {
            return Ok(None);
        }

        let mut conn = self.get_connection().await?;
        let key = self.metadata_key();

        let serialized: Option<String> = conn.get(&key).await.map_err(|e| {
            error!("Failed to load metadata from Redis: {}", e);
            RuntimeError::RedisError(format!("Failed to load metadata: {}", e))
        })?;

        match serialized {
            Some(data) => {
                let metadata: PersistenceMetadata = serde_json::from_str(&data).map_err(|e| {
                    error!("Failed to deserialize metadata: {}", e);
                    RuntimeError::SerializationError(format!(
                        "Failed to deserialize metadata: {}",
                        e
                    ))
                })?;

                debug!(
                    "Loaded persistence metadata (version: {}, total_pools: {})",
                    metadata.version, metadata.total_pools
                );
                Ok(Some(metadata))
            }
            None => {
                debug!("No persistence metadata found in Redis");
                Ok(None)
            }
        }
    }

    /// Check if persistence is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_container_info_conversion() {
        let original = ContainerInfo {
            id: "test-id".to_string(),
            name: "test-container".to_string(),
            container_port: 8080,
            status: ContainerStatus::Healthy,
            last_active: Instant::now(),
            idle_since: None,
        };

        let persisted = PersistedContainerInfo::from_container_info(&original);
        let converted = persisted.to_container_info();

        assert_eq!(original.id, converted.id);
        assert_eq!(original.name, converted.name);
        assert_eq!(original.container_port, converted.container_port);
        assert_eq!(original.status, converted.status);
    }

    #[test]
    fn test_container_info_conversion_with_idle() {
        let original = ContainerInfo {
            id: "test-id-idle".to_string(),
            name: "test-container-idle".to_string(),
            container_port: 3000,
            status: ContainerStatus::Idle,
            last_active: Instant::now(),
            idle_since: Some(Instant::now()),
        };

        let persisted = PersistedContainerInfo::from_container_info(&original);
        let converted = persisted.to_container_info();

        assert_eq!(original.id, converted.id);
        assert_eq!(original.name, converted.name);
        assert_eq!(original.container_port, converted.container_port);
        assert_eq!(original.status, converted.status);
        assert!(converted.idle_since.is_some());
    }

    #[test]
    fn test_persistence_config_default() {
        let config = PersistenceConfig::default();
        assert!(config.enabled);
        assert_eq!(config.redis_url, "redis://localhost:6379");
        assert_eq!(config.key_prefix, "autoscaler");
        assert_eq!(config.batch_size, 50);
    }

    #[test]
    fn test_pool_state_serialization() {
        let pool_state = PersistedPoolState {
            function_name: "test-function".to_string(),
            containers: vec![PersistedContainerInfo {
                id: "container-1".to_string(),
                name: "test-container-1".to_string(),
                container_port: 8080,
                status: ContainerStatus::Healthy,
                last_active_unix: 1000,
                idle_since_unix: None,
            }],
            min_containers: 1,
            max_containers: 5,
            config: MonitoringConfig::default(),
            last_updated: 1703001234,
        };

        // Test serialization
        let serialized = serde_json::to_string(&pool_state).expect("Failed to serialize");
        assert!(serialized.contains("test-function"));
        assert!(serialized.contains("container-1"));

        // Test deserialization
        let deserialized: PersistedPoolState =
            serde_json::from_str(&serialized).expect("Failed to deserialize");
        assert_eq!(deserialized.function_name, "test-function");
        assert_eq!(deserialized.containers.len(), 1);
        assert_eq!(deserialized.containers[0].id, "container-1");
        assert_eq!(deserialized.last_updated, 1703001234);
    }

    #[test]
    fn test_metadata_creation() {
        let metadata = PersistenceMetadata::new(42);
        assert_eq!(metadata.version, "1.0");
        assert_eq!(metadata.total_pools, 42);
        assert!(metadata.last_cleanup > 0);
    }
}
