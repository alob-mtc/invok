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

/// Default runtime if not specified
pub const DEFAULT_RUNTIME_VALUE: &str = "go";

/// Default maximum function size (10MB)
pub const DEFAULT_MAX_FUNCTION_SIZE_VALUE: usize = 10 * 1024 * 1024;

// Autoscaling defaults
pub const DEFAULT_CPU_OVERLOAD_THRESHOLD: f64 = 0.70;
pub const DEFAULT_MEMORY_OVERLOAD_THRESHOLD: u64 = 200_000_000; // 200 MB
pub const DEFAULT_COOLDOWN_CPU_THRESHOLD: f64 = 0.10;
pub const DEFAULT_COOLDOWN_DURATION_SECS: u64 = 30;
pub const DEFAULT_MIN_CONTAINERS_PER_FUNCTION: usize = 1;
pub const DEFAULT_MAX_CONTAINERS_PER_FUNCTION: usize = 10;
pub const DEFAULT_POLL_INTERVAL_SECS: u64 = 2;

/// Autoscaling configuration
#[derive(Debug, Clone)]
pub struct AutoscalingConfig {
    /// CPU usage threshold to trigger scale-up (0.0-1.0)
    pub cpu_overload_threshold: f64,
    /// Memory usage threshold to trigger scale-up (bytes)
    pub memory_overload_threshold: u64,
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
                .and_then(|s| s.parse::<u64>().ok())
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
        };

        Self {
            default_runtime,
            max_function_size,
            autoscaling,
        }
    }
}
