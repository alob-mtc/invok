# Invok Autoscaling Feature

This document describes the resource-aware autoscaling feature for Invok, which automatically manages container pools based on resource usage.

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
- **AutoscalingRuntimeBuilder**: High-level interface for configuring and building runtime

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
use runtime::core::integration::AutoscalingRuntimeBuilder;
use std::time::Duration;

// Using builder pattern for custom configuration
let runtime = AutoscalingRuntimeBuilder::new()
    .cpu_overload_threshold(0.80)           // 80% CPU threshold
    .memory_overload_threshold(500_000_000) // 500 MB memory threshold
    .min_containers_per_function(2)         // Always keep 2 containers warm
    .max_containers_per_function(20)        // Scale up to 20 containers max
    .cooldown_duration(Duration::from_secs(60)) // 60 second cooldown
    .poll_interval(Duration::from_secs(1))   // Monitor every second
    .build("invok-network".to_string());
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
