//! Health check system for monitoring component status
//!
//! This module provides comprehensive health checking for all system components
//! including storage, network, persistence, and external dependencies.

use crate::metrics::MetricsCollector;
use crate::storage::MemoryStore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::debug;

/// Health checker that monitors all system components
#[derive(Debug)]
pub struct HealthChecker {
    components: Arc<RwLock<HashMap<String, ComponentHealth>>>,
    storage: Arc<MemoryStore>,
    metrics: Arc<MetricsCollector>,
    last_check: Arc<RwLock<Instant>>,
    check_interval: Duration,
}

/// Overall health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub timestamp: String,
    pub uptime_seconds: u64,
    pub components: HashMap<String, ComponentHealth>,
    pub summary: HealthSummary,
}

/// Individual component health
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentHealth {
    pub status: String,
    pub last_check: String,
    pub response_time_ms: u64,
    pub details: HashMap<String, String>,
    pub error: Option<String>,
}

/// Health summary statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthSummary {
    pub healthy_components: usize,
    pub unhealthy_components: usize,
    pub total_components: usize,
    pub overall_status: String,
}

/// Result of a health check operation
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    pub is_healthy: bool,
    pub status: HealthStatus,
}

impl HealthChecker {
    /// Create a new health checker
    pub fn new(storage: Arc<MemoryStore>, metrics: Arc<MetricsCollector>) -> Self {
        Self {
            components: Arc::new(RwLock::new(HashMap::new())),
            storage,
            metrics,
            last_check: Arc::new(RwLock::new(Instant::now())),
            check_interval: Duration::from_secs(30),
        }
    }

    /// Check all components and return overall health status
    pub async fn check_all_components(&self) -> HealthCheckResult {
        let start_time = Instant::now();
        let mut components = HashMap::new();

        // Check storage component
        let storage_health = self.check_storage_health().await;
        components.insert("storage".to_string(), storage_health);

        // Check metrics component
        let metrics_health = self.check_metrics_health().await;
        components.insert("metrics".to_string(), metrics_health);

        // Check memory usage
        let memory_health = self.check_memory_health().await;
        components.insert("memory".to_string(), memory_health);

        // Check disk space (if AOF is enabled)
        let disk_health = self.check_disk_health().await;
        components.insert("disk".to_string(), disk_health);

        // Check network connectivity
        let network_health = self.check_network_health().await;
        components.insert("network".to_string(), network_health);

        // Update component cache
        {
            let mut component_cache = self.components.write().await;
            *component_cache = components.clone();
        }

        // Update last check time
        {
            let mut last_check = self.last_check.write().await;
            *last_check = Instant::now();
        }

        // Calculate summary
        let healthy_count = components
            .values()
            .filter(|c| c.status == "healthy")
            .count();
        let total_count = components.len();
        let unhealthy_count = total_count - healthy_count;

        let overall_status = if unhealthy_count == 0 {
            "healthy".to_string()
        } else if healthy_count > unhealthy_count {
            "degraded".to_string()
        } else {
            "unhealthy".to_string()
        };

        let summary = HealthSummary {
            healthy_components: healthy_count,
            unhealthy_components: unhealthy_count,
            total_components: total_count,
            overall_status: overall_status.clone(),
        };

        let uptime = self.metrics.uptime();
        let status = HealthStatus {
            status: overall_status.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            uptime_seconds: uptime.as_secs(),
            components,
            summary,
        };

        let is_healthy = overall_status == "healthy";

        debug!(
            "Health check completed in {:?}, status: {}",
            start_time.elapsed(),
            overall_status
        );

        HealthCheckResult { is_healthy, status }
    }

    /// Check if the service is ready to accept traffic
    pub async fn is_ready(&self) -> bool {
        // Service is ready if storage and metrics are healthy
        let storage_health = self.check_storage_health().await;
        let metrics_health = self.check_metrics_health().await;

        storage_health.status == "healthy" && metrics_health.status == "healthy"
    }

    /// Check if the service is alive (basic liveness check)
    pub async fn is_alive(&self) -> bool {
        // Basic liveness check - just verify we can respond
        // In a more complex system, this might check for deadlocks or critical failures
        true
    }

