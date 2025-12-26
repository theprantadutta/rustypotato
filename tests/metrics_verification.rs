//! Metrics verification tests
//!
//! Tests verify the accuracy and consistency of the metrics collection system:
//! - Command latency recording
//! - Operation counting
//! - Connection tracking
//! - Histogram accuracy
//! - Memory usage tracking

use rustypotato::metrics::collector::{
    CommandMetrics, ConnectionEvent, MetricsCollector, NetworkMetrics, PerformanceMetrics,
    StorageMetrics, StorageOperation,
};
use rustypotato::metrics::Histogram;
use std::time::Duration;

// ==================== Histogram Tests ====================

#[test]
fn test_histogram_empty() {
    let histogram = Histogram::new();

    assert_eq!(histogram.count(), 0);
    assert_eq!(histogram.sum(), Duration::ZERO);
    assert!(histogram.average().is_none());
    assert!(histogram.percentile(50.0).is_none());
}

#[test]
fn test_histogram_single_value() {
    let mut histogram = Histogram::new();
    histogram.record(Duration::from_millis(100));

    assert_eq!(histogram.count(), 1);
    assert!(histogram.average().is_some());
}

#[test]
fn test_histogram_multiple_values() {
    let mut histogram = Histogram::new();
    histogram.record(Duration::from_micros(100));
    histogram.record(Duration::from_micros(200));
    histogram.record(Duration::from_micros(300));

    assert_eq!(histogram.count(), 3);
    assert_eq!(histogram.sum(), Duration::from_micros(600));

    let avg = histogram.average().unwrap();
    // Average should be ~200μs
    assert!(
        avg >= Duration::from_micros(190) && avg <= Duration::from_micros(210),
        "Average should be ~200μs, got {:?}",
        avg
    );
}

#[test]
fn test_histogram_percentiles() {
    let mut histogram = Histogram::new();

    // Add 100 values from 1μs to 100μs
    for i in 1..=100 {
        histogram.record(Duration::from_micros(i));
    }

    assert_eq!(histogram.count(), 100);

    // Percentiles should exist
    let p50 = histogram.percentile(50.0);
    assert!(p50.is_some(), "P50 should exist");

    let p95 = histogram.percentile(95.0);
    assert!(p95.is_some(), "P95 should exist");

    let p99 = histogram.percentile(99.0);
    assert!(p99.is_some(), "P99 should exist");
}

#[test]
fn test_histogram_reset() {
    let mut histogram = Histogram::new();
    histogram.record(Duration::from_millis(100));
    histogram.record(Duration::from_millis(200));

    histogram.reset();

    assert_eq!(histogram.count(), 0);
    assert_eq!(histogram.sum(), Duration::ZERO);
}

// ==================== Performance Metrics Tests ====================

#[test]
fn test_performance_metrics_new() {
    let metrics = PerformanceMetrics::new();

    assert_eq!(metrics.total_operations, 0);
    assert_eq!(metrics.current_memory_usage, 0);
    assert_eq!(metrics.peak_memory_usage, 0);
}

#[test]
fn test_performance_metrics_operation_latency() {
    let mut metrics = PerformanceMetrics::new();

    metrics.record_operation_latency(Duration::from_millis(10));
    metrics.record_operation_latency(Duration::from_millis(20));
    metrics.record_operation_latency(Duration::from_millis(30));

    assert_eq!(metrics.total_operations, 3);
    assert_eq!(metrics.latency_histogram.count(), 3);
}

#[test]
fn test_performance_metrics_memory_tracking() {
    let mut metrics = PerformanceMetrics::new();

    metrics.update_memory_usage(1000);
    assert_eq!(metrics.current_memory_usage, 1000);
    assert_eq!(metrics.peak_memory_usage, 1000);

    metrics.update_memory_usage(2000);
    assert_eq!(metrics.current_memory_usage, 2000);
    assert_eq!(metrics.peak_memory_usage, 2000);

    // Memory decreases but peak stays
    metrics.update_memory_usage(500);
    assert_eq!(metrics.current_memory_usage, 500);
    assert_eq!(metrics.peak_memory_usage, 2000);
}

