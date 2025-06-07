use crate::core::autoscaler::{Autoscaler, AutoscalerConfig};
use crate::core::container_manager::MonitoringConfig;
use crate::core::metrics_client::{MetricsClient, MetricsConfig};
use crate::shared::error::{AppResult, RuntimeError};
use bollard::Docker;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

/// Configuration builder for the autoscaling runtime
pub struct AutoscalingRuntimeBuilder {
    cpu_overload_threshold: f64,
    memory_overload_threshold: f64,
    cooldown_cpu_threshold: f64,
    cooldown_duration: Duration,
    poll_interval: Duration,
    min_containers_per_function: usize,
    max_containers_per_function: usize,
    scale_check_interval: Duration,
    prometheus_url: String,
    query_timeout: u64,
    cache_ttl: u64,
    max_retries: u32,
}

impl Default for AutoscalingRuntimeBuilder {
    fn default() -> Self {
        Self {
            cpu_overload_threshold: 70.0,
            memory_overload_threshold: 70.0,
            cooldown_cpu_threshold: 0.0,
            cooldown_duration: Duration::from_secs(30),
            poll_interval: Duration::from_secs(2),
            min_containers_per_function: 1,
            max_containers_per_function: 10,
            scale_check_interval: Duration::from_secs(1),
            prometheus_url: "http://prometheus:9090".to_string(),
            query_timeout: 3,
            cache_ttl: 5,
            max_retries: 3,
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

    pub fn memory_overload_threshold(mut self, threshold: f64) -> Self {
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

    pub fn scale_check_interval(mut self, interval: Duration) -> Self {
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

    pub fn prometheus_url(mut self, url: String) -> Self {
        self.prometheus_url = url;
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
                prometheus_url: self.prometheus_url.clone(),
            },
            min_containers_per_function: self.min_containers_per_function,
            max_containers_per_function: self.max_containers_per_function,
            scale_check_interval: self.scale_check_interval,
        };

        let docker = Docker::connect_with_http_defaults().unwrap();

        let metrics_config = MetricsConfig {
            prometheus_url: self.prometheus_url,
            query_timeout: Duration::from_secs(self.query_timeout),
            cache_ttl: Duration::from_secs(self.cache_ttl),
            max_retries: self.max_retries,
        };
        let metrics_client = MetricsClient::new(metrics_config);
        let autoscaler =
            Autoscaler::new(docker, config, docker_compose_network_host, metrics_client);
        autoscaler
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_pattern() {
        let runtime = AutoscalingRuntimeBuilder::new()
            .cpu_overload_threshold(80.0)
            .memory_overload_threshold(75.0)
            .min_containers_per_function(2)
            .max_containers_per_function(20)
            .build("test-network".to_string());

        // Test that the runtime was created successfully
        assert_eq!(runtime.get_config().monitoring.cpu_overload_threshold, 80.0);
    }
}
