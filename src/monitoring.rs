//! Monitoring and health check infrastructure for RustyPotato
//! 
//! This module provides health check endpoints, log rotation management,
//! and enhanced monitoring capabilities for production deployments.

pub mod health;
pub mod log_rotation;

pub use health::{HealthChecker, HealthStatus, ComponentHealth, HealthCheckResult};
pub use log_rotation::{LogRotationManager, LogRotationConfig, RotationPolicy};

use crate::error::{Result, RustyPotatoError};
use crate::metrics::MetricsCollector;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, error, info, warn};

/// Monitoring server that provides health checks and enhanced metrics
pub struct MonitoringServer {
    health_checker: Arc<HealthChecker>,
    metrics_collector: Arc<MetricsCollector>,
    log_rotation: Arc<LogRotationManager>,
    bind_address: String,
    port: u16,
}

impl MonitoringServer {
    /// Create a new monitoring server
    pub fn new(
        health_checker: Arc<HealthChecker>,
        metrics_collector: Arc<MetricsCollector>,
        log_rotation: Arc<LogRotationManager>,
        bind_address: String,
        port: u16,
    ) -> Self {
        Self {
            health_checker,
            metrics_collector,
            log_rotation,
            bind_address,
            port,
        }
    }

    /// Start the monitoring server
    pub async fn start(&self) -> Result<()> {
        let addr = format!("{}:{}", self.bind_address, self.port);
        let listener = TcpListener::bind(&addr).await
            .map_err(|e| RustyPotatoError::NetworkError {
                message: format!("Failed to bind monitoring server to {}: {}", addr, e),
                source: Some(Box::new(e)),
                connection_id: None,
            })?;
        
        info!("Monitoring server listening on {}", addr);

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    debug!("Monitoring request from {}", addr);
                    let health_checker = Arc::clone(&self.health_checker);
                    let metrics_collector = Arc::clone(&self.metrics_collector);
                    let log_rotation = Arc::clone(&self.log_rotation);
                    
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_request(
                            stream, 
                            health_checker, 
                            metrics_collector,
                            log_rotation
                        ).await {
                            warn!("Error handling monitoring request from {}: {}", addr, e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept monitoring connection: {}", e);
                }
            }
        }
    }

    /// Handle a single HTTP request
    async fn handle_request(
        mut stream: TcpStream,
        health_checker: Arc<HealthChecker>,
        metrics_collector: Arc<MetricsCollector>,
        log_rotation: Arc<LogRotationManager>,
    ) -> Result<()> {
        let mut buffer = [0; 2048];
        let bytes_read = stream.read(&mut buffer).await
            .map_err(|e| RustyPotatoError::NetworkError {
                message: format!("Failed to read monitoring request: {}", e),
                source: Some(Box::new(e)),
                connection_id: None,
            })?;
        
        if bytes_read == 0 {
            return Ok(());
        }

        let request = String::from_utf8_lossy(&buffer[..bytes_read]);
        let request_line = request.lines().next().unwrap_or("");
        debug!("Monitoring request: {}", request_line);

        let response = if request_line.starts_with("GET /health") {
            Self::handle_health_check(&health_checker).await
        } else if request_line.starts_with("GET /health/ready") {
            Self::handle_readiness_check(&health_checker).await
        } else if request_line.starts_with("GET /health/live") {
            Self::handle_liveness_check(&health_checker).await
        } else if request_line.starts_with("GET /metrics") {
            Self::handle_metrics_request(&metrics_collector).await
        } else if request_line.starts_with("GET /metrics/summary") {
            Self::handle_metrics_summary(&metrics_collector).await
        } else if request_line.starts_with("POST /logs/rotate") {
            Self::handle_log_rotation(&log_rotation).await
        } else if request_line.starts_with("GET /logs/status") {
            Self::handle_log_status(&log_rotation).await
        } else {
            Self::handle_not_found()
        };

        stream.write_all(response.as_bytes()).await
            .map_err(|e| RustyPotatoError::NetworkError {
                message: format!("Failed to write monitoring response: {}", e),
                source: Some(Box::new(e)),
                connection_id: None,
            })?;

        stream.flush().await
            .map_err(|e| RustyPotatoError::NetworkError {
                message: format!("Failed to flush monitoring response: {}", e),
                source: Some(Box::new(e)),
                connection_id: None,
            })?;

        Ok(())
    }

    /// Handle health check request
    async fn handle_health_check(health_checker: &HealthChecker) -> String {
        let health_result = health_checker.check_all_components().await;
        let status_code = if health_result.is_healthy() { 200 } else { 503 };
        
        let json_response = serde_json::to_string_pretty(&health_result.status)
            .unwrap_or_else(|_| r#"{"status": "error", "message": "Failed to serialize health check"}"#.to_string());

        format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            status_code,
            if status_code == 200 { "OK" } else { "Service Unavailable" },
            json_response.len(),
            json_response
        )
    }

    /// Handle readiness check request
    async fn handle_readiness_check(health_checker: &HealthChecker) -> String {
        let is_ready = health_checker.is_ready().await;
        let status_code = if is_ready { 200 } else { 503 };
        let response_body = if is_ready { "READY" } else { "NOT READY" };

        format!(
            "HTTP/1.1 {} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
            status_code,
            if status_code == 200 { "OK" } else { "Service Unavailable" },
            response_body.len(),
            response_body
        )
    }

    /// Handle liveness check request
    async fn handle_liveness_check(health_checker: &HealthChecker) -> String {
        let is_alive = health_checker.is_alive().await;
        let status_code = if is_alive { 200 } else { 503 };
        let response_body = if is_alive { "ALIVE" } else { "NOT ALIVE" };

        format!(
            "HTTP/1.1 {} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
            status_code,
            if status_code == 200 { "OK" } else { "Service Unavailable" },
            response_body.len(),
            response_body
        )
    }

    /// Handle metrics request (Prometheus format)
    async fn handle_metrics_request(metrics_collector: &MetricsCollector) -> String {
        let metrics = Self::format_prometheus_metrics(metrics_collector).await;
        
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            metrics.len(),
            metrics
        )
    }

    /// Handle metrics summary request (JSON format)
    async fn handle_metrics_summary(metrics_collector: &MetricsCollector) -> String {
        let summary = metrics_collector.get_summary().await;
        let json_response = serde_json::to_string_pretty(&summary)
            .unwrap_or_else(|_| r#"{"error": "Failed to serialize metrics summary"}"#.to_string());

        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            json_response.len(),
            json_response
        )
    }

    /// Handle log rotation request
    async fn handle_log_rotation(log_rotation: &LogRotationManager) -> String {
        match log_rotation.rotate_now().await {
            Ok(()) => {
                let response_body = r#"{"status": "success", "message": "Log rotation completed"}"#;
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    response_body.len(),
                    response_body
                )
            }
            Err(e) => {
                let response_body = format!(r#"{{"status": "error", "message": "Log rotation failed: {}"}}"#, e);
                format!(
                    "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    response_body.len(),
                    response_body
                )
            }
        }
    }

    /// Handle log status request
    async fn handle_log_status(log_rotation: &LogRotationManager) -> String {
        let status = log_rotation.get_status().await;
        let json_response = serde_json::to_string_pretty(&status)
            .unwrap_or_else(|_| r#"{"error": "Failed to serialize log status"}"#.to_string());

        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            json_response.len(),
            json_response
        )
    }

    /// Handle 404 Not Found
    fn handle_not_found() -> String {
        let response_body = "Not Found";
        format!(
            "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
            response_body.len(),
            response_body
        )
    }

    /// Format metrics in Prometheus format (enhanced version)
    async fn format_prometheus_metrics(collector: &MetricsCollector) -> String {
        let summary = collector.get_summary().await;
        let performance = collector.get_performance_metrics().await;
        let commands = collector.get_command_metrics().await;
        let network = collector.get_network_metrics().await;
        let storage = collector.get_storage_metrics().await;

        let mut output = String::new();

        // Server info with enhanced metadata
        output.push_str("# HELP rustypotato_info Server information\n");
        output.push_str("# TYPE rustypotato_info gauge\n");
        output.push_str(&format!("rustypotato_info{{version=\"{}\",build=\"{}\"}} 1\n", 
                                env!("CARGO_PKG_VERSION"), 
                                option_env!("BUILD_ID").unwrap_or("unknown")));

        // Uptime
        output.push_str("# HELP rustypotato_uptime_seconds Server uptime in seconds\n");
        output.push_str("# TYPE rustypotato_uptime_seconds counter\n");
        output.push_str(&format!("rustypotato_uptime_seconds {}\n", summary.uptime.as_secs()));

        // Operations
        output.push_str("# HELP rustypotato_operations_total Total number of operations\n");
        output.push_str("# TYPE rustypotato_operations_total counter\n");
        output.push_str(&format!("rustypotato_operations_total {}\n", summary.total_operations));

        output.push_str("# HELP rustypotato_operations_per_second Operations per second\n");
        output.push_str("# TYPE rustypotato_operations_per_second gauge\n");
        output.push_str(&format!("rustypotato_operations_per_second {}\n", summary.operations_per_second));

        // Enhanced latency metrics
        if let Some(_avg_latency) = summary.average_latency {
            output.push_str("# HELP rustypotato_latency_seconds Average operation latency\n");
            output.push_str("# TYPE rustypotato_latency_seconds histogram\n");
            output.push_str(&format!("rustypotato_latency_seconds_sum {}\n", 
                                   performance.latency_histogram.sum().as_secs_f64()));
            output.push_str(&format!("rustypotato_latency_seconds_count {}\n", 
                                   performance.latency_histogram.count()));
            
            // Add histogram buckets
            let percentiles = [50.0, 75.0, 90.0, 95.0, 99.0, 99.9];
            for p in &percentiles {
                if let Some(latency) = performance.latency_histogram.percentile(*p) {
                    output.push_str(&format!("rustypotato_latency_seconds{{quantile=\"{:.3}\"}} {}\n", 
                                           p / 100.0, latency.as_secs_f64()));
                }
            }
        }

        // Commands with error rates
        output.push_str("# HELP rustypotato_commands_total Total number of commands by type\n");
        output.push_str("# TYPE rustypotato_commands_total counter\n");
        for (command, count) in &commands.command_counts {
            output.push_str(&format!("rustypotato_commands_total{{command=\"{}\"}} {}\n", command, count));
        }

        output.push_str("# HELP rustypotato_command_errors_total Total number of command errors by type\n");
        output.push_str("# TYPE rustypotato_command_errors_total counter\n");
        for (command, errors) in &commands.command_errors {
            output.push_str(&format!("rustypotato_command_errors_total{{command=\"{}\"}} {}\n", command, errors));
        }

        output.push_str("# HELP rustypotato_command_error_rate Command error rate by type\n");
        output.push_str("# TYPE rustypotato_command_error_rate gauge\n");
        for command in commands.command_counts.keys() {
            let error_rate = commands.command_error_rate(command);
            output.push_str(&format!("rustypotato_command_error_rate{{command=\"{}\"}} {}\n", command, error_rate));
        }

        // Network metrics
        output.push_str("# HELP rustypotato_network_bytes_total Total network bytes transferred\n");
        output.push_str("# TYPE rustypotato_network_bytes_total counter\n");
        output.push_str(&format!("rustypotato_network_bytes_total{{direction=\"read\"}} {}\n", network.total_bytes_read));
        output.push_str(&format!("rustypotato_network_bytes_total{{direction=\"written\"}} {}\n", network.total_bytes_written));

        output.push_str("# HELP rustypotato_connections_total Total connections by status\n");
        output.push_str("# TYPE rustypotato_connections_total counter\n");
        output.push_str(&format!("rustypotato_connections_total{{status=\"accepted\"}} {}\n", network.connections_accepted));
        output.push_str(&format!("rustypotato_connections_total{{status=\"rejected\"}} {}\n", network.connections_rejected));
        output.push_str(&format!("rustypotato_connections_total{{status=\"closed\"}} {}\n", network.connections_closed));

        output.push_str("# HELP rustypotato_active_connections Current number of active connections\n");
        output.push_str("# TYPE rustypotato_active_connections gauge\n");
        output.push_str(&format!("rustypotato_active_connections {}\n", network.active_connections));

        // Memory metrics
        output.push_str("# HELP rustypotato_memory_usage_bytes Current memory usage\n");
        output.push_str("# TYPE rustypotato_memory_usage_bytes gauge\n");
        output.push_str(&format!("rustypotato_memory_usage_bytes {}\n", performance.current_memory_usage));

        output.push_str("# HELP rustypotato_memory_peak_bytes Peak memory usage\n");
        output.push_str("# TYPE rustypotato_memory_peak_bytes gauge\n");
        output.push_str(&format!("rustypotato_memory_peak_bytes {}\n", performance.peak_memory_usage));

        // Storage metrics
        output.push_str("# HELP rustypotato_storage_operations_total Total storage operations\n");
        output.push_str("# TYPE rustypotato_storage_operations_total counter\n");
        output.push_str(&format!("rustypotato_storage_operations_total{{type=\"aof_write\"}} {}\n", storage.aof_writes));
        output.push_str(&format!("rustypotato_storage_operations_total{{type=\"memory\"}} {}\n", storage.memory_operations));

        output.push_str("# HELP rustypotato_aof_bytes_written_total Total bytes written to AOF\n");
        output.push_str("# TYPE rustypotato_aof_bytes_written_total counter\n");
        output.push_str(&format!("rustypotato_aof_bytes_written_total {}\n", storage.aof_bytes_written));

        output.push_str("# HELP rustypotato_cache_operations_total Cache operations\n");
        output.push_str("# TYPE rustypotato_cache_operations_total counter\n");
        output.push_str(&format!("rustypotato_cache_operations_total{{result=\"hit\"}} {}\n", storage.cache_hits));
        output.push_str(&format!("rustypotato_cache_operations_total{{result=\"miss\"}} {}\n", storage.cache_misses));

        if storage.cache_hits + storage.cache_misses > 0 {
            output.push_str("# HELP rustypotato_cache_hit_rate Cache hit rate\n");
            output.push_str("# TYPE rustypotato_cache_hit_rate gauge\n");
            output.push_str(&format!("rustypotato_cache_hit_rate {}\n", storage.cache_hit_rate()));
        }

        output
    }
}