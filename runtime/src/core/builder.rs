use crate::core::autoscaler::{Autoscaler, AutoscalerConfig};
use crate::core::container_manager::MonitoringConfig;
use crate::core::metrics_client::MetricsClient;
use crate::core::persistence::PersistenceConfig;
use crate::shared::error::{AppResult, RuntimeError};
use bollard::Docker;
use std::sync::Arc;
use std::time::Duration;

/// The main autoscaling runtime
pub struct AutoscalingRuntime {
    pub autoscaler: Arc<Autoscaler>,
}

impl AutoscalingRuntime {
    /// Start the runtime
    pub async fn start(&self) -> AppResult<()> {
        self.autoscaler.start().await
    }

    /// Get the autoscaler reference
    pub fn autoscaler(&self) -> &Arc<Autoscaler> {
        &self.autoscaler
    }
}

/// Builder for configuring and creating the autoscaling runtime
#[derive(Default)]
pub struct AutoscalingRuntimeBuilder {
    docker_compose_network_host: Option<String>,
    scale_check_interval: Option<Duration>,
    min_containers_per_function: Option<usize>,
    max_containers_per_function: Option<usize>,
    persistence_enabled: Option<bool>,
    redis_url: Option<String>,
    persistence_key_prefix: Option<String>,
    persistence_batch_size: Option<usize>,
    cpu_overload_threshold: Option<f64>,
    memory_overload_threshold: Option<f64>,
    cooldown_cpu_threshold: Option<f64>,
    cooldown_duration: Option<Duration>,
}

impl AutoscalingRuntimeBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn cpu_overload_threshold(mut self, threshold: f64) -> Self {
        self.cpu_overload_threshold = Some(threshold);
        self
    }

    pub fn memory_overload_threshold(mut self, threshold: f64) -> Self {
        self.memory_overload_threshold = Some(threshold);
        self
    }

    pub fn cooldown_cpu_threshold(mut self, threshold: f64) -> Self {
        self.cooldown_cpu_threshold = Some(threshold);
        self
    }

    pub fn cooldown_duration(mut self, duration: Duration) -> Self {
        self.cooldown_duration = Some(duration);
        self
    }

    pub fn docker_compose_network_host(mut self, host: String) -> Self {
        self.docker_compose_network_host = Some(host);
        self
    }

    pub fn scale_check_interval(mut self, interval: Duration) -> Self {
        self.scale_check_interval = Some(interval);
        self
    }

    pub fn min_containers_per_function(mut self, min: usize) -> Self {
        self.min_containers_per_function = Some(min);
        self
    }

    pub fn max_containers_per_function(mut self, max: usize) -> Self {
        self.max_containers_per_function = Some(max);
        self
    }

    pub fn persistence_enabled(mut self, enabled: bool) -> Self {
        self.persistence_enabled = Some(enabled);
        self
    }

    pub fn redis_url(mut self, url: String) -> Self {
        self.redis_url = Some(url);
        self
    }

    pub fn persistence_key_prefix(mut self, prefix: String) -> Self {
        self.persistence_key_prefix = Some(prefix);
        self
    }

    pub fn persistence_batch_size(mut self, batch_size: usize) -> Self {
        self.persistence_batch_size = Some(batch_size);
        self
    }

    pub async fn build(self) -> AppResult<AutoscalingRuntime> {
        let docker_compose_network_host = self
            .docker_compose_network_host
            .unwrap_or_else(|| "host.docker.internal".to_string());

        let scale_check_interval = self.scale_check_interval.unwrap_or(Duration::from_secs(10));

        let min_containers = self.min_containers_per_function.unwrap_or(1);
        let max_containers = self.max_containers_per_function.unwrap_or(10);

        let cpu_overload_threshold = self.cpu_overload_threshold.unwrap_or(80.0);
        let memory_overload_threshold = self.memory_overload_threshold.unwrap_or(80.0);
        let cooldown_cpu_threshold = self.cooldown_cpu_threshold.unwrap_or(0.0);
        let cooldown_duration = self.cooldown_duration.unwrap_or(Duration::from_secs(60));

        // Configure persistence
        let persistence_enabled = self.persistence_enabled.unwrap_or(true);
        let redis_url = self
            .redis_url
            .unwrap_or_else(|| "redis://localhost:6379".to_string());
        let persistence_key_prefix = self
            .persistence_key_prefix
            .unwrap_or_else(|| "autoscaler".to_string());
        let persistence_batch_size = self.persistence_batch_size.unwrap_or(50);

        let persistence_config = PersistenceConfig {
            enabled: persistence_enabled,
            redis_url,
            key_prefix: persistence_key_prefix,
            batch_size: persistence_batch_size,
        };

        // Initialize Docker client
        let docker = Docker::connect_with_http_defaults()
            .map_err(|e| RuntimeError::System(format!("Failed to connect to Docker: {}", e)))?;

        // Initialize metrics client
        let metrics_config = crate::core::metrics_client::MetricsConfig {
            prometheus_url: "http://prometheus:9090".to_string(),
            query_timeout: Duration::from_secs(3),
            cache_ttl: Duration::from_secs(5),
            max_retries: 3,
        };
        let metrics_client = MetricsClient::new(metrics_config);

        // Initialize monitoring configuration
        let monitoring = MonitoringConfig {
            cpu_overload_threshold,
            memory_overload_threshold,
            cooldown_cpu_threshold,
            poll_interval: scale_check_interval,
            cooldown_duration,
        };
        // Create autoscaler config
        let autoscaler_config = AutoscalerConfig {
            monitoring,
            min_containers_per_function: min_containers,
            max_containers_per_function: max_containers,
            scale_check_interval,
        };

        // Create autoscaler with persistence
        let autoscaler = Autoscaler::new(
            docker.clone(),
            autoscaler_config,
            docker_compose_network_host.clone(),
            metrics_client,
        )
        .with_persistence(persistence_config)?;

        Ok(AutoscalingRuntime {
            autoscaler: Arc::new(autoscaler),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_builder_pattern() {
        let runtime = AutoscalingRuntimeBuilder::new()
            .docker_compose_network_host("test-network".to_string())
            .min_containers_per_function(2)
            .max_containers_per_function(20)
            .build()
            .await
            .unwrap();

        // Test that the runtime was created successfully
        assert_eq!(
            runtime.autoscaler.get_config().min_containers_per_function,
            2
        );
    }
}