#[test]
fn test_performance_metrics_reset() {
    let mut metrics = PerformanceMetrics::new();

    metrics.record_operation_latency(Duration::from_millis(100));
    metrics.update_memory_usage(5000);

    metrics.reset();

    assert_eq!(metrics.total_operations, 0);
    assert_eq!(metrics.current_memory_usage, 0);
    assert_eq!(metrics.peak_memory_usage, 0);
}

// ==================== Command Metrics Tests ====================

#[test]
fn test_command_metrics_recording() {
    let mut metrics = CommandMetrics::new();

    metrics.record_command("SET", Duration::from_millis(10));
    metrics.record_command("GET", Duration::from_millis(5));
    metrics.record_command("SET", Duration::from_millis(15));

    assert_eq!(metrics.total_commands(), 3);
    assert_eq!(*metrics.command_counts.get("SET").unwrap(), 2);
    assert_eq!(*metrics.command_counts.get("GET").unwrap(), 1);
}

#[test]
fn test_command_metrics_most_frequent() {
    let mut metrics = CommandMetrics::new();

    metrics.record_command("SET", Duration::from_millis(10));
    metrics.record_command("GET", Duration::from_millis(5));
    metrics.record_command("GET", Duration::from_millis(5));
    metrics.record_command("GET", Duration::from_millis(5));

    let (cmd, count) = metrics.most_frequent_command().unwrap();
    assert_eq!(cmd, "GET");
    assert_eq!(count, 3);
}

#[test]
fn test_command_metrics_error_rate() {
    let mut metrics = CommandMetrics::new();

    // 10 successful commands
    for _ in 0..10 {
        metrics.record_command("SET", Duration::from_millis(10));
    }

    // 2 errors
    metrics.record_command_error("SET");
    metrics.record_command_error("SET");

    let error_rate = metrics.command_error_rate("SET");
    assert!(
        (error_rate - 0.2).abs() < 0.01,
        "Error rate should be ~20%, got {}",
        error_rate
    );
}

#[test]
fn test_command_metrics_reset() {
    let mut metrics = CommandMetrics::new();

    metrics.record_command("SET", Duration::from_millis(10));
    metrics.record_command_error("SET");

    metrics.reset();

    assert_eq!(metrics.total_commands(), 0);
    assert!(metrics.command_counts.is_empty());
    assert!(metrics.command_errors.is_empty());
}

// ==================== Network Metrics Tests ====================

#[test]
fn test_network_metrics_bytes_tracking() {
    let mut metrics = NetworkMetrics::new();

    metrics.add_bytes_read(1000);
    metrics.add_bytes_written(500);

    assert_eq!(metrics.total_bytes_read, 1000);
    assert_eq!(metrics.total_bytes_written, 500);

    metrics.add_bytes_read(500);
    metrics.add_bytes_written(250);

    assert_eq!(metrics.total_bytes_read, 1500);
    assert_eq!(metrics.total_bytes_written, 750);
}

#[test]
fn test_network_metrics_connection_tracking() {
    let mut metrics = NetworkMetrics::new();

    // Accept 5 connections
    for _ in 0..5 {
        metrics.increment_connections_accepted();
    }

    assert_eq!(metrics.connections_accepted, 5);
    assert_eq!(metrics.active_connections, 5);

    // Close 2 connections
    metrics.increment_connections_closed();
    metrics.increment_connections_closed();

    assert_eq!(metrics.connections_closed, 2);
    assert_eq!(metrics.active_connections, 3);

    // Reject 1 connection
    metrics.increment_connections_rejected();

    assert_eq!(metrics.connections_rejected, 1);
    assert_eq!(metrics.active_connections, 3); // Rejected doesn't add to active
}

