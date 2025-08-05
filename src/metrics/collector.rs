//! Metrics collector for gathering performance and operational metrics
//!
//! This module provides a centralized metrics collection system that tracks
//! various aspects of RustyPotato performance including command latencies,
//! network throughput, storage operations, and system health.

use crate::metrics::Histogram;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::debug;

/// Main metrics collector that aggregates all system metrics
#[derive(Debug)]
pub struct MetricsCollector {
    performance: Arc<RwLock<PerformanceMetrics>>,
    commands: Arc<RwLock<CommandMetrics>>,
    network: Arc<RwLock<NetworkMetrics>>,
    storage: Arc<RwLock<StorageMetrics>>,
    start_time: Instant,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new() -> Self {
        Self {
            performance: Arc::new(RwLock::new(PerformanceMetrics::new())),
            commands: Arc::new(RwLock::new(CommandMetrics::new())),
            network: Arc::new(RwLock::new(NetworkMetrics::new())),
            storage: Arc::new(RwLock::new(StorageMetrics::new())),
            start_time: Instant::now(),
        }
    }

    /// Record command execution time
    pub async fn record_command_latency(&self, command: &str, duration: Duration) {
        let mut commands = self.commands.write().await;
        commands.record_command(command, duration);

        let mut performance = self.performance.write().await;
        performance.record_operation_latency(duration);

        debug!("Recorded command '{}' latency: {:?}", command, duration);
    }

    /// Record network bytes transferred
    pub async fn record_network_bytes(&self, bytes_read: u64, bytes_written: u64) {
        let mut network = self.network.write().await;
        network.add_bytes_read(bytes_read);
        network.add_bytes_written(bytes_written);
    }

    /// Record connection event
    pub async fn record_connection_event(&self, event: ConnectionEvent) {
        let mut network = self.network.write().await;
        match event {
            ConnectionEvent::Connected => network.increment_connections_accepted(),
            ConnectionEvent::Disconnected => network.increment_connections_closed(),
            ConnectionEvent::Rejected => network.increment_connections_rejected(),
        }
    }

    /// Record storage operation
    pub async fn record_storage_operation(&self, operation: StorageOperation, duration: Duration) {
        let mut storage = self.storage.write().await;
        storage.record_operation(operation, duration);
    }

    /// Record memory usage
    pub async fn record_memory_usage(&self, bytes: u64) {
        let mut performance = self.performance.write().await;
        performance.update_memory_usage(bytes);
    }

    /// Get current performance metrics
    pub async fn get_performance_metrics(&self) -> PerformanceMetrics {
        self.performance.read().await.clone()
    }

    /// Get current command metrics
    pub async fn get_command_metrics(&self) -> CommandMetrics {
        self.commands.read().await.clone()
    }

    /// Get current network metrics
    pub async fn get_network_metrics(&self) -> NetworkMetrics {
        self.network.read().await.clone()
    }

    /// Get current storage metrics
    pub async fn get_storage_metrics(&self) -> StorageMetrics {
        self.storage.read().await.clone()
    }

    /// Get uptime
    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Reset all metrics
    pub async fn reset(&self) {
        let mut performance = self.performance.write().await;
        performance.reset();

        let mut commands = self.commands.write().await;
        commands.reset();

        let mut network = self.network.write().await;
        network.reset();

        let mut storage = self.storage.write().await;
        storage.reset();

        debug!("All metrics reset");
    }

    /// Get summary statistics
    pub async fn get_summary(&self) -> MetricsSummary {
        let performance = self.get_performance_metrics().await;
        let commands = self.get_command_metrics().await;
        let network = self.get_network_metrics().await;
        let storage = self.get_storage_metrics().await;

        MetricsSummary {
            uptime: self.uptime(),
            total_operations: performance.total_operations,
            operations_per_second: performance.operations_per_second(),
            average_latency: performance.latency_histogram.average(),
            p95_latency: performance.latency_histogram.percentile(95.0),
            p99_latency: performance.latency_histogram.percentile(99.0),
            total_commands: commands.total_commands(),
            most_frequent_command: commands.most_frequent_command(),
            total_bytes_read: network.total_bytes_read,
            total_bytes_written: network.total_bytes_written,
            active_connections: network.active_connections,
            memory_usage_bytes: performance.current_memory_usage,
            storage_operations: storage.total_operations(),
        }
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Performance metrics tracking latency and throughput
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub latency_histogram: Histogram,
    pub total_operations: u64,
    pub current_memory_usage: u64,
    pub peak_memory_usage: u64,
    pub start_time: Instant,
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self {
            latency_histogram: Histogram::new(),
            total_operations: 0,
            current_memory_usage: 0,
            peak_memory_usage: 0,
            start_time: Instant::now(),
        }
    }

