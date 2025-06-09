# Autoscaler State Persistence

This document explains the Redis-based persistence system for the Invok autoscaler that enables zero-downtime server restarts and state recovery.

## Overview

The autoscaler persistence system stores container pool state in Redis, allowing the server to:

- **Restart without losing container state**: Containers keep running while server restarts
- **Maintain scaling decisions**: Previous autoscaling state is preserved
- **Fast recovery**: Immediate restoration of pool configurations and container metadata
- **Zero-downtime deployments**: No container recreation after server restarts

## Architecture

The system stores each container pool independently, allowing for efficient scaling and recovery:

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Autoscaler    │───▶│   Redis Store   │───▶│ State Recovery  │
│  (Live State)   │    │   (Persisted)   │    │  (On Restart)   │
└─────────────────┘    └─────────────────┘    └─────────────────┘
        │                       │                       │
        ▼                       ▼                       ▼
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│ Container Pools │    │ Serialized JSON │    │ Restored Pools  │
│ Function State  │    │ Pool Snapshots  │    │ Validated State │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

## Data Structures

### PersistedContainerInfo

Serializable version of container metadata:

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PersistedContainerInfo {
    pub id: String,                    // Docker container ID
    pub name: String,                  // Container name  
    pub container_port: u32,           // Internal port
    pub status: ContainerStatus,       // Health status
    pub last_active_unix: i64,         // Last activity timestamp
    pub idle_since_unix: Option<i64>,  // Idle start timestamp
}
```

### PersistedPoolState

Complete function pool state:

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PersistedPoolState {
    pub function_name: String,         // Function identifier
    pub containers: Vec<PersistedContainerInfo>,
    pub min_containers: usize,         // Scaling constraints
    pub max_containers: usize,
    pub config: MonitoringConfig,      // Thresholds and settings
    pub last_updated: i64,            // When pool was last updated
}
```

### PersistenceMetadata

System metadata for tracking and cleanup:

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PersistenceMetadata {
    pub version: String,              // Schema version
    pub last_cleanup: i64,            // Last cleanup timestamp
    pub total_pools: usize,           // Number of active pools
}
```

## Redis Key Structure

Individual pool storage with metadata:

- `autoscaler:pool:{function_key}` - Individual pool state (JSON)
- `autoscaler:metadata` - System metadata and statistics

**Example:**
```
autoscaler:pool:hello-world-abc123  -> Pool state for hello-world function
autoscaler:metadata                 -> System metadata
```

## Configuration

### PersistenceConfig

```rust
struct PersistenceConfig {
    enabled: bool,           // Enable/disable persistence
    redis_url: String,       // Redis connection URL
    key_prefix: String,      // Prefix for Redis keys
    batch_size: usize,       // Parallel loading batch size (default: 50)
}
```

### Builder Configuration

```rust
let runtime = AutoscalingRuntimeBuilder::new()
    .persistence_enabled(true)
    .redis_url("redis://localhost:6379")
    .persistence_key_prefix("autoscaler")
    .persistence_batch_size(50)  // Load 50 pools at a time during recovery
    .build()
    .await?;
```

## Operations

### State Recovery (Startup)

1. **Load Metadata**: Get system information and statistics
2. **Discover Pool Keys**: Scan for all `autoscaler:pool:*` keys
3. **Parallel Loading**: Load pools in configurable batches (default: 50 at a time)
4. **Container Validation**: Verify containers still exist in Docker
5. **Cleanup**: Remove invalid containers and empty pools
6. **Update Metadata**: Save current state statistics

### State Persistence (Runtime)

**Immediate Updates**: Pool states are saved to Redis instantly when changes occur:
- New pool creation
- Container addition/removal  
- Container status changes
- Configuration updates

**No Periodic Snapshots**: Changes are persisted immediately, not on intervals.

### Container Validation

During recovery, each container is validated against Docker reality:

```rust
pub async fn validate_and_sync_containers(&self) -> AppResult<()> {
    for entry in &self.containers {
        match self.docker.inspect_container(&entry.id, None).await {
            Ok(inspect) => {
                if !Self::is_container_running(&inspect) {
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

## Recovery Process

### Server Restart Flow

1. **Load Pool Keys**: Discover all persisted pools
2. **Batch Loading**: Load pool states in parallel batches
3. **Container Validation**: Check each container against Docker
4. **State Reconciliation**: Remove invalid containers, keep valid ones
5. **Cleanup**: Delete empty pools and stale Redis keys

## Error Handling

### Graceful Degradation

- **Redis Unavailable**: Autoscaler continues without persistence, logs warnings
- **Load Failures**: Failed pools are logged, system continues with successful loads
- **Save Failures**: Logged as warnings, don't block normal operation
- **Container Validation**: Invalid containers removed, pool continues with valid ones

### Error Recovery

The system is designed to handle failures gracefully:

```rust
// Persistence operations are always wrapped in error handling
if let Err(e) = self.save_pool_state(function_key, &pool).await {
    warn!("Failed to save new pool state for {}: {}", function_key, e);
    // Continue operation without persistence
}
```

## Performance Characteristics

### Scalability Benefits

| Aspect | Individual Storage | Monolithic Snapshots |
|--------|-------------------|---------------------|
| Memory usage | ~1KB per pool | ~400KB+ for all pools |
| Update cost | Single pool only | Entire system state |
| Recovery time | Parallel loading | Serial loading |
| Network overhead | Minimal per change | Large payloads |

### Recovery Performance

- **Parallel Loading**: Configurable batch size (default: 20 pools)
- **Memory Efficient**: No large JSON payloads in memory
- **Linear Scaling**: Recovery time scales linearly with pool count
- **Fast Startup**: Typical recovery in <2 seconds

## Data Expiration

- **TTL**: All pool keys have 24-hour expiration (refreshed on updates)
- **Auto-cleanup**: Stale pools removed during startup
- **Metadata refresh**: System metadata updated after recovery

## Monitoring

### Redis Inspection

```bash
# List all pools
redis-cli KEYS "autoscaler:pool:*"

# Get specific pool state  
redis-cli GET "autoscaler:pool:hello-world-abc123"

# Get metadata
redis-cli GET "autoscaler:metadata"

# Count active pools
redis-cli EVAL "return #redis.call('KEYS', 'autoscaler:pool:*')" 0
```

The individual pool storage approach provides efficient scaling to thousands of function pools while maintaining fast recovery times and minimal memory overhead. 