//! Test configuration and utilities
//!
//! This module provides common configuration and utility functions
//! used across all test categories.

use rustypotato::Config;

use tempfile::TempDir;

/// Create a test configuration with safe defaults
pub fn create_test_config() -> Config {
    let mut config = Config::default();
    config.server.port = 0; // Use random port for testing
    config.server.max_connections = 1000;
    config.storage.aof_enabled = false; // Disable by default for faster tests
    config.network.connection_timeout = 5;
    config.network.read_timeout = 5;
    config.network.write_timeout = 5;
    config
}

/// Create a test configuration with persistence enabled
pub fn create_persistent_test_config() -> (Config, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let aof_path = temp_dir.path().join("test.aof");

    let mut config = create_test_config();
    config.storage.aof_enabled = true;
    config.storage.aof_path = aof_path;
    config.storage.aof_fsync_policy = rustypotato::config::FsyncPolicy::Always; // Immediate sync for testing

    (config, temp_dir)
}

/// Test timeouts
pub mod timeouts {
    use std::time::Duration;

    pub const SHORT: Duration = Duration::from_millis(100);
    pub const MEDIUM: Duration = Duration::from_secs(1);
    pub const LONG: Duration = Duration::from_secs(5);
    pub const VERY_LONG: Duration = Duration::from_secs(30);
}

/// Test data generators
pub mod generators {
    use rustypotato::ValueType;

    pub fn test_key(prefix: &str, id: usize) -> String {
        format!("{prefix}_{id:06}")
    }

    pub fn test_value(prefix: &str, id: usize) -> String {
        format!("{prefix}_{id:06}")
    }

    pub fn large_value(size: usize) -> String {
        "x".repeat(size)
    }

    pub fn unicode_value() -> String {
        "🚀 Unicode test value 🎯 with émojis and spëcial chars".to_string()
    }

    pub fn test_integer_value(base: i64, offset: usize) -> ValueType {
        ValueType::Integer(base + offset as i64)
    }

    pub fn test_string_value(prefix: &str, id: usize) -> ValueType {
        ValueType::String(test_value(prefix, id))
    }
}

/// Test assertions and helpers
pub mod assertions {
    use std::time::Duration;

    pub fn assert_duration_within(actual: Duration, expected: Duration, tolerance: Duration) {
        let diff = actual.abs_diff(expected);

        assert!(
            diff <= tolerance,
            "Duration {actual:?} not within {tolerance:?} of expected {expected:?}"
        );
    }

    pub fn assert_performance_threshold(ops_per_sec: u64, min_threshold: u64) {
        assert!(
            ops_per_sec >= min_threshold,
            "Performance below threshold: {ops_per_sec} ops/sec < {min_threshold} ops/sec"
        );
    }
}

/// Test environment setup
pub mod setup {
    use super::*;
    use rustypotato::RustyPotatoServer;
    use std::net::SocketAddr;
    use tokio::time::{sleep, Duration};

    pub async fn start_test_server() -> (RustyPotatoServer, SocketAddr) {
        let config = create_test_config();
        let mut server = RustyPotatoServer::new(config).expect("Failed to create test server");
        let addr = server
            .start_with_addr()
            .await
            .expect("Failed to start test server");

        // Give server time to start
        sleep(Duration::from_millis(50)).await;

        (server, addr)
    }

    pub async fn start_persistent_test_server() -> (RustyPotatoServer, SocketAddr, TempDir) {
        let (config, temp_dir) = create_persistent_test_config();
        let mut server =
            RustyPotatoServer::new(config).expect("Failed to create persistent test server");
        let addr = server
            .start_with_addr()
            .await
            .expect("Failed to start persistent test server");

        // Give server time to start
        sleep(Duration::from_millis(100)).await;

        (server, addr, temp_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_creation() {
        let config = create_test_config();
        assert_eq!(config.server.port, 0);
        assert!(!config.storage.aof_enabled);
        assert_eq!(config.server.max_connections, 1000);
    }

    #[test]
    fn test_persistent_config_creation() {
        let (config, _temp_dir) = create_persistent_test_config();
        assert!(config.storage.aof_enabled);
        assert!(matches!(
            config.storage.aof_fsync_policy,
            rustypotato::config::FsyncPolicy::Always
        ));
    }

    #[test]
    fn test_generators() {
        assert_eq!(generators::test_key("test", 42), "test_000042");
        assert_eq!(generators::test_value("val", 123), "val_000123");
        assert_eq!(generators::large_value(5), "xxxxx");
        assert!(!generators::unicode_value().is_empty());
    }

    #[test]
    fn test_assertions() {
        use std::time::Duration;

        // Should not panic
        assertions::assert_duration_within(
            Duration::from_millis(100),
            Duration::from_millis(95),
            Duration::from_millis(10),
        );

        // Should not panic
        assertions::assert_performance_threshold(1000, 500);
    }

    #[tokio::test]
    async fn test_server_setup() {
        let (_server, addr) = setup::start_test_server().await;
        assert!(addr.port() > 0);
    }
}