#[test]
fn test_network_metrics_throughput() {
    let mut metrics = NetworkMetrics::new();

    // Add 1MB of data
    metrics.add_bytes_read(500_000);
    metrics.add_bytes_written(500_000);

    // Calculate throughput over 1 second
    let throughput = metrics.throughput_mbps(Duration::from_secs(1));

    // 1MB = 8Mb, so throughput should be 8 Mbps
    assert!(
        (throughput - 8.0).abs() < 0.1,
        "Throughput should be ~8 Mbps, got {}",
        throughput
    );
}

#[test]
fn test_network_metrics_reset() {
    let mut metrics = NetworkMetrics::new();

    metrics.add_bytes_read(1000);
    metrics.increment_connections_accepted();
    metrics.increment_connections_closed();

    metrics.reset();

    assert_eq!(metrics.total_bytes_read, 0);
    assert_eq!(metrics.total_bytes_written, 0);
    assert_eq!(metrics.connections_accepted, 0);
    assert_eq!(metrics.connections_closed, 0);
    // active_connections should NOT be reset as it represents current state
}

// ==================== Storage Metrics Tests ====================

#[test]
fn test_storage_metrics_new() {
    let metrics = StorageMetrics::new();
    assert_eq!(metrics.aof_writes, 0);
    assert_eq!(metrics.memory_operations, 0);
}

// ==================== Metrics Collector Integration Tests ====================

#[tokio::test]
async fn test_metrics_collector_command_latency() {
    let collector = MetricsCollector::new();

    collector
        .record_command_latency("SET", Duration::from_millis(10))
        .await;
    collector
        .record_command_latency("GET", Duration::from_millis(5))
        .await;
    collector
        .record_command_latency("SET", Duration::from_millis(15))
        .await;

    let command_metrics = collector.get_command_metrics().await;
    assert_eq!(command_metrics.total_commands(), 3);

    let perf_metrics = collector.get_performance_metrics().await;
    assert_eq!(perf_metrics.total_operations, 3);
}

#[tokio::test]
async fn test_metrics_collector_network_events() {
    let collector = MetricsCollector::new();

    collector
        .record_connection_event(ConnectionEvent::Connected)
        .await;
    collector
        .record_connection_event(ConnectionEvent::Connected)
        .await;
    collector
        .record_connection_event(ConnectionEvent::Disconnected)
        .await;
    collector
        .record_connection_event(ConnectionEvent::Rejected)
        .await;

    let network_metrics = collector.get_network_metrics().await;
    assert_eq!(network_metrics.connections_accepted, 2);
    assert_eq!(network_metrics.connections_closed, 1);
    assert_eq!(network_metrics.connections_rejected, 1);
    assert_eq!(network_metrics.active_connections, 1);
}

#[tokio::test]
async fn test_metrics_collector_network_bytes() {
    let collector = MetricsCollector::new();

    collector.record_network_bytes(1000, 500).await;
    collector.record_network_bytes(2000, 1000).await;

    let network_metrics = collector.get_network_metrics().await;
    assert_eq!(network_metrics.total_bytes_read, 3000);
    assert_eq!(network_metrics.total_bytes_written, 1500);
}

#[tokio::test]
async fn test_metrics_collector_storage_operations() {
    let collector = MetricsCollector::new();

    collector
        .record_storage_operation(StorageOperation::AofWrite(100), Duration::from_millis(5))
        .await;
    collector
        .record_storage_operation(StorageOperation::MemoryWrite, Duration::from_millis(1))
        .await;

    let storage_metrics = collector.get_storage_metrics().await;
    assert!(storage_metrics.total_operations() >= 2);
}

#[tokio::test]
async fn test_metrics_collector_memory_usage() {
    let collector = MetricsCollector::new();

    collector.record_memory_usage(1_000_000).await;

    let perf_metrics = collector.get_performance_metrics().await;
    assert_eq!(perf_metrics.current_memory_usage, 1_000_000);
    assert_eq!(perf_metrics.peak_memory_usage, 1_000_000);
}

