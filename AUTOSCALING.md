# Invok Autoscaling Feature

This document describes the new resource-aware autoscaling feature for Invok, which automatically manages container pools based on resource usage.

## Overview

The autoscaling feature extends Invok to:

1. **Continuously monitor resource usage** on each running function container
2. **Automatically spawn new containers** when existing ones become overloaded
3. **Redistribute function invocations** across healthy containers
4. **Gracefully decommission underutilized containers** to free resources

## Architecture

### Core Components

- **ContainerManager**: Monitors individual container resource usage (CPU, memory)
- **ContainerPool**: Manages a pool of containers for a specific function
- **Autoscaler**: Coordinates multiple container pools and makes scaling decisions
- **AutoscalingRuntime**: High-level interface that integrates with the existing Invok runtime

### Resource Monitoring

Each container is continuously monitored for:
- **CPU usage percentage** (0.0-1.0)
- **Memory usage** in bytes (RSS)
- **Container status**: Healthy, Overloaded, or Idle

## Configuration

### Environment Variables

You can configure autoscaling behavior using these environment variables:

```bash
# Monitoring thresholds
CPU_OVERLOAD_THRESHOLD=0.70          # 70% CPU triggers overload
MEMORY_OVERLOAD_THRESHOLD=300000000  # 300 MB memory triggers overload
COOLDOWN_CPU_THRESHOLD=0.10          # 10% CPU for cooldown
COOLDOWN_DURATION_SECS=30            # 30 seconds cooldown period

# Container limits
MIN_CONTAINERS_PER_FUNCTION=1        # Minimum containers to maintain
MAX_CONTAINERS_PER_FUNCTION=10       # Maximum containers allowed
POLL_INTERVAL_SECS=2                 # Resource monitoring interval
```

### Programmatic Configuration

```rust
use runtime::core::integration::{AutoscalingRuntime, AutoscalingRuntimeBuilder};
use std::time::Duration;

// Using builder pattern for custom configuration
let runtime = AutoscalingRuntimeBuilder::new()
    .cpu_overload_threshold(0.80)           // 80% CPU threshold
    .memory_overload_threshold(500_000_000) // 500 MB memory threshold
    .min_containers_per_function(2)         // Always keep 2 containers warm
    .max_containers_per_function(20)        // Scale up to 20 containers max
    .cooldown_duration(Duration::from_secs(60)) // 60 second cooldown
    .poll_interval(Duration::from_secs(1))   // Monitor every second
    .build(docker, "invok-network".to_string());
```

## Usage Examples

### Basic Usage

```rust
use runtime::core::integration::AutoscalingRuntime;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create autoscaling runtime with defaults
    let runtime = AutoscalingRuntime::new("invok-network".to_string()).await?;
    
    // Start the autoscaler background tasks
    runtime.start().await?;
    
    // Register a container for a function
    runtime.register_container(
        "my-function",
        "container-123".to_string(),
        "my-function-container-1".to_string(),
    ).await?;
    
    // Execute function (will use best available container)
    if let Some(container_id) = runtime.execute_function("my-function").await? {
        println!("Function executed on container: {}", container_id);
    }
    
    // Get status of all pools
    let status = runtime.get_status();
    println!("Runtime status: {}", serde_json::to_string_pretty(&status)?);
    
    Ok(())
}
```

### Integration with Existing Invok Code

```rust
use runtime::core::integration::AutoscalingRuntime;
use runtime::core::runner::{runner, ContainerDetails};

async fn enhanced_function_execution(
    runtime: &AutoscalingRuntime,
    function_name: &str,
    image_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Try to get an existing healthy container
    if let Some(container_id) = runtime.execute_function(function_name).await? {
        println!("Using existing container: {}", container_id);
        return Ok(());
    }
    
    // If no containers available, create a new one
    let container_details = ContainerDetails {
        container_port: 8080,
        bind_port: "8080".to_string(),
        container_name: format!("{}-{}", function_name, uuid::Uuid::new_v4()),
        timeout: 300,
        docker_compose_network_host: "invok-network".to_string(),
    };
    
    // Start the container using existing runner
    runner(image_name, container_details.clone()).await?;
    
    // Register it with the autoscaler
    runtime.register_container(
        function_name,
        container_details.container_name.clone(),
        container_details.container_name,
    ).await?;
    
    Ok(())
}
```

## Scaling Behavior

### Scale-Up Triggers

A new container is spawned when:
- **All existing containers are overloaded** (CPU > threshold OR memory > threshold)
- **Container count < max_containers_per_function**

