//! RustyPotato - A high-performance Redis-compatible key-value store
//!
//! RustyPotato is built in Rust with a focus on performance, memory safety,
//! and concurrency. It provides Redis-like functionality with additional
//! unique features.

// Core modules
pub mod config;
pub mod error;
pub mod logging;

// Feature modules
pub mod cli;
pub mod commands;
pub mod metrics;
pub mod monitoring;
pub mod network;
pub mod storage;

// Public API exports
pub use config::Config;
pub use error::{Result, RustyPotatoError};

// Test utilities (only available in test builds)
#[cfg(test)]
pub mod test_utils;

// Re-export commonly used types
pub use commands::{CommandRegistry, CommandResult, ResponseValue};
pub use metrics::{MetricsCollector, MetricsServer};
pub use monitoring::{HealthChecker, HealthStatus, LogRotationManager, MonitoringServer};
pub use network::TcpServer;
pub use storage::{MemoryStore, StoredValue, ValueType};

use commands::{
    DecrCommand, DelCommand, ExistsCommand, ExpireCommand, GetCommand, IncrCommand, SetCommand,
    TtlCommand,
};
use std::sync::Arc;

/// RustyPotato server instance with fully integrated components
pub struct RustyPotatoServer {
    config: Arc<Config>,
    storage: Arc<MemoryStore>,
    command_registry: Arc<CommandRegistry>,
    metrics: Arc<MetricsCollector>,
    tcp_server: Option<TcpServer>,
}

impl RustyPotatoServer {
    /// Create a new RustyPotato server with the given configuration
    pub fn new(config: Config) -> Result<Self> {
        let config = Arc::new(config);
        let storage = Arc::new(MemoryStore::new());
        let metrics = Arc::new(MetricsCollector::new());

        Self::with_components_internal(config, storage, metrics)
    }

    /// Create a new RustyPotato server with shared components
    pub fn with_components(
        config: Config,
        storage: Arc<MemoryStore>,
        metrics: Arc<MetricsCollector>,
    ) -> Result<Self> {
        let config = Arc::new(config);
        Self::with_components_internal(config, storage, metrics)
    }

    /// Internal method to create server with components
    fn with_components_internal(
        config: Arc<Config>,
        storage: Arc<MemoryStore>,
        metrics: Arc<MetricsCollector>,
    ) -> Result<Self> {
        // Create and populate command registry with all available commands
        let mut command_registry = CommandRegistry::new();

        // Register basic string commands
        command_registry.register(Box::new(SetCommand));
        command_registry.register(Box::new(GetCommand));
        command_registry.register(Box::new(DelCommand));
        command_registry.register(Box::new(ExistsCommand));

        // Register TTL commands
        command_registry.register(Box::new(ExpireCommand));
        command_registry.register(Box::new(TtlCommand));

        // Register atomic integer commands
        command_registry.register(Box::new(IncrCommand));
        command_registry.register(Box::new(DecrCommand));

        let command_registry = Arc::new(command_registry);

        // Create TCP server with all components wired together
        let tcp_server = TcpServer::with_metrics(
            Arc::clone(&config),
            Arc::clone(&storage),
            Arc::clone(&command_registry),
            Arc::clone(&metrics),
        );

        Ok(Self {
            config,
            storage,
            command_registry,
            metrics,
            tcp_server: Some(tcp_server),
        })
    }

    /// Start the server and begin accepting connections
    pub async fn start(&mut self) -> Result<()> {
        if let Some(mut tcp_server) = self.tcp_server.take() {
            tracing::info!(
                "Starting RustyPotato server with {} registered commands",
                self.command_registry.command_count()
            );

            tcp_server.start().await
        } else {
            Err(RustyPotatoError::InternalError {
                message: "Server already started or not properly initialized".to_string(),
                component: Some("server".to_string()),
                source: None,
            })
        }
    }

    /// Start the server and return the listening address (useful for testing)
    pub async fn start_with_addr(&mut self) -> Result<std::net::SocketAddr> {
        if let Some(mut tcp_server) = self.tcp_server.take() {
            tracing::info!(
                "Starting RustyPotato server with {} registered commands",
                self.command_registry.command_count()
            );

            tcp_server.start_with_addr().await
        } else {
            Err(RustyPotatoError::InternalError {
                message: "Server already started or not properly initialized".to_string(),
                component: Some("server".to_string()),
                source: None,
            })
        }
    }

    /// Shutdown the server gracefully
    pub async fn shutdown(&self) -> Result<()> {
        // Note: Since we moved tcp_server in start(), we can't access it here
        // In a production system, we'd keep a reference to the server for shutdown
        // For now, this is a placeholder
        tracing::info!("Server shutdown requested");
        Ok(())
    }

    /// Get the server configuration
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Get a reference to the storage layer
    pub fn storage(&self) -> &MemoryStore {
        &self.storage
    }

    /// Get a reference to the command registry
    pub fn command_registry(&self) -> &CommandRegistry {
        &self.command_registry
    }

    /// Get a reference to the metrics collector
    pub fn metrics(&self) -> &MetricsCollector {
        &self.metrics
    }

    /// Get server statistics
    pub async fn stats(&self) -> ServerStats {
        ServerStats {
            registered_commands: self.command_registry.command_count(),
            command_names: self.command_registry.command_names(),
            config_summary: format!(
                "{}:{}",
                self.config.server.bind_address, self.config.server.port
            ),
        }
    }
}

/// Server statistics for monitoring
#[derive(Debug, Clone)]
pub struct ServerStats {
    pub registered_commands: usize,
    pub command_names: Vec<String>,
    pub config_summary: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let config = Config::default();
        let server = RustyPotatoServer::new(config);
        assert!(server.is_ok());
    }

    #[test]
    fn test_server_config_access() {
        let config = Config::default();
        let server = RustyPotatoServer::new(config).unwrap();
        assert_eq!(server.config().server.port, 6379);
    }

    #[tokio::test]
    async fn test_server_stats() {
        let config = Config::default();
        let server = RustyPotatoServer::new(config).unwrap();
        let stats = server.stats().await;

        assert_eq!(stats.registered_commands, 8); // SET, GET, DEL, EXISTS, EXPIRE, TTL, INCR, DECR
        assert!(stats.command_names.contains(&"SET".to_string()));
        assert!(stats.command_names.contains(&"GET".to_string()));
        assert!(stats.command_names.contains(&"INCR".to_string()));
        assert_eq!(stats.config_summary, "127.0.0.1:6379");
    }

    #[test]
    fn test_server_component_access() {
        let config = Config::default();
        let server = RustyPotatoServer::new(config).unwrap();

        // Test that we can access the storage and command registry
        assert_eq!(server.command_registry().command_count(), 8);
        assert!(server.command_registry().has_command("SET"));
        assert!(server.command_registry().has_command("GET"));
    }

    #[tokio::test]
    async fn test_server_start_with_addr() {
        let mut config = Config::default();
        config.server.port = 0; // Use random port

        let mut server = RustyPotatoServer::new(config).unwrap();
        let addr = server.start_with_addr().await.unwrap();

        // Should get a valid address with a port assigned
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        assert!(addr.port() > 0);
    }
}
