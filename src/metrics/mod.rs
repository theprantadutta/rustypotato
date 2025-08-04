//! Metrics collection and monitoring for RustyPotato
//! 
//! This module provides comprehensive metrics collection for monitoring
//! performance, throughput, latency, and system health.

pub mod collector;
pub mod server;

pub use collector::{
    MetricsCollector, PerformanceMetrics, CommandMetrics, NetworkMetrics, StorageMetrics,
    ConnectionEvent, StorageOperation, MetricsSummary
};
pub use server::MetricsServer;

use std::time::{Duration, Instant};

/// Histogram for tracking latency distributions
#[derive(Debug, Clone)]
pub struct Histogram {
    buckets: Vec<(Duration, u64)>,
    total_count: u64,
    total_sum: Duration,
}

impl Histogram {
    /// Create a new histogram with predefined buckets
    pub fn new() -> Self {
        let buckets = vec![
            (Duration::from_micros(10), 0),    // 10μs
            (Duration::from_micros(50), 0),    // 50μs
            (Duration::from_micros(100), 0),   // 100μs
            (Duration::from_micros(500), 0),   // 500μs
            (Duration::from_millis(1), 0),     // 1ms
            (Duration::from_millis(5), 0),     // 5ms
            (Duration::from_millis(10), 0),    // 10ms
            (Duration::from_millis(50), 0),    // 50ms
            (Duration::from_millis(100), 0),   // 100ms
            (Duration::from_millis(500), 0),   // 500ms
            (Duration::from_secs(1), 0),       // 1s
            (Duration::from_secs(5), 0),       // 5s
        ];
        
        Self {
            buckets,
            total_count: 0,
            total_sum: Duration::ZERO,
        }
    }

    /// Record a value in the histogram
    pub fn record(&mut self, value: Duration) {
        self.total_count += 1;
        self.total_sum += value;

        for (bucket_limit, count) in &mut self.buckets {
            if value <= *bucket_limit {
                *count += 1;
            }
        }
    }

    /// Get the percentile value
    pub fn percentile(&self, p: f64) -> Option<Duration> {
        if self.total_count == 0 {
            return None;
        }

        let target_count = (self.total_count as f64 * p / 100.0) as u64;
        let mut cumulative = 0;

        for (bucket_limit, count) in &self.buckets {
            cumulative += count;
            if cumulative >= target_count {
                return Some(*bucket_limit);
            }
        }

        self.buckets.last().map(|(limit, _)| *limit)
    }

    /// Get the average value
    pub fn average(&self) -> Option<Duration> {
        if self.total_count == 0 {
            None
        } else {
            Some(self.total_sum / self.total_count as u32)
        }
    }

    /// Get total count
    pub fn count(&self) -> u64 {
        self.total_count
    }

    /// Get total sum
    pub fn sum(&self) -> Duration {
        self.total_sum
    }

    /// Reset the histogram
    pub fn reset(&mut self) {
        for (_, count) in &mut self.buckets {
            *count = 0;
        }
        self.total_count = 0;
        self.total_sum = Duration::ZERO;
    }
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}

/// Timer for measuring operation duration
pub struct Timer {
    start: Instant,
}

impl Timer {
    /// Start a new timer
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Stop the timer and return the elapsed duration
    pub fn stop(self) -> Duration {
        self.start.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_histogram_creation() {
        let histogram = Histogram::new();
        assert_eq!(histogram.count(), 0);
        assert_eq!(histogram.sum(), Duration::ZERO);
        assert!(histogram.average().is_none());
        assert!(histogram.percentile(50.0).is_none());
    }

    #[test]
    fn test_histogram_record() {
        let mut histogram = Histogram::new();
        
        histogram.record(Duration::from_micros(25));
        histogram.record(Duration::from_micros(75));
        histogram.record(Duration::from_micros(150));
        
        assert_eq!(histogram.count(), 3);
        assert_eq!(histogram.sum(), Duration::from_micros(250));
        let avg = histogram.average().unwrap();
        assert!(avg >= Duration::from_micros(80) && avg <= Duration::from_micros(85));
    }

    #[test]
    fn test_histogram_percentiles() {
        let mut histogram = Histogram::new();
        
        // Add 100 values from 1μs to 100μs
        for i in 1..=100 {
            histogram.record(Duration::from_micros(i));
        }
        
        assert_eq!(histogram.count(), 100);
        
        // Test percentiles
        assert!(histogram.percentile(50.0).is_some());
        assert!(histogram.percentile(95.0).is_some());
        assert!(histogram.percentile(99.0).is_some());
    }

    #[test]
    fn test_histogram_reset() {
        let mut histogram = Histogram::new();
        
        histogram.record(Duration::from_micros(100));
        assert_eq!(histogram.count(), 1);
        
        histogram.reset();
        assert_eq!(histogram.count(), 0);
        assert_eq!(histogram.sum(), Duration::ZERO);
    }

    #[test]
    fn test_timer() {
        let timer = Timer::start();
        std::thread::sleep(Duration::from_millis(1));
        let elapsed = timer.stop();
        
        assert!(elapsed >= Duration::from_millis(1));
        assert!(elapsed < Duration::from_millis(100)); // Should be much less than 100ms
    }
}