### Scale-Down Triggers

A container is removed when:
- **Container is idle** (CPU < cooldown_threshold) for cooldown_duration
- **Container count > min_containers_per_function**

### Load Balancing

New requests are routed to:
1. **Healthiest container** (lowest CPU usage among healthy containers)
2. **Overloaded container** (as last resort if no healthy containers available)
3. **New container** (spawned immediately if at capacity)

## Monitoring and Debugging

### Status Endpoint

```rust
let status = runtime.get_status();
println!("{}", serde_json::to_string_pretty(&status)?);
```

Example output:
```json
{
  "pools": {
    "my-function": {
      "function_name": "my-function",
      "container_count": 3,
      "healthy_containers": 2,
      "overloaded_containers": 1,
      "idle_containers": 0
    }
  },
  "runtime": "autoscaling"
}
```

### Logs

The autoscaler produces structured logs for monitoring:

```
INFO - Starting autoscaler with config: AutoscalerConfig { ... }
INFO - Created new container pool for function: my-function
INFO - Container my-function-container-1 status changed from Healthy to Overloaded
INFO - Scaling up function: my-function
INFO - Successfully scaled up function my-function with container my-function-container-2
INFO - Scaled down container my-function-container-3 for function my-function
```

## Performance Considerations

### Resource Overhead

- **Memory**: ~1-2 MB per monitored container
- **CPU**: ~0.1% per monitored container (with 2-second polling)
- **Network**: Minimal (local Docker API calls only)

### Scaling Latency

- **Scale-up**: 2-10 seconds (depending on image size and container startup time)
- **Scale-down**: 30+ seconds (configurable cooldown period)
- **Load balancing**: <1ms (in-memory container selection)

## Best Practices

### Configuration Tuning

1. **Set appropriate thresholds**: 
   - CPU: 60-80% for most workloads
   - Memory: 70-90% of container limit

2. **Adjust polling frequency**:
   - High-frequency workloads: 1-2 seconds
   - Batch workloads: 5-10 seconds

3. **Configure container limits**:
   - Min containers: 1-2 for warm starts
   - Max containers: Based on available resources

### Monitoring

1. **Track key metrics**:
   - Container count per function
   - Scale-up/down frequency
   - Resource utilization trends

2. **Set up alerts**:
   - Frequent scaling events
   - Max container limit reached
   - All containers overloaded

### Resource Management

1. **Set Docker resource limits**:
   ```rust
   // In container configuration
   memory: Some(512 * 1024 * 1024), // 512 MB limit
   cpu_quota: Some(50000),           // 0.5 CPU limit
   ```

2. **Monitor host resources**:
   - Ensure sufficient CPU/memory for scaling
   - Monitor Docker daemon performance

## Troubleshooting

### Common Issues

1. **Containers not scaling up**:
   - Check if max_containers_per_function reached
   - Verify Docker daemon connectivity
   - Check resource thresholds

2. **Containers not scaling down**:
   - Verify cooldown_duration setting
   - Check if min_containers_per_function reached
   - Monitor container idle detection

3. **High resource usage**:
   - Increase polling interval
   - Reduce number of monitored containers
   - Optimize container resource limits

### Debug Commands

```rust
// Get detailed autoscaler status
let autoscaler = runtime.autoscaler();
let pools = autoscaler.get_all_pool_status();

// Check specific function pool
let pool = autoscaler.get_or_create_pool("my-function").await;
let container_count = pool.container_count();
let candidates = pool.get_scaledown_candidates();
```

## Migration Guide

### From Static Timeout to Autoscaling

1. **Update configuration**:
   ```rust
   // Old: Fixed timeout
   let timeout = Duration::from_secs(12);
   
   // New: Autoscaling configuration
   let runtime = AutoscalingRuntimeBuilder::new()
       .cpu_overload_threshold(0.70)
       .build(docker, network_host);
   ```

2. **Update function execution**:
   ```rust
   // Old: Direct runner call
   runner(image_name, container_details).await?;
   
   // New: Autoscaling-aware execution
   runtime.execute_function(function_name).await?;
   ```

3. **Update monitoring**:
   ```rust
   // Old: Manual container tracking
   // New: Built-in pool status
   let status = runtime.get_status();
   ```

## Future Enhancements

- **Predictive scaling** based on request patterns
- **Custom metrics** support (beyond CPU/memory)
- **Multi-region** container distribution
- **Cost optimization** algorithms
- **Integration** with Kubernetes HPA