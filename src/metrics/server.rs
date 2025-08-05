//! HTTP metrics server for exposing Prometheus-compatible metrics
//!
//! This module provides an HTTP server that exposes metrics in Prometheus
//! format for monitoring and alerting systems.

use crate::metrics::MetricsCollector;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info, warn};

/// HTTP metrics server for exposing metrics
pub struct MetricsServer {
    collector: Arc<MetricsCollector>,
    bind_address: String,
    port: u16,
}

impl MetricsServer {
    /// Create a new metrics server
    pub fn new(collector: Arc<MetricsCollector>, bind_address: String, port: u16) -> Self {
        Self {
            collector,
            bind_address,
            port,
        }
    }

    /// Start the metrics server
    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let addr = format!("{}:{}", self.bind_address, self.port);
        let listener = TcpListener::bind(&addr).await?;

        info!("Metrics server listening on {}", addr);

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    debug!("Metrics request from {}", addr);
                    let collector = Arc::clone(&self.collector);
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_request(stream, collector).await {
                            warn!("Error handling metrics request from {}: {}", addr, e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept metrics connection: {}", e);
                }
            }
        }
    }

    /// Handle a single HTTP request
    async fn handle_request(
        mut stream: TcpStream,
        collector: Arc<MetricsCollector>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut buffer = [0; 1024];
        let bytes_read = stream.read(&mut buffer).await?;

        if bytes_read == 0 {
            return Ok(());
        }

        let request = String::from_utf8_lossy(&buffer[..bytes_read]);
        debug!("Metrics request: {}", request.lines().next().unwrap_or(""));

        // Simple HTTP request parsing
        if request.starts_with("GET /metrics") {
            let metrics = Self::format_prometheus_metrics(&collector).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
                metrics.len(),
                metrics
            );
            stream.write_all(response.as_bytes()).await?;
        } else if request.starts_with("GET /health") {
            let response =
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nOK";
            stream.write_all(response.as_bytes()).await?;
        } else {
            let response = "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 9\r\n\r\nNot Found";
            stream.write_all(response.as_bytes()).await?;
        }

        stream.flush().await?;
        Ok(())
    }

    /// Format metrics in Prometheus format
    async fn format_prometheus_metrics(collector: &MetricsCollector) -> String {
        let summary = collector.get_summary().await;
        let performance = collector.get_performance_metrics().await;
        let commands = collector.get_command_metrics().await;
        let network = collector.get_network_metrics().await;
        let storage = collector.get_storage_metrics().await;

        let mut output = String::new();

        // Server info
        output.push_str("# HELP rustypotato_uptime_seconds Server uptime in seconds\n");
        output.push_str("# TYPE rustypotato_uptime_seconds counter\n");
        output.push_str(&format!(
            "rustypotato_uptime_seconds {}\n",
            summary.uptime.as_secs()
        ));

        // Operations
        output.push_str("# HELP rustypotato_operations_total Total number of operations\n");
        output.push_str("# TYPE rustypotato_operations_total counter\n");
        output.push_str(&format!(
            "rustypotato_operations_total {}\n",
            summary.total_operations
        ));

        output.push_str("# HELP rustypotato_operations_per_second Operations per second\n");
        output.push_str("# TYPE rustypotato_operations_per_second gauge\n");
        output.push_str(&format!(
            "rustypotato_operations_per_second {}\n",
            summary.operations_per_second
        ));

        // Latency
        if let Some(avg_latency) = summary.average_latency {
            output.push_str("# HELP rustypotato_latency_seconds_avg Average operation latency\n");
            output.push_str("# TYPE rustypotato_latency_seconds_avg gauge\n");
            output.push_str(&format!(
                "rustypotato_latency_seconds_avg {}\n",
                avg_latency.as_secs_f64()
            ));
        }

        if let Some(p95_latency) = summary.p95_latency {
            output.push_str("# HELP rustypotato_latency_seconds_p95 95th percentile latency\n");
            output.push_str("# TYPE rustypotato_latency_seconds_p95 gauge\n");
            output.push_str(&format!(
                "rustypotato_latency_seconds_p95 {}\n",
                p95_latency.as_secs_f64()
            ));
        }

        if let Some(p99_latency) = summary.p99_latency {
            output.push_str("# HELP rustypotato_latency_seconds_p99 99th percentile latency\n");
            output.push_str("# TYPE rustypotato_latency_seconds_p99 gauge\n");
            output.push_str(&format!(
                "rustypotato_latency_seconds_p99 {}\n",
                p99_latency.as_secs_f64()
            ));
        }

        // Commands
        output.push_str("# HELP rustypotato_commands_total Total number of commands by type\n");
        output.push_str("# TYPE rustypotato_commands_total counter\n");
        for (command, count) in &commands.command_counts {
            output.push_str(&format!(
                "rustypotato_commands_total{{command=\"{}\"}} {}\n",
                command, count
            ));
        }

        output.push_str(
            "# HELP rustypotato_command_errors_total Total number of command errors by type\n",
        );
        output.push_str("# TYPE rustypotato_command_errors_total counter\n");
        for (command, errors) in &commands.command_errors {
            output.push_str(&format!(
                "rustypotato_command_errors_total{{command=\"{}\"}} {}\n",
                command, errors
            ));
        }

        // Network
        output.push_str("# HELP rustypotato_network_bytes_total Total network bytes transferred\n");
        output.push_str("# TYPE rustypotato_network_bytes_total counter\n");
        output.push_str(&format!(
            "rustypotato_network_bytes_total{{direction=\"read\"}} {}\n",
            network.total_bytes_read
        ));
        output.push_str(&format!(
            "rustypotato_network_bytes_total{{direction=\"written\"}} {}\n",
            network.total_bytes_written
        ));

        output.push_str("# HELP rustypotato_connections_total Total connections by status\n");
        output.push_str("# TYPE rustypotato_connections_total counter\n");
        output.push_str(&format!(
            "rustypotato_connections_total{{status=\"accepted\"}} {}\n",
            network.connections_accepted
        ));
        output.push_str(&format!(
            "rustypotato_connections_total{{status=\"rejected\"}} {}\n",
            network.connections_rejected
        ));
        output.push_str(&format!(
            "rustypotato_connections_total{{status=\"closed\"}} {}\n",
            network.connections_closed
        ));

        output.push_str(
            "# HELP rustypotato_active_connections Current number of active connections\n",
        );
        output.push_str("# TYPE rustypotato_active_connections gauge\n");
        output.push_str(&format!(
            "rustypotato_active_connections {}\n",
            network.active_connections
        ));

        // Memory
        output.push_str("# HELP rustypotato_memory_usage_bytes Current memory usage\n");
        output.push_str("# TYPE rustypotato_memory_usage_bytes gauge\n");
        output.push_str(&format!(
            "rustypotato_memory_usage_bytes {}\n",
            performance.current_memory_usage
        ));

        output.push_str("# HELP rustypotato_memory_peak_bytes Peak memory usage\n");
        output.push_str("# TYPE rustypotato_memory_peak_bytes gauge\n");
        output.push_str(&format!(
            "rustypotato_memory_peak_bytes {}\n",
            performance.peak_memory_usage
        ));

        // Storage
        output.push_str("# HELP rustypotato_storage_operations_total Total storage operations\n");
        output.push_str("# TYPE rustypotato_storage_operations_total counter\n");
        output.push_str(&format!(
            "rustypotato_storage_operations_total{{type=\"aof_write\"}} {}\n",
            storage.aof_writes
        ));
        output.push_str(&format!(
            "rustypotato_storage_operations_total{{type=\"memory\"}} {}\n",
            storage.memory_operations
        ));

        output.push_str("# HELP rustypotato_aof_bytes_written_total Total bytes written to AOF\n");
        output.push_str("# TYPE rustypotato_aof_bytes_written_total counter\n");
        output.push_str(&format!(
            "rustypotato_aof_bytes_written_total {}\n",
            storage.aof_bytes_written
        ));

        output.push_str("# HELP rustypotato_cache_operations_total Cache operations\n");
        output.push_str("# TYPE rustypotato_cache_operations_total counter\n");
        output.push_str(&format!(
            "rustypotato_cache_operations_total{{result=\"hit\"}} {}\n",
            storage.cache_hits
        ));
        output.push_str(&format!(
            "rustypotato_cache_operations_total{{result=\"miss\"}} {}\n",
            storage.cache_misses
        ));

        if storage.cache_hits + storage.cache_misses > 0 {
            output.push_str("# HELP rustypotato_cache_hit_rate Cache hit rate\n");
            output.push_str("# TYPE rustypotato_cache_hit_rate gauge\n");
            output.push_str(&format!(
                "rustypotato_cache_hit_rate {}\n",
                storage.cache_hit_rate()
            ));
        }

        output
    }

    /// Get a simple JSON summary of metrics
    pub async fn get_json_summary(&self) -> String {
        let summary = self.collector.get_summary().await;

        format!(
            r#"{{
  "uptime_seconds": {},
  "total_operations": {},
  "operations_per_second": {:.2},
  "average_latency_microseconds": {},
  "p95_latency_microseconds": {},
  "p99_latency_microseconds": {},
  "total_commands": {},
  "most_frequent_command": {},
  "total_bytes_read": {},
  "total_bytes_written": {},
  "active_connections": {},
  "memory_usage_bytes": {},
  "storage_operations": {}
}}"#,
            summary.uptime.as_secs(),
            summary.total_operations,
            summary.operations_per_second,
            summary.average_latency.map(|d| d.as_micros()).unwrap_or(0),
            summary.p95_latency.map(|d| d.as_micros()).unwrap_or(0),
            summary.p99_latency.map(|d| d.as_micros()).unwrap_or(0),
            summary.total_commands,
            summary
                .most_frequent_command
                .map(|(cmd, count)| format!("\"{}({}times)\"", cmd, count))
                .unwrap_or_else(|| "null".to_string()),
            summary.total_bytes_read,
            summary.total_bytes_written,
            summary.active_connections,
            summary.memory_usage_bytes,
            summary.storage_operations
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_metrics_server_creation() {
        let collector = Arc::new(MetricsCollector::new());
        let server = MetricsServer::new(collector, "127.0.0.1".to_string(), 0);

        assert_eq!(server.bind_address, "127.0.0.1");
        assert_eq!(server.port, 0);
    }

    #[tokio::test]
    async fn test_prometheus_format() {
        let collector = Arc::new(MetricsCollector::new());

        // Add some test data
        collector
            .record_command_latency("SET", Duration::from_micros(100))
            .await;
        collector
            .record_command_latency("GET", Duration::from_micros(50))
            .await;
        collector.record_network_bytes(1024, 512).await;

        let metrics = MetricsServer::format_prometheus_metrics(&collector).await;

        assert!(metrics.contains("rustypotato_uptime_seconds"));
        assert!(metrics.contains("rustypotato_operations_total"));
        assert!(metrics.contains("rustypotato_commands_total{command=\"SET\"}"));
        assert!(metrics.contains("rustypotato_commands_total{command=\"GET\"}"));
        assert!(metrics.contains("rustypotato_network_bytes_total{direction=\"read\"}"));
        assert!(metrics.contains("# HELP"));
        assert!(metrics.contains("# TYPE"));
    }

    #[tokio::test]
    async fn test_json_summary() {
        let collector = Arc::new(MetricsCollector::new());
        let server = MetricsServer::new(collector.clone(), "127.0.0.1".to_string(), 0);

        // Add some test data
        collector
            .record_command_latency("SET", Duration::from_micros(100))
            .await;
        collector.record_memory_usage(1024 * 1024).await;

        let json = server.get_json_summary().await;

        assert!(json.contains("\"uptime_seconds\""));
        assert!(json.contains("\"total_operations\""));
        assert!(json.contains("\"operations_per_second\""));
        assert!(json.contains("\"memory_usage_bytes\""));

        // Should be valid JSON
        let _: serde_json::Value = serde_json::from_str(&json).expect("Invalid JSON");
    }

    #[tokio::test]
    async fn test_handle_request_simulation() {
        // This test simulates the request handling logic without actual network I/O
        let collector = Arc::new(MetricsCollector::new());

        // Add test data
        collector
            .record_command_latency("SET", Duration::from_micros(100))
            .await;

        let metrics = MetricsServer::format_prometheus_metrics(&collector).await;

        // Simulate HTTP response format
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            metrics.len(),
            metrics
        );

        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("Content-Type: text/plain"));
        assert!(response.contains("rustypotato_operations_total"));
    }
}