    /// Get cached health status (faster than full check)
    pub async fn get_cached_status(&self) -> Option<HealthStatus> {
        let last_check = *self.last_check.read().await;

        // Return cached status if it's recent enough
        if last_check.elapsed() < self.check_interval {
            let components = self.components.read().await.clone();
            if !components.is_empty() {
                let healthy_count = components
                    .values()
                    .filter(|c| c.status == "healthy")
                    .count();
                let total_count = components.len();
                let unhealthy_count = total_count - healthy_count;

                let overall_status = if unhealthy_count == 0 {
                    "healthy".to_string()
                } else if healthy_count > unhealthy_count {
                    "degraded".to_string()
                } else {
                    "unhealthy".to_string()
                };

                let summary = HealthSummary {
                    healthy_components: healthy_count,
                    unhealthy_components: unhealthy_count,
                    total_components: total_count,
                    overall_status: overall_status.clone(),
                };

                let uptime = self.metrics.uptime();
                return Some(HealthStatus {
                    status: overall_status,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    uptime_seconds: uptime.as_secs(),
                    components,
                    summary,
                });
            }
        }

        None
    }

    /// Check storage component health
    async fn check_storage_health(&self) -> ComponentHealth {
        let start_time = Instant::now();
        let mut details = HashMap::new();

        // Test basic storage operations
        let test_key = "__health_check_test__";
        let test_value = "health_check_value";

        match self.storage.set(test_key, test_value).await {
            Ok(()) => {
                match self.storage.get(test_key) {
                    Ok(Some(value)) => {
                        if value.value.to_string() == test_value {
                            // Clean up test key
                            let _ = self.storage.delete(test_key).await;

                            details.insert("test_operation".to_string(), "success".to_string());
                            details.insert("storage_type".to_string(), "memory".to_string());

                            ComponentHealth {
                                status: "healthy".to_string(),
                                last_check: chrono::Utc::now().to_rfc3339(),
                                response_time_ms: start_time.elapsed().as_millis() as u64,
                                details,
                                error: None,
                            }
                        } else {
                            details
                                .insert("test_operation".to_string(), "value_mismatch".to_string());
                            ComponentHealth {
                                status: "unhealthy".to_string(),
                                last_check: chrono::Utc::now().to_rfc3339(),
                                response_time_ms: start_time.elapsed().as_millis() as u64,
                                details,
                                error: Some("Storage test value mismatch".to_string()),
                            }
                        }
                    }
                    Ok(None) => {
                        details.insert("test_operation".to_string(), "key_not_found".to_string());
                        ComponentHealth {
                            status: "unhealthy".to_string(),
                            last_check: chrono::Utc::now().to_rfc3339(),
                            response_time_ms: start_time.elapsed().as_millis() as u64,
                            details,
                            error: Some("Storage test key not found after set".to_string()),
                        }
                    }
                    Err(e) => {
                        details.insert("test_operation".to_string(), "get_failed".to_string());
                        ComponentHealth {
                            status: "unhealthy".to_string(),
                            last_check: chrono::Utc::now().to_rfc3339(),
                            response_time_ms: start_time.elapsed().as_millis() as u64,
                            details,
                            error: Some(format!("Storage get failed: {e}")),
                        }
                    }
                }
            }
            Err(e) => {
                details.insert("test_operation".to_string(), "set_failed".to_string());
                ComponentHealth {
                    status: "unhealthy".to_string(),
                    last_check: chrono::Utc::now().to_rfc3339(),
                    response_time_ms: start_time.elapsed().as_millis() as u64,
                    details,
                    error: Some(format!("Storage set failed: {e}")),
                }
            }
        }
    }

    /// Check metrics component health
    async fn check_metrics_health(&self) -> ComponentHealth {
        let start_time = Instant::now();
        let mut details = HashMap::new();

        // Test metrics collection
        let summary = self.metrics.get_summary().await;

        details.insert(
            "total_operations".to_string(),
            summary.total_operations.to_string(),
        );
        details.insert(
            "uptime_seconds".to_string(),
            summary.uptime.as_secs().to_string(),
        );
        details.insert(
            "active_connections".to_string(),
            summary.active_connections.to_string(),
        );

        ComponentHealth {
            status: "healthy".to_string(),
            last_check: chrono::Utc::now().to_rfc3339(),
            response_time_ms: start_time.elapsed().as_millis() as u64,
            details,
            error: None,
        }
    }