    pub fn record_operation_latency(&mut self, duration: Duration) {
        self.latency_histogram.record(duration);
        self.total_operations += 1;
    }

    pub fn update_memory_usage(&mut self, bytes: u64) {
        self.current_memory_usage = bytes;
        if bytes > self.peak_memory_usage {
            self.peak_memory_usage = bytes;
        }
    }

    pub fn operations_per_second(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.total_operations as f64 / elapsed
        } else {
            0.0
        }
    }

    pub fn reset(&mut self) {
        self.latency_histogram.reset();
        self.total_operations = 0;
        self.current_memory_usage = 0;
        self.peak_memory_usage = 0;
        self.start_time = Instant::now();
    }
}

/// Command-specific metrics tracking
#[derive(Debug, Clone)]
pub struct CommandMetrics {
    pub command_counts: HashMap<String, u64>,
    pub command_latencies: HashMap<String, Histogram>,
    pub command_errors: HashMap<String, u64>,
}

impl CommandMetrics {
    pub fn new() -> Self {
        Self {
            command_counts: HashMap::new(),
            command_latencies: HashMap::new(),
            command_errors: HashMap::new(),
        }
    }

    pub fn record_command(&mut self, command: &str, duration: Duration) {
        *self.command_counts.entry(command.to_string()).or_insert(0) += 1;
        self.command_latencies
            .entry(command.to_string())
            .or_insert_with(Histogram::new)
            .record(duration);
    }

    pub fn record_command_error(&mut self, command: &str) {
        *self.command_errors.entry(command.to_string()).or_insert(0) += 1;
    }

    pub fn total_commands(&self) -> u64 {
        self.command_counts.values().sum()
    }

    pub fn most_frequent_command(&self) -> Option<(String, u64)> {
        self.command_counts
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(cmd, count)| (cmd.clone(), *count))
    }

    pub fn command_error_rate(&self, command: &str) -> f64 {
        let errors = self.command_errors.get(command).unwrap_or(&0);
        let total = self.command_counts.get(command).unwrap_or(&0);

        if *total > 0 {
            *errors as f64 / *total as f64
        } else {
            0.0
        }
    }

    pub fn reset(&mut self) {
        self.command_counts.clear();
        self.command_latencies.clear();
        self.command_errors.clear();
    }
}

/// Network metrics tracking connections and data transfer
#[derive(Debug, Clone)]
pub struct NetworkMetrics {
    pub total_bytes_read: u64,
    pub total_bytes_written: u64,
    pub connections_accepted: u64,
    pub connections_rejected: u64,
    pub connections_closed: u64,
    pub active_connections: u64,
}

impl NetworkMetrics {
    pub fn new() -> Self {
        Self {
            total_bytes_read: 0,
            total_bytes_written: 0,
            connections_accepted: 0,
            connections_rejected: 0,
            connections_closed: 0,
            active_connections: 0,
        }
    }

    pub fn add_bytes_read(&mut self, bytes: u64) {
        self.total_bytes_read += bytes;
    }

    pub fn add_bytes_written(&mut self, bytes: u64) {
        self.total_bytes_written += bytes;
    }

    pub fn increment_connections_accepted(&mut self) {
        self.connections_accepted += 1;
        self.active_connections += 1;
    }

    pub fn increment_connections_rejected(&mut self) {
        self.connections_rejected += 1;
    }

    pub fn increment_connections_closed(&mut self) {
        self.connections_closed += 1;
        if self.active_connections > 0 {
            self.active_connections -= 1;
        }
    }

