# Autoscaler Persistence Implementation

This document describes the Redis persistence implementation for the autoscaler state, enabling zero-downtime server restarts while preserving running containers.

## Overview

The persistence system uses **individual pool storage** approach where each function's container pool is stored separately in Redis. This provides:

- **Scalability**: No monolithic snapshots that grow linearly with pool count
- **Efficiency**: Only changed pools need to be updated
- **Parallel Processing**: Pools can be loaded/saved concurrently
- **Memory Efficiency**: No large JSON payloads in memory

## Architecture

### Key Components

1. **AutoscalerPersistence**: Main persistence handler with Redis operations
2. **PersistedPoolState**: Serializable representation of container pool state
3. **PersistedContainerInfo**: Serializable container information
4. **PersistenceMetadata**: System metadata and statistics
5. **ContainerPool Integration**: Built-in persistence methods for seamless operation

### Redis Key Structure

```
autoscaler:pool:{function_key}  -> Individual pool state (JSON)
autoscaler:metadata            -> System metadata (JSON)
```

**Example:**
- `autoscaler:pool:hello-world-abc123` -> Pool state for hello-world function with user hash abc123
- `autoscaler:metadata` -> Persistence system metadata

### Data Structures

#### PersistedPoolState
```rust
struct PersistedPoolState {
    function_name: String,
    containers: Vec<PersistedContainerInfo>,
    min_containers: usize,
    max_containers: usize,
    config: MonitoringConfig,
    last_updated: i64,  // Unix timestamp
}
```

#### PersistedContainerInfo
```rust
struct PersistedContainerInfo {
    id: String,
    name: String,
    container_port: u32,
    status: ContainerStatus,
    last_active_unix: i64,     // Unix timestamp
    idle_since_unix: Option<i64>,
}
```

## Configuration

### PersistenceConfig
```rust
struct PersistenceConfig {
    enabled: bool,           // Enable/disable persistence
    redis_url: String,       // Redis connection URL
    key_prefix: String,      // Prefix for Redis keys
    batch_size: usize,       // Parallel loading batch size
}
```

### Builder Configuration
```rust
let runtime = AutoscalingRuntimeBuilder::new()
    .persistence_enabled(true)
    .redis_url("redis://localhost:6379")
    .persistence_key_prefix("autoscaler")
    .persistence_batch_size(50)  // Load 50 pools at a time
    .build()
    .await?;
```

## Operations

### State Recovery (Startup)

1. **Load Metadata**: Get system information and statistics
2. **Discover Pool Keys**: Scan for all `autoscaler:pool:*` keys
3. **Parallel Loading**: Load pools in configurable batches
4. **Container Validation**: Verify containers still exist in Docker
5. **Cleanup**: Remove invalid containers and empty pools
6. **Update Metadata**: Save current state statistics

```rust
// Parallel batch loading example
for chunk in function_keys.chunks(batch_size) {
    let load_tasks: Vec<_> = chunk.iter().map(|key| {
        persistence.load_pool_state(key)
    }).collect();
    
    let results = join_all(load_tasks).await;
    // Process results...
}
```

### State Persistence (Runtime)

**Individual Pool Updates**: Pools are saved to Redis whenever they change:
- New pool creation
- Container addition/removal
- Container status changes
- Container activation (last_active update)

**Immediate Updates**: No periodic snapshots, all changes saved instantly:
```rust
// After any pool change
let persisted_pool = pool.to_persisted_state();
persistence.save_pool_state(function_key, &persisted_pool).await?;
```

### Container Validation

During recovery, each container is validated against Docker:
```rust
pub async fn validate_and_sync_containers(&self) -> AppResult<()> {
    for entry in &self.containers {
        match self.docker.inspect_container(&entry.id, None).await {
            Ok(inspect) => {
                if !Self::is_container_running(&inspect) {
                    // Remove non-running container
                    self.containers.remove(&entry.id);
                }
            }
            Err(_) => {
                // Container doesn't exist, remove it
                self.containers.remove(&entry.id);
            }
        }
    }
}
```

