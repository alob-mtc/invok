use std::env;

const MAX_FUNCTION_SIZE_ENV_VARIABLE: &str = "MAX_FUNCTION_SIZE";
const DEFAULT_RUNTIME_ENV_VARIABLE: &str = "DEFAULT_RUNTIME";

// Autoscaling configuration environment variables
const CPU_OVERLOAD_THRESHOLD_ENV: &str = "CPU_OVERLOAD_THRESHOLD";
const MEMORY_OVERLOAD_THRESHOLD_ENV: &str = "MEMORY_OVERLOAD_THRESHOLD";
const COOLDOWN_CPU_THRESHOLD_ENV: &str = "COOLDOWN_CPU_THRESHOLD";
const COOLDOWN_DURATION_SECS_ENV: &str = "COOLDOWN_DURATION_SECS";
const MIN_CONTAINERS_PER_FUNCTION_ENV: &str = "MIN_CONTAINERS_PER_FUNCTION";
const MAX_CONTAINERS_PER_FUNCTION_ENV: &str = "MAX_CONTAINERS_PER_FUNCTION";
const POLL_INTERVAL_SECS_ENV: &str = "POLL_INTERVAL_SECS";

// Prometheus configuration environment variables
const USE_PROMETHEUS_METRICS_ENV: &str = "USE_PROMETHEUS_METRICS";
const PROMETHEUS_URL_ENV: &str = "PROMETHEUS_URL";
const FALLBACK_TO_DOCKER_ENV: &str = "FALLBACK_TO_DOCKER";

/// Default runtime if not specified
pub const DEFAULT_RUNTIME_VALUE: &str = "go";

/// Default maximum function size (10MB)
pub const DEFAULT_MAX_FUNCTION_SIZE_VALUE: usize = 10 * 1024 * 1024;

// Autoscaling defaults
pub const DEFAULT_CPU_OVERLOAD_THRESHOLD: f64 = 70.0;
pub const DEFAULT_MEMORY_OVERLOAD_THRESHOLD: f64 = 70.0; // 200 MB
pub const DEFAULT_COOLDOWN_CPU_THRESHOLD: f64 = 0.0;
pub const DEFAULT_COOLDOWN_DURATION_SECS: u64 = 30;
pub const DEFAULT_MIN_CONTAINERS_PER_FUNCTION: usize = 1;
pub const DEFAULT_MAX_CONTAINERS_PER_FUNCTION: usize = 10;
pub const DEFAULT_POLL_INTERVAL_SECS: u64 = 1;

// Prometheus defaults
pub const DEFAULT_USE_PROMETHEUS_METRICS: bool = false;
pub const DEFAULT_PROMETHEUS_URL: &str = "http://prometheus:9090";
pub const DEFAULT_FALLBACK_TO_DOCKER: bool = true;

/// Autoscaling configuration
#[derive(Debug, Clone)]
pub struct AutoscalingConfig {
    /// CPU usage threshold to trigger scale-up (0.0-1.0)
    pub cpu_overload_threshold: f64,
    /// Memory usage threshold to trigger scale-up (bytes)
    pub memory_overload_threshold: f64,
    /// CPU usage threshold for scale-down consideration (0.0-1.0)
    pub cooldown_cpu_threshold: f64,
    /// Duration to wait before scaling down idle containers (seconds)
    pub cooldown_duration_secs: u64,
    /// Minimum number of containers to maintain per function
    pub min_containers_per_function: usize,
    /// Maximum number of containers allowed per function
    pub max_containers_per_function: usize,
    /// Interval for polling container metrics (seconds)
    pub poll_interval_secs: u64,
    /// Whether to use Prometheus for metrics collection
    pub use_prometheus_metrics: bool,
    /// Prometheus server URL
    pub prometheus_url: String,
    /// Whether to fallback to Docker stats if Prometheus fails
    pub fallback_to_docker: bool,
}