    pub fn throughput_mbps(&self, duration: Duration) -> f64 {
        let total_bytes = self.total_bytes_read + self.total_bytes_written;
        let seconds = duration.as_secs_f64();
        if seconds > 0.0 {
            (total_bytes as f64 * 8.0) / (seconds * 1_000_000.0) // Convert to Mbps
        } else {
            0.0
        }
    }

    pub fn reset(&mut self) {
        self.total_bytes_read = 0;
        self.total_bytes_written = 0;
        self.connections_accepted = 0;
        self.connections_rejected = 0;
        self.connections_closed = 0;
        // Don't reset active_connections as it represents current state
    }
}

/// Storage metrics tracking persistence operations
#[derive(Debug, Clone)]
pub struct StorageMetrics {
    pub aof_writes: u64,
    pub aof_write_latency: Histogram,
    pub aof_bytes_written: u64,
    pub memory_operations: u64,
    pub memory_operation_latency: Histogram,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

impl StorageMetrics {
    pub fn new() -> Self {
        Self {
            aof_writes: 0,
            aof_write_latency: Histogram::new(),
            aof_bytes_written: 0,
            memory_operations: 0,
            memory_operation_latency: Histogram::new(),
            cache_hits: 0,
            cache_misses: 0,
        }
    }

    pub fn record_operation(&mut self, operation: StorageOperation, duration: Duration) {
        match operation {
            StorageOperation::AofWrite(bytes) => {
                self.aof_writes += 1;
                self.aof_write_latency.record(duration);
                self.aof_bytes_written += bytes;
            }
            StorageOperation::MemoryRead | StorageOperation::MemoryWrite => {
                self.memory_operations += 1;
                self.memory_operation_latency.record(duration);
            }
            StorageOperation::CacheHit => {
                self.cache_hits += 1;
            }
            StorageOperation::CacheMiss => {
                self.cache_misses += 1;
            }
        }
    }

    pub fn total_operations(&self) -> u64 {
        self.aof_writes + self.memory_operations + self.cache_hits + self.cache_misses
    }

    pub fn cache_hit_rate(&self) -> f64 {
        let total_cache_ops = self.cache_hits + self.cache_misses;
        if total_cache_ops > 0 {
            self.cache_hits as f64 / total_cache_ops as f64
        } else {
            0.0
        }
    }

    pub fn reset(&mut self) {
        self.aof_writes = 0;
        self.aof_write_latency.reset();
        self.aof_bytes_written = 0;
        self.memory_operations = 0;
        self.memory_operation_latency.reset();
        self.cache_hits = 0;
        self.cache_misses = 0;
    }
}

/// Connection events for metrics tracking
#[derive(Debug, Clone, Copy)]
pub enum ConnectionEvent {
    Connected,
    Disconnected,
    Rejected,
}

/// Storage operations for metrics tracking
#[derive(Debug, Clone, Copy)]
pub enum StorageOperation {
    AofWrite(u64), // bytes written
    MemoryRead,
    MemoryWrite,
    CacheHit,
    CacheMiss,
}

/// Summary of all metrics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MetricsSummary {
    pub uptime: Duration,
    pub total_operations: u64,
    pub operations_per_second: f64,
    pub average_latency: Option<Duration>,
    pub p95_latency: Option<Duration>,
    pub p99_latency: Option<Duration>,
    pub total_commands: u64,
    pub most_frequent_command: Option<(String, u64)>,
    pub total_bytes_read: u64,
    pub total_bytes_written: u64,
    pub active_connections: u64,
    pub memory_usage_bytes: u64,
    pub storage_operations: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_metrics_collector_creation() {
        let collector = MetricsCollector::new();
        assert!(collector.uptime() < Duration::from_millis(100));

        let summary = collector.get_summary().await;
        assert_eq!(summary.total_operations, 0);
        assert_eq!(summary.total_commands, 0);
    }

    #[tokio::test]
    async fn test_command_metrics() {
        let collector = MetricsCollector::new();

        collector
            .record_command_latency("SET", Duration::from_micros(100))
            .await;
        collector
            .record_command_latency("GET", Duration::from_micros(50))
            .await;
        collector
            .record_command_latency("SET", Duration::from_micros(120))
            .await;

        let metrics = collector.get_command_metrics().await;
        assert_eq!(metrics.total_commands(), 3);
        assert_eq!(metrics.command_counts.get("SET"), Some(&2));
        assert_eq!(metrics.command_counts.get("GET"), Some(&1));

        let most_frequent = metrics.most_frequent_command();
        assert_eq!(most_frequent, Some(("SET".to_string(), 2)));
    }