## Error Handling

### Graceful Degradation
- **Redis Unavailable**: Autoscaler continues without persistence
- **Load Failures**: Failed pools logged, system continues with successful loads
- **Save Failures**: Logged as warnings, don't block operation

### Error Types
```rust
pub enum RuntimeError {
    RedisError(String),           // Redis connection/operation issues
    SerializationError(String),   // JSON serialization failures
    ContainerValidationError(String),
    // ... other errors
}
```

## Performance Characteristics

### Scalability Comparison

| Pool Count | Individual Storage | Monolithic Snapshot |
|------------|-------------------|-------------------|
| 100        | ~100KB total      | ~400KB per snapshot |
| 1,000      | ~1MB total        | ~4MB per snapshot |
| 10,000     | ~10MB total       | ~40MB per snapshot |
| 100,000    | ~100MB total      | ~400MB per snapshot |

### Recovery Performance

- **Parallel Loading**: Configurable batch size (default: 50 pools)
- **Memory Efficient**: No large JSON payloads
- **Linear Scaling**: O(n) with number of pools
- **Fast Startup**: Only load active pools

### Runtime Performance

- **Instant Updates**: No waiting for snapshot intervals
- **Minimal Overhead**: Only changed pools are saved
- **Network Efficient**: Small individual payloads
- **Redis Friendly**: Distributed across many keys

## Monitoring and Debugging

### Metrics Available

```rust
// Get all pool status for monitoring
let status = autoscaler.get_all_pool_status();

// Example output
{
  "hello-world-abc123": {
    "containers": 3,
    "healthy": 2,
    "idle": 1,
    "last_updated": 1703001234
  }
}
```

### Redis Inspection

```bash
# List all pools
redis-cli KEYS "autoscaler:pool:*"

# Get specific pool state
redis-cli GET "autoscaler:pool:hello-world-abc123"

# Get metadata
redis-cli GET "autoscaler:metadata"

# Count pools
redis-cli EVAL "return #redis.call('KEYS', 'autoscaler:pool:*')" 0
```

### Cleanup Operations

```rust
// Manual cleanup of stale pools
let active_keys: Vec<String> = autoscaler.get_all_pool_keys();
persistence.cleanup_stale_pools(&active_keys).await?;

// Delete specific pool
persistence.delete_pool_state("function-key").await?;
```

## Data Expiration

- **TTL**: All keys have 24-hour expiration (refreshed on update)
- **Auto-cleanup**: Stale pools removed during startup
- **Manual cleanup**: Available via API for maintenance

## Testing

The implementation includes comprehensive tests:

```bash
cd runtime
cargo test persistence
```

Test coverage includes:
- Container info serialization/deserialization
- Pool state persistence round-trip
- Parallel loading simulation
- Error handling scenarios
- Configuration validation

## Migration and Compatibility

### Schema Versioning
- Version field in metadata for future migrations
- Backward compatibility maintained where possible
- Graceful handling of unknown fields

### Zero-Downtime Deployment
1. Deploy new version (with backward compatibility)
2. Let it restore from existing Redis state
3. Verify operation
4. Old version state automatically cleaned up

## Best Practices

### Configuration
- **Batch Size**: Set based on available memory and Redis performance
- **Key Prefix**: Use environment-specific prefixes for isolation
- **Redis Connection**: Use connection pooling for high throughput

### Monitoring
- Monitor Redis memory usage with many pools
- Track failed load/save operations
- Alert on persistence disabled scenarios

### Maintenance
- Regular cleanup of expired keys
- Monitor Redis storage growth
- Backup critical pool states if needed

## Future Enhancements

### Potential Improvements
1. **Compression**: Gzip pool state for large deployments
2. **Partial Updates**: Only save changed container info
3. **Sharding**: Distribute pools across multiple Redis instances
4. **Backup/Restore**: Export/import pool states
5. **Metrics Integration**: Prometheus metrics for persistence operations

The individual pool storage approach provides a solid foundation for scaling to hundreds of thousands of function pools while maintaining efficient operation and fast recovery times. 