    /// Check memory usage health
    async fn check_memory_health(&self) -> ComponentHealth {
        let start_time = Instant::now();
        let mut details = HashMap::new();

        let performance = self.metrics.get_performance_metrics().await;
        let current_memory = performance.current_memory_usage;
        let peak_memory = performance.peak_memory_usage;

        details.insert(
            "current_memory_bytes".to_string(),
            current_memory.to_string(),
        );
        details.insert("peak_memory_bytes".to_string(), peak_memory.to_string());

        // Check if we have system memory info available
        let status = if let Ok(sys_info) = sys_info::mem_info() {
            let total_memory = sys_info.total * 1024; // Convert KB to bytes
            let memory_usage_percent = (current_memory as f64 / total_memory as f64) * 100.0;

            details.insert(
                "total_system_memory_bytes".to_string(),
                total_memory.to_string(),
            );
            details.insert(
                "memory_usage_percent".to_string(),
                format!("{memory_usage_percent:.2}"),
            );

            if memory_usage_percent > 90.0 {
                "unhealthy"
            } else if memory_usage_percent > 80.0 {
                "degraded"
            } else {
                "healthy"
            }
        } else {
            // If we can't get system info, just check if memory usage is reasonable
            details.insert("system_info".to_string(), "unavailable".to_string());

            if current_memory > 1024 * 1024 * 1024 {
                // 1GB
                "degraded"
            } else {
                "healthy"
            }
        };

        ComponentHealth {
            status: status.to_string(),
            last_check: chrono::Utc::now().to_rfc3339(),
            response_time_ms: start_time.elapsed().as_millis() as u64,
            details,
            error: if status == "unhealthy" {
                Some("High memory usage detected".to_string())
            } else {
                None
            },
        }
    }

    /// Check disk space health
    async fn check_disk_health(&self) -> ComponentHealth {
        let start_time = Instant::now();
        let mut details = HashMap::new();

        // For now, just check if we can write to the current directory
        // In a production system, this would check AOF directory space
        let test_file = ".health_check_disk_test";

        let status = match tokio::fs::write(test_file, "test").await {
            Ok(()) => {
                // Clean up test file
                let _ = tokio::fs::remove_file(test_file).await;
                details.insert("disk_write_test".to_string(), "success".to_string());
                "healthy"
            }
            Err(e) => {
                details.insert("disk_write_test".to_string(), "failed".to_string());
                details.insert("error".to_string(), e.to_string());
                "unhealthy"
            }
        };

        ComponentHealth {
            status: status.to_string(),
            last_check: chrono::Utc::now().to_rfc3339(),
            response_time_ms: start_time.elapsed().as_millis() as u64,
            details,
            error: if status == "unhealthy" {
                Some("Disk write test failed".to_string())
            } else {
                None
            },
        }
    }

    /// Check network health
    async fn check_network_health(&self) -> ComponentHealth {
        let start_time = Instant::now();
        let mut details = HashMap::new();

        let network_metrics = self.metrics.get_network_metrics().await;

        details.insert(
            "active_connections".to_string(),
            network_metrics.active_connections.to_string(),
        );
        details.insert(
            "total_bytes_read".to_string(),
            network_metrics.total_bytes_read.to_string(),
        );
        details.insert(
            "total_bytes_written".to_string(),
            network_metrics.total_bytes_written.to_string(),
        );
        details.insert(
            "connections_accepted".to_string(),
            network_metrics.connections_accepted.to_string(),
        );
        details.insert(
            "connections_rejected".to_string(),
            network_metrics.connections_rejected.to_string(),
        );

        // Calculate rejection rate
        let total_connection_attempts =
            network_metrics.connections_accepted + network_metrics.connections_rejected;
        let rejection_rate = if total_connection_attempts > 0 {
            (network_metrics.connections_rejected as f64 / total_connection_attempts as f64) * 100.0
        } else {
            0.0
        };

        details.insert(
            "connection_rejection_rate_percent".to_string(),
            format!("{rejection_rate:.2}"),
        );

        let status = if rejection_rate > 50.0 {
            "unhealthy"
        } else if rejection_rate > 20.0 {
            "degraded"
        } else {
            "healthy"
        };

        ComponentHealth {
            status: status.to_string(),
            last_check: chrono::Utc::now().to_rfc3339(),
            response_time_ms: start_time.elapsed().as_millis() as u64,
            details,
            error: if status == "unhealthy" {
                Some("High connection rejection rate".to_string())
            } else {
                None
            },
        }
    }
}