#[tokio::test]
async fn test_metrics_collector_summary() {
    let collector = MetricsCollector::new();

    // Record some activity
    collector
        .record_command_latency("SET", Duration::from_millis(10))
        .await;
    collector
        .record_connection_event(ConnectionEvent::Connected)
        .await;
    collector.record_network_bytes(100, 50).await;

    let summary = collector.get_summary().await;

    assert_eq!(summary.total_operations, 1);
    assert_eq!(summary.total_commands, 1);
    assert_eq!(summary.total_bytes_read, 100);
    assert_eq!(summary.total_bytes_written, 50);
    assert_eq!(summary.active_connections, 1);
}

#[tokio::test]
async fn test_metrics_collector_reset() {
    let collector = MetricsCollector::new();

    // Record activity
    collector
        .record_command_latency("SET", Duration::from_millis(10))
        .await;
    collector.record_network_bytes(1000, 500).await;

    // Reset
    collector.reset().await;

    // Verify reset
    let perf_metrics = collector.get_performance_metrics().await;
    assert_eq!(perf_metrics.total_operations, 0);

    let network_metrics = collector.get_network_metrics().await;
    assert_eq!(network_metrics.total_bytes_read, 0);
}

#[tokio::test]
async fn test_metrics_collector_uptime() {
    let collector = MetricsCollector::new();

    // Sleep briefly
    tokio::time::sleep(Duration::from_millis(100)).await;

    let uptime = collector.uptime();
    assert!(uptime >= Duration::from_millis(100));
}

// ==================== Concurrent Metrics Access Tests ====================

#[tokio::test]
async fn test_concurrent_metrics_recording() {
    use std::sync::Arc;
    use tokio::sync::Barrier;

    let collector = Arc::new(MetricsCollector::new());
    let barrier = Arc::new(Barrier::new(10));
    let mut handles = vec![];

    for i in 0..10 {
        let collector = Arc::clone(&collector);
        let barrier = Arc::clone(&barrier);

        handles.push(tokio::spawn(async move {
            barrier.wait().await;

            // Each task records 100 commands
            for j in 0..100 {
                let cmd = if j % 2 == 0 { "SET" } else { "GET" };
                collector
                    .record_command_latency(cmd, Duration::from_micros(j as u64))
                    .await;
            }

            i
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Should have 10 tasks * 100 commands = 1000 total
    let perf_metrics = collector.get_performance_metrics().await;
    assert_eq!(perf_metrics.total_operations, 1000);

    let cmd_metrics = collector.get_command_metrics().await;
    assert_eq!(cmd_metrics.total_commands(), 1000);
    assert_eq!(*cmd_metrics.command_counts.get("SET").unwrap(), 500);
    assert_eq!(*cmd_metrics.command_counts.get("GET").unwrap(), 500);
}

// ==================== Latency Histogram Accuracy Tests ====================

#[test]
fn test_histogram_accuracy_uniform_distribution() {
    let mut histogram = Histogram::new();

    // Add values at each bucket boundary
    for i in 0..100 {
        histogram.record(Duration::from_micros(10 * i));
    }

    assert_eq!(histogram.count(), 100);

    // Percentiles should be monotonically increasing
    let p50 = histogram.percentile(50.0);
    let p75 = histogram.percentile(75.0);
    let p95 = histogram.percentile(95.0);

    assert!(p50 <= p75, "P50 should be <= P75");
    assert!(p75 <= p95, "P75 should be <= P95");
}

#[test]
fn test_histogram_edge_cases() {
    let mut histogram = Histogram::new();

    // Add very small value
    histogram.record(Duration::from_nanos(1));
    assert_eq!(histogram.count(), 1);

    // Add very large value
    histogram.record(Duration::from_secs(10));
    assert_eq!(histogram.count(), 2);

    // Percentiles should still work
    assert!(histogram.percentile(50.0).is_some());
    assert!(histogram.percentile(99.0).is_some());
}