    #[tokio::test]
    async fn test_network_metrics() {
        let collector = MetricsCollector::new();

        collector.record_network_bytes(1024, 512).await;
        collector
            .record_connection_event(ConnectionEvent::Connected)
            .await;
        collector
            .record_connection_event(ConnectionEvent::Connected)
            .await;
        collector
            .record_connection_event(ConnectionEvent::Rejected)
            .await;

        let metrics = collector.get_network_metrics().await;
        assert_eq!(metrics.total_bytes_read, 1024);
        assert_eq!(metrics.total_bytes_written, 512);
        assert_eq!(metrics.connections_accepted, 2);
        assert_eq!(metrics.connections_rejected, 1);
        assert_eq!(metrics.active_connections, 2);
    }

    #[tokio::test]
    async fn test_storage_metrics() {
        let collector = MetricsCollector::new();

        collector
            .record_storage_operation(StorageOperation::AofWrite(256), Duration::from_micros(200))
            .await;
        collector
            .record_storage_operation(StorageOperation::MemoryRead, Duration::from_micros(10))
            .await;
        collector
            .record_storage_operation(StorageOperation::CacheHit, Duration::ZERO)
            .await;
        collector
            .record_storage_operation(StorageOperation::CacheMiss, Duration::ZERO)
            .await;

        let metrics = collector.get_storage_metrics().await;
        assert_eq!(metrics.aof_writes, 1);
        assert_eq!(metrics.aof_bytes_written, 256);
        assert_eq!(metrics.memory_operations, 1);
        assert_eq!(metrics.cache_hits, 1);
        assert_eq!(metrics.cache_misses, 1);
        assert_eq!(metrics.cache_hit_rate(), 0.5);
        assert_eq!(metrics.total_operations(), 4);
    }

    #[tokio::test]
    async fn test_performance_metrics() {
        let collector = MetricsCollector::new();

        collector
            .record_command_latency("TEST", Duration::from_micros(100))
            .await;
        collector
            .record_command_latency("TEST", Duration::from_micros(200))
            .await;
        collector.record_memory_usage(1024 * 1024).await;

        let metrics = collector.get_performance_metrics().await;
        assert_eq!(metrics.total_operations, 2);
        assert_eq!(metrics.current_memory_usage, 1024 * 1024);
        assert_eq!(metrics.peak_memory_usage, 1024 * 1024);
        assert!(metrics.operations_per_second() > 0.0);
    }

    #[tokio::test]
    async fn test_metrics_reset() {
        let collector = MetricsCollector::new();

        collector
            .record_command_latency("SET", Duration::from_micros(100))
            .await;
        collector.record_network_bytes(1024, 512).await;

        let summary_before = collector.get_summary().await;
        assert!(summary_before.total_operations > 0);

        collector.reset().await;

        let summary_after = collector.get_summary().await;
        assert_eq!(summary_after.total_operations, 0);
        assert_eq!(summary_after.total_commands, 0);
        assert_eq!(summary_after.total_bytes_read, 0);
    }

    #[test]
    fn test_command_metrics_error_rate() {
        let mut metrics = CommandMetrics::new();

        metrics.record_command("SET", Duration::from_micros(100));
        metrics.record_command("SET", Duration::from_micros(120));
        metrics.record_command_error("SET");

        assert_eq!(metrics.command_error_rate("SET"), 0.5); // 1 error out of 2 commands
        assert_eq!(metrics.command_error_rate("GET"), 0.0); // No GET commands
    }

    #[test]
    fn test_network_metrics_throughput() {
        let mut metrics = NetworkMetrics::new();

        metrics.add_bytes_read(1024 * 1024); // 1MB
        metrics.add_bytes_written(512 * 1024); // 512KB

        let throughput = metrics.throughput_mbps(Duration::from_secs(1));
        assert!(throughput > 10.0 && throughput < 15.0); // ~12 Mbps with some tolerance
    }
}