impl HealthCheckResult {
    /// Check if the overall health is healthy
    pub fn is_healthy(&self) -> bool {
        self.is_healthy
    }

    /// Get the health status
    pub fn status(&self) -> &HealthStatus {
        &self.status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::MetricsCollector;
    use crate::storage::MemoryStore;

    #[tokio::test]
    async fn test_health_checker_creation() {
        let storage = Arc::new(MemoryStore::new());
        let metrics = Arc::new(MetricsCollector::new());

        let health_checker = HealthChecker::new(storage, metrics);

        // Basic test that it was created successfully
        assert!(health_checker.is_alive().await);
    }

    #[tokio::test]
    async fn test_storage_health_check() {
        let storage = Arc::new(MemoryStore::new());
        let metrics = Arc::new(MetricsCollector::new());
        let health_checker = HealthChecker::new(storage, metrics);

        let storage_health = health_checker.check_storage_health().await;

        assert_eq!(storage_health.status, "healthy");
        assert!(storage_health.error.is_none());
        // Response time should be reasonable (removed always-true comparison)
        assert!(storage_health.details.contains_key("test_operation"));
    }

    #[tokio::test]
    async fn test_metrics_health_check() {
        let storage = Arc::new(MemoryStore::new());
        let metrics = Arc::new(MetricsCollector::new());
        let health_checker = HealthChecker::new(storage, metrics);

        let metrics_health = health_checker.check_metrics_health().await;

        assert_eq!(metrics_health.status, "healthy");
        assert!(metrics_health.error.is_none());
        assert!(metrics_health.details.contains_key("total_operations"));
        assert!(metrics_health.details.contains_key("uptime_seconds"));
    }

    #[tokio::test]
    async fn test_full_health_check() {
        let storage = Arc::new(MemoryStore::new());
        let metrics = Arc::new(MetricsCollector::new());
        let health_checker = HealthChecker::new(storage, metrics);

        let health_result = health_checker.check_all_components().await;

        // Should have multiple components
        assert!(health_result.status.components.len() >= 4);
        assert!(health_result.status.components.contains_key("storage"));
        assert!(health_result.status.components.contains_key("metrics"));
        assert!(health_result.status.components.contains_key("memory"));
        assert!(health_result.status.components.contains_key("network"));

        // Summary should be calculated correctly
        assert_eq!(
            health_result.status.summary.total_components,
            health_result.status.components.len()
        );
        assert!(health_result.status.summary.healthy_components > 0);
    }

    #[tokio::test]
    async fn test_readiness_check() {
        let storage = Arc::new(MemoryStore::new());
        let metrics = Arc::new(MetricsCollector::new());
        let health_checker = HealthChecker::new(storage, metrics);

        let is_ready = health_checker.is_ready().await;

        // Should be ready if storage and metrics are healthy
        assert!(is_ready);
    }

    #[tokio::test]
    async fn test_liveness_check() {
        let storage = Arc::new(MemoryStore::new());
        let metrics = Arc::new(MetricsCollector::new());
        let health_checker = HealthChecker::new(storage, metrics);

        let is_alive = health_checker.is_alive().await;

        // Should always be alive for basic implementation
        assert!(is_alive);
    }

    #[tokio::test]
    async fn test_cached_status() {
        let storage = Arc::new(MemoryStore::new());
        let metrics = Arc::new(MetricsCollector::new());
        let health_checker = HealthChecker::new(storage, metrics);

        // Initially no cached status
        assert!(health_checker.get_cached_status().await.is_none());

        // Run a health check to populate cache
        let _result = health_checker.check_all_components().await;

        // Now should have cached status
        let cached = health_checker.get_cached_status().await;
        assert!(cached.is_some());

        let cached_status = cached.unwrap();
        assert!(!cached_status.components.is_empty());
        // Uptime should be reasonable (removed always-true comparison)
    }
}