impl Default for AutoscalingConfig {
    fn default() -> Self {
        Self {
            cpu_overload_threshold: DEFAULT_CPU_OVERLOAD_THRESHOLD,
            memory_overload_threshold: DEFAULT_MEMORY_OVERLOAD_THRESHOLD,
            cooldown_cpu_threshold: DEFAULT_COOLDOWN_CPU_THRESHOLD,
            cooldown_duration_secs: DEFAULT_COOLDOWN_DURATION_SECS,
            min_containers_per_function: DEFAULT_MIN_CONTAINERS_PER_FUNCTION,
            max_containers_per_function: DEFAULT_MAX_CONTAINERS_PER_FUNCTION,
            poll_interval_secs: DEFAULT_POLL_INTERVAL_SECS,
            use_prometheus_metrics: DEFAULT_USE_PROMETHEUS_METRICS,
            prometheus_url: DEFAULT_PROMETHEUS_URL.to_string(),
            fallback_to_docker: DEFAULT_FALLBACK_TO_DOCKER,
        }
    }
}

/// Function service configuration
#[derive(Debug, Clone)]
pub struct InvokFunctionConfig {
    /// Default runtime to use for functions
    pub default_runtime: String,

    /// Maximum function size in bytes
    pub max_function_size: usize,

    /// Autoscaling configuration
    pub autoscaling: AutoscalingConfig,
}

impl InvokFunctionConfig {
    /// Load function configuration from environment
    pub fn from_env() -> Self {
        let default_runtime = env::var(DEFAULT_RUNTIME_ENV_VARIABLE)
            .unwrap_or_else(|_| DEFAULT_RUNTIME_VALUE.to_string());

        let max_function_size = env::var(MAX_FUNCTION_SIZE_ENV_VARIABLE)
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(DEFAULT_MAX_FUNCTION_SIZE_VALUE);

        let autoscaling = AutoscalingConfig {
            cpu_overload_threshold: env::var(CPU_OVERLOAD_THRESHOLD_ENV)
                .ok()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(DEFAULT_CPU_OVERLOAD_THRESHOLD),
            memory_overload_threshold: env::var(MEMORY_OVERLOAD_THRESHOLD_ENV)
                .ok()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(DEFAULT_MEMORY_OVERLOAD_THRESHOLD),
            cooldown_cpu_threshold: env::var(COOLDOWN_CPU_THRESHOLD_ENV)
                .ok()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(DEFAULT_COOLDOWN_CPU_THRESHOLD),
            cooldown_duration_secs: env::var(COOLDOWN_DURATION_SECS_ENV)
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(DEFAULT_COOLDOWN_DURATION_SECS),
            min_containers_per_function: env::var(MIN_CONTAINERS_PER_FUNCTION_ENV)
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(DEFAULT_MIN_CONTAINERS_PER_FUNCTION),
            max_containers_per_function: env::var(MAX_CONTAINERS_PER_FUNCTION_ENV)
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(DEFAULT_MAX_CONTAINERS_PER_FUNCTION),
            poll_interval_secs: env::var(POLL_INTERVAL_SECS_ENV)
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(DEFAULT_POLL_INTERVAL_SECS),
            use_prometheus_metrics: env::var(USE_PROMETHEUS_METRICS_ENV)
                .ok()
                .and_then(|s| s.parse::<bool>().ok())
                .unwrap_or(DEFAULT_USE_PROMETHEUS_METRICS),
            prometheus_url: env::var(PROMETHEUS_URL_ENV)
                .unwrap_or_else(|_| DEFAULT_PROMETHEUS_URL.to_string()),
            fallback_to_docker: env::var(FALLBACK_TO_DOCKER_ENV)
                .ok()
                .and_then(|s| s.parse::<bool>().ok())
                .unwrap_or(DEFAULT_FALLBACK_TO_DOCKER),
        };

        Self {
            default_runtime,
            max_function_size,
            autoscaling,
        }
    }
}
