# Invok Autoscaling System

The Invok serverless platform features an advanced container autoscaling system that uses **Prometheus metrics** for real-time monitoring and intelligent scaling decisions.

## Overview

The autoscaling system automatically manages container instances for serverless functions based on:
- **CPU Usage**: Scales up when containers exceed CPU thresholds
- **Memory Usage**: Monitors memory consumption for scaling decisions  
- **Request Load**: Tracks container activity and idle time
- **Cooldown Periods**: Prevents rapid scaling oscillations

## Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   cAdvisor      │───▶│   Prometheus    │───▶│  Invok Core     │
│ (Metrics Source)│    │ (Metrics Store) │    │ (Autoscaler)    │
└─────────────────┘    └─────────────────┘    └─────────────────┘
        │                       │                       │
        ▼                       ▼                       ▼
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│ Container Stats │    │ Time Series DB  │    │ Scaling Actions │
│ (CPU, Memory)   │    │ (1s intervals)  │    │ (Add/Remove)    │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

## Key Components

### 1. **MetricsClient** (`runtime/src/core/metrics_client.rs`)

Handles all communication with Prometheus:

```rust
pub struct MetricsClient {
    config: MetricsConfig,
    client: Client,
    cpu_cache: RwLock<HashMap<String, CachedMetric>>,
    memory_cache: RwLock<HashMap<String, CachedMetric>>,
}
```

**Features:**
- **Caching**: x-second TTL to reduce Prometheus load
- **Connection Pooling**: Reuses HTTP connections
- **Retry Logic**: 3 automatic retries with backoff
- **Query Optimization**: Uses 30-second rate windows for stable metrics

### 2. **ContainerPool** (`runtime/src/core/container_manager.rs`)

Manages containers for individual functions:

```rust
pub struct ContainerPool {
    function_name: String,
    containers: Arc<RwLock<Vec<ContainerInfo>>>,
    docker: Docker,
    network_host: String,
    config: MonitoringConfig,
    min_containers: usize,
    max_containers: usize,
    metrics_client: Arc<MetricsClient>,  // Required Prometheus client
}
```

### 3. **Autoscaler** (`runtime/src/core/autoscaler.rs`)

Orchestrates all scaling decisions:

```rust
pub struct Autoscaler {
    pools: Arc<RwLock<HashMap<String, Arc<ContainerPool>>>>,
    docker: Docker,
    config: AutoscalerConfig,
    docker_compose_network_host: String,
    metrics_client: Arc<MetricsClient>,  // Required
}
```

### 4. **AutoscalingRuntimeBuilder** (`runtime/src/core/builder.rs`)

Simplified builder pattern for configuration:

```rust
let runtime = AutoscalingRuntimeBuilder::new()
    .cpu_overload_threshold(80.0)
    .memory_overload_threshold(100.0)
    .min_containers_per_function(0)
    .max_containers_per_function(5)
    .cooldown_duration(Duration::from_secs(15))
    .scale_check_interval(Duration::from_secs(1))
    .prometheus_url("http://prometheus:9090".to_string())
    .build(network_host);
```

## Configuration

### Environment Variables

```bash
# Core scaling thresholds
CPU_OVERLOAD_THRESHOLD=80.0           # CPU % to trigger scale-up
MEMORY_OVERLOAD_THRESHOLD=100.0       # Memory % to trigger scale-up
COOLDOWN_CPU_THRESHOLD=0.0            # CPU % for scale-down consideration

# Container limits
MIN_CONTAINERS_PER_FUNCTION=0         # Minimum containers (can scale to zero)
MAX_CONTAINERS_PER_FUNCTION=5         # Maximum containers per function

# Timing configuration
POLL_INTERVAL_SECS=1                  # How often to check metrics
COOLDOWN_DURATION_SECS=15             # Wait time before scaling down

# Prometheus configuration
PROMETHEUS_URL=http://prometheus:9090  # Prometheus server URL
```

### Monitoring Configuration

```rust
pub struct MonitoringConfig {
    pub cpu_overload_threshold: f64,      // 80.0 = 80% CPU
    pub memory_overload_threshold: f64,   // 100.0 = 100%
    pub cooldown_cpu_threshold: f64,      // 0.0 = cooldown at 0%
    pub cooldown_duration: Duration,      // 15s wait before scale-down
    pub poll_interval: Duration,          // 1s metric collection interval
    pub prometheus_url: String,           // Prometheus endpoint
}
```

## Scaling Logic

### Scale-Up Triggers

A function scales up when **ALL** containers are overloaded:

```rust
pub fn needs_scale_up(&self) -> bool {
    let containers = self.containers.read().unwrap();
    
    if containers.len() >= self.max_containers {
        return false;  // At capacity
    }
    
    // Scale up if ALL containers are overloaded
    !containers.is_empty() && 
    containers.iter().all(|c| c.status == ContainerStatus::Overloaded)
}
```

**Overloaded Condition:**
```rust
if cpu_usage > cpu_threshold || memory_usage > memory_threshold {
    self.status = ContainerStatus::Overloaded;
}
```

### Scale-Down Triggers

Containers scale down when idle for the cooldown period:

```rust
pub fn get_scaledown_candidates(&self) -> Vec<String> {
    let containers = self.containers.read().unwrap();
    
    if containers.len() <= self.min_containers {
        return Vec::new();  // At minimum
    }
    
    containers.iter()
        .filter(|c| c.is_eligible_for_scaledown(self.config.cooldown_duration))
        .map(|c| c.id.clone())
        .collect()
}
```

**Idle Condition:**
```rust
if cpu_usage <= cooldown_threshold {
    if self.status != ContainerStatus::Idle {
        self.idle_since = Some(Instant::now());  // Start cooldown timer
        self.status = ContainerStatus::Idle;
    }
}
```

## Metrics Collection

### Prometheus Queries

**CPU Usage (30-second rate):**
```promql
rate(container_cpu_usage_seconds_total{id=~"/docker/CONTAINER_ID.*"}[30s]) * 100
```

**Memory Usage Percentage:**
```promql
(container_memory_usage_bytes{id=~"/docker/CONTAINER_ID.*"} / 
 container_spec_memory_limit_bytes{id=~"/docker/CONTAINER_ID.*"}) * 100
```

### Monitoring Flow

1. **cAdvisor** collects container stats (1-second intervals)
2. **Prometheus** scrapes cAdvisor metrics (1-second scrape interval)
3. **MetricsClient** queries Prometheus every 1 second
4. **ContainerPool** updates container status based on metrics
5. **Autoscaler** makes scaling decisions every x seconds

## Container States

```rust
pub enum ContainerStatus {
    Healthy,    // Normal operation, can receive requests
    Overloaded, // Above thresholds, trigger scale-up
    Idle,       // Below threshold, candidate for scale-down
}
```

### State Transitions

```
┌─────────┐   CPU/Memory > Threshold  ┌────────────┐
│ Healthy │──────────────────────────▶│ Overloaded │
│         │◀──────────────────────────│            │
└─────────┘   CPU/Memory < Threshold  └────────────┘
     │                                       
     │ CPU < Cooldown Threshold               
     ▼                                       
┌─────────┐   Recent Activity         ┌────────────┐
│  Idle   │──────────────────────────▶│  Healthy   │
│         │                           │            │
└─────────┘                           └────────────┘
```

## Load Balancing

The autoscaler selects the healthiest container for requests:

```rust
pub fn get_healthiest_container(&self) -> Option<ContainerDetails> {
    let containers = self.containers.read().unwrap();
    
    // 1. Filter healthy/idle containers
    let mut healthy_containers: Vec<&ContainerInfo> = containers
        .iter()
        .filter(|c| c.status == ContainerStatus::Healthy || 
                   c.status == ContainerStatus::Idle)
        .collect();
    
    // 2. Sort by CPU usage (ascending) and last active time
    healthy_containers.sort_by(|a, b| {
        a.cpu_usage.partial_cmp(&b.cpu_usage)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.last_active.cmp(&b.last_active))
    });
    
    // 3. Return least loaded container
    healthy_containers.first().map(|c| toContainerDetails(c))
}
```

## Monitoring & Observability

### Grafana Dashboards

Access: http://localhost:3001 (admin/admin)

**Key Metrics:**
- Container CPU usage over time
- Memory consumption patterns
- Container count per function
- Network I/O statistics

### Prometheus Metrics

Access: http://localhost:9090

**Useful Queries:**
```promql
# Current container count
count(container_last_seen{name=~".*invok.*"})

# Average CPU usage
avg(rate(container_cpu_usage_seconds_total{name=~".*invok.*"}[1m])) * 100

# Memory usage
sum(container_memory_usage_bytes{name=~".*invok.*"}) / 1024 / 1024
```

### Application Logs

```bash
# Watch autoscaling events
docker logs -f invok-core | grep -i "scaling\|prometheus\|container"

# Monitor specific function
docker logs -f invok-core | grep "function-name"
```

## Performance Characteristics

### Response Times
- **Metric Collection**: ~10ms per container
- **Scaling Decision**: ~50ms per function pool
- **Container Startup**: ~2-5 seconds
- **Scale-up Latency**: 2-5 seconds total
- **Scale-down Latency**: 15+ seconds (cooldown)

### Resource Usage
- **Memory**: ~50MB for metrics client + caching
- **CPU**: ~1-2% baseline, +0.1% per container monitored
- **Network**: ~1KB/s per container to Prometheus

### Scalability Limits
- **Functions**: Unlimited (bounded by memory)
- **Containers per Function**: Configurable (default: 5)
- **Total Containers**: Limited by Docker host resources
- **Metric Collection**: 1000+ containers supported

## Best Practices

### Production Configuration

```bash
# Production-optimized settings
CPU_OVERLOAD_THRESHOLD=70.0           # Lower threshold for faster response
MEMORY_OVERLOAD_THRESHOLD=80.0        # Prevent OOM conditions
MIN_CONTAINERS_PER_FUNCTION=1         # Keep warm containers
COOLDOWN_DURATION_SECS=60             # Longer cooldown for stability
POLL_INTERVAL_SECS=5                  # Reduce metric overhead
```
