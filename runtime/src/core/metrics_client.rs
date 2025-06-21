use crate::shared::error::{AppResult, RuntimeError};
use dashmap::DashMap;
use reqwest::Client;
use serde::Deserialize;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, warn};

#[derive(Debug, Deserialize)]
struct PrometheusResponse {
    status: String,
    data: PrometheusData,
}

#[derive(Debug, Deserialize)]
struct PrometheusData {
    result: Vec<PrometheusResult>,
}

#[derive(Debug, Deserialize)]
struct PrometheusResult {
    value: (f64, String), // [timestamp, value]
}

/// Configuration for the metrics client
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    pub prometheus_url: String,
    pub query_timeout: Duration,
    pub cache_ttl: Duration,
    pub max_retries: u32,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            prometheus_url: "http://prometheus:9090".to_string(),
            query_timeout: Duration::from_secs(5),
            cache_ttl: Duration::from_secs(5),
            max_retries: 3,
        }
    }
}

/// Cache entry for metrics
#[derive(Debug, Clone)]
struct CachedMetric {
    value: f64,
    timestamp: Instant,
}

/// Client for fetching container metrics from Prometheus
pub struct MetricsClient {
    config: MetricsConfig,
    client: Client,
    cpu_cache: DashMap<String, CachedMetric>,
    memory_cache: DashMap<String, CachedMetric>,
}

impl MetricsClient {
    pub fn new(config: MetricsConfig) -> Self {
        let client = Client::builder()
            .timeout(config.query_timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config,
            client,
            cpu_cache: DashMap::new(),
            memory_cache: DashMap::new(),
        }
    }

    /// Get CPU usage percentage for a container
    pub async fn get_container_cpu_usage(&self, container_id: &str) -> AppResult<f64> {
        // Check cache first
        if let Some(cached) = self.get_cached_cpu(container_id) {
            debug!("Using cached CPU metric for container {}", container_id);
            return Ok(cached);
        }

        // Query Prometheus for CPU usage
        // Using rate over 30 seconds to get a more stable metric
        let query = format!(
            "rate(container_cpu_usage_seconds_total{{id=~\"/docker/{}.*\"}}[30s]) * 100",
            &container_id[0..12] // Use shortened container ID
        );

        let result = self.query_prometheus(&query).await?;

        // Cache the result
        self.cache_cpu_metric(container_id, result);

        debug!("Fetched CPU usage for {}: {:.2}%", container_id, result);
        Ok(result)
    }

    /// Get memory usage percentage for a container
    pub async fn get_container_memory_usage(&self, container_id: &str) -> AppResult<f64> {
        // Check cache first
        if let Some(cached) = self.get_cached_memory(container_id) {
            debug!("Using cached memory metric for container {}", container_id);
            return Ok(cached);
        }

        // Query Prometheus for memory usage percentage
        let query = format!(
            "(container_memory_usage_bytes{{id=~\"/docker/{}.*\"}} / container_spec_memory_limit_bytes{{id=~\"/docker/{}.*\"}}) * 100",
            &container_id[0..12], &container_id[0..12]
        );

        let result = self.query_prometheus(&query).await?;

        // Cache the result
        self.cache_memory_metric(container_id, result);

        debug!("Fetched memory usage for {}: {:.2}%", container_id, result);
        Ok(result)
    }

    /// Query Prometheus and return the first result value
    async fn query_prometheus(&self, query: &str) -> AppResult<f64> {
        let url = format!("{}/api/v1/query", self.config.prometheus_url);

        for attempt in 1..=self.config.max_retries {
            match self.execute_query(&url, query).await {
                Ok(value) => return Ok(value),
                Err(e) => {
                    if attempt == self.config.max_retries {
                        return Err(e);
                    }
                    warn!(
                        "Prometheus query attempt {} failed: {}, retrying...",
                        attempt, e
                    );
                    sleep(Duration::from_millis(100 * attempt as u64)).await;
                }
            }
        }

        Err(RuntimeError::System(
            "All Prometheus query attempts failed".to_string(),
        ))
    }

    /// Execute a single Prometheus query
    async fn execute_query(&self, url: &str, query: &str) -> AppResult<f64> {
        let response = self
            .client
            .get(url)
            .query(&[("query", query)])
            .send()
            .await
            .map_err(|e| RuntimeError::System(format!("Failed to query Prometheus: {}", e)))?;

        if !response.status().is_success() {
            return Err(RuntimeError::System(format!(
                "Prometheus query failed with status: {}",
                response.status()
            )));
        }

        let prom_response: PrometheusResponse = response.json().await.map_err(|e| {
            RuntimeError::System(format!("Failed to parse Prometheus response: {}", e))
        })?;

        if prom_response.status != "success" {
            return Err(RuntimeError::System(format!(
                "Prometheus query was not successful: {}",
                prom_response.status
            )));
        }

        // Extract the first result value
        if let Some(result) = prom_response.data.result.first() {
            let value_str = &result.value.1;
            let value = value_str.parse::<f64>().map_err(|e| {
                RuntimeError::System(format!("Failed to parse metric value: {}", e))
            })?;

            // Handle NaN values (common when containers just started)
            if value.is_nan() || value.is_infinite() {
                debug!("Received NaN/Infinite value from Prometheus, returning 0.0");
                return Ok(0.0);
            }

            Ok(value)
        } else {
            debug!("No metrics found for query: {}", query);
            Ok(0.0) // Return 0 if no metrics found (container might be starting)
        }
    }

    /// Get cached CPU metric if still valid
    fn get_cached_cpu(&self, container_id: &str) -> Option<f64> {
        let cached = self.cpu_cache.get(container_id)?;

        if cached.timestamp.elapsed() < self.config.cache_ttl {
            Some(cached.value)
        } else {
            None
        }
    }

    /// Get cached memory metric if still valid
    fn get_cached_memory(&self, container_id: &str) -> Option<f64> {
        let cached = self.memory_cache.get(container_id)?;

        if cached.timestamp.elapsed() < self.config.cache_ttl {
            Some(cached.value)
        } else {
            None
        }
    }

    /// Cache CPU metric
    fn cache_cpu_metric(&self, container_id: &str, value: f64) {
        self.cpu_cache.insert(
            container_id.to_string(),
            CachedMetric {
                value,
                timestamp: Instant::now(),
            },
        );
    }

    /// Cache memory metric
    fn cache_memory_metric(&self, container_id: &str, value: f64) {
        self.memory_cache.insert(
            container_id.to_string(),
            CachedMetric {
                value,
                timestamp: Instant::now(),
            },
        );
    }

    /// Health check for the metrics client
    pub async fn health_check(&self) -> bool {
        let url = format!("{}/api/v1/query", self.config.prometheus_url);
        match self.client.get(&url).query(&[("query", "up")]).send().await {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_config_default() {
        let config = MetricsConfig::default();
        assert_eq!(config.prometheus_url, "http://prometheus:9090");
        assert_eq!(config.query_timeout, Duration::from_secs(5));
        assert_eq!(config.cache_ttl, Duration::from_secs(5));
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_cache_operations() {
        let config = MetricsConfig::default();
        let client = MetricsClient::new(config);

        // Test caching
        client.cache_cpu_metric("test-container", 50.0);
        assert_eq!(client.get_cached_cpu("test-container"), Some(50.0));

        client.cache_memory_metric("test-container", 75.0);
        assert_eq!(client.get_cached_memory("test-container"), Some(75.0));
    }
}
