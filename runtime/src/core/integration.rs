use crate::core::autoscaler::{Autoscaler, AutoscalerConfig};
use crate::core::container_manager::MonitoringConfig;
use crate::shared::error::{AppResult, RuntimeError};
use bollard::Docker;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

/// Enhanced runtime that integrates autoscaling capabilities
pub struct AutoscalingRuntime {}

impl AutoscalingRuntime {
    /// Create a new autoscaling runtime with custom configuration
    pub fn new_with_config(
        docker: Docker,
        config: AutoscalerConfig,
        docker_compose_network_host: String,
    ) -> Autoscaler {
        Autoscaler::new(docker, config, docker_compose_network_host)
    }
}

/// Configuration builder for the autoscaling runtime
pub struct AutoscalingRuntimeBuilder {
    cpu_overload_threshold: f64,
    memory_overload_threshold: u64,
    cooldown_cpu_threshold: f64,
    cooldown_duration: Duration,
    poll_interval: Duration,
    min_containers_per_function: usize,
    max_containers_per_function: usize,
    scale_check_interval: Duration,
}

impl Default for AutoscalingRuntimeBuilder {
    fn default() -> Self {
        Self {
            cpu_overload_threshold: 0.70,
            memory_overload_threshold: 300_000_000,
            cooldown_cpu_threshold: 0.10,
            cooldown_duration: Duration::from_secs(30),
            poll_interval: Duration::from_secs(2),
            min_containers_per_function: 1,
            max_containers_per_function: 10,
            scale_check_interval: Duration::from_secs(10),
        }
    }
}

impl AutoscalingRuntimeBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cpu_overload_threshold(mut self, threshold: f64) -> Self {
        self.cpu_overload_threshold = threshold;
        self
    }

    pub fn memory_overload_threshold(mut self, threshold: u64) -> Self {
        self.memory_overload_threshold = threshold;
        self
    }

    pub fn cooldown_cpu_threshold(mut self, threshold: f64) -> Self {
        self.cooldown_cpu_threshold = threshold;
        self
    }

    pub fn cooldown_duration(mut self, duration: Duration) -> Self {
        self.cooldown_duration = duration;
        self
    }

    pub fn poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    pub fn min_containers_per_function(mut self, min: usize) -> Self {
        self.min_containers_per_function = min;
        self
    }

    pub fn max_containers_per_function(mut self, max: usize) -> Self {
        self.max_containers_per_function = max;
        self
    }

    pub fn scale_check_interval(mut self, interval: Duration) -> Self {
        self.scale_check_interval = interval;
        self
    }

    pub fn build(self, docker_compose_network_host: String) -> Autoscaler {
        let config = AutoscalerConfig {
            monitoring: MonitoringConfig {
                cpu_overload_threshold: self.cpu_overload_threshold,
                memory_overload_threshold: self.memory_overload_threshold,
                cooldown_cpu_threshold: self.cooldown_cpu_threshold,
                cooldown_duration: self.cooldown_duration,
                poll_interval: self.poll_interval,
            },
            min_containers_per_function: self.min_containers_per_function,
            max_containers_per_function: self.max_containers_per_function,
            scale_check_interval: self.scale_check_interval,
        };

        let docker = Docker::connect_with_http_defaults().unwrap();
        AutoscalingRuntime::new_with_config(docker, config, docker_compose_network_host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_pattern() {
        let runtime = AutoscalingRuntimeBuilder::new()
            .cpu_overload_threshold(0.8)
            .memory_overload_threshold(500_000_000)
            .min_containers_per_function(2)
            .max_containers_per_function(20)
            .build("test-network".to_string());

        // Test that the runtime was created successfully
        assert_eq!(runtime.get_config().monitoring.cpu_overload_threshold, 0.8);
    }
}
