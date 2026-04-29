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
pub use storage::{replay_aof_file, MemoryStore, PersistenceManager, StoredValue, ValueType};

use crate::network::ConnectionPool;
use commands::{
    BgrewriteaofCommand, CommandCommand, DbsizeCommand, DecrCommand, DelCommand, EchoCommand,
    ExistsCommand, ExpireCommand, ExpireatCommand, FlushdbCommand, GetCommand, HdelCommand,
    HexistsCommand, HgetCommand, HgetallCommand, HkeysCommand, HlenCommand, HmgetCommand,
    HsetCommand, HvalsCommand, IncrCommand, InfoCommand, KeysCommand, MgetCommand, MsetCommand,
    PexpireCommand, PexpireatCommand, PingCommand, PttlCommand, QuitCommand, SetCommand,
    TtlCommand, TypeCommand,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

/// RustyPotato server instance with fully integrated components
pub struct RustyPotatoServer {
    config: Arc<Config>,
    storage: Arc<MemoryStore>,
    command_registry: Arc<CommandRegistry>,
    metrics: Arc<MetricsCollector>,
    persistence: Option<Arc<PersistenceManager>>,
    /// Slot wired into the `BgrewriteaofCommand` instance in the
    /// registry so it can find the persistence manager once
    /// `with_persistence` is called.
    bgrewrite_persistence_slot: Arc<std::sync::OnceLock<Arc<PersistenceManager>>>,
    /// Captured from `TcpServer` at construction time. Stays accessible
    /// even after `start()` has consumed the inner `TcpServer`, so
    /// `shutdown()` can signal the running accept loop.
    shutdown_tx: broadcast::Sender<()>,
    /// Same lifecycle as `shutdown_tx`. `shutdown()` polls this to wait
    /// for in-flight connections to drain.
    connection_pool: Arc<ConnectionPool>,
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
        command_registry.register(Box::new(PttlCommand));
        command_registry.register(Box::new(PexpireCommand));
        command_registry.register(Box::new(ExpireatCommand));
        command_registry.register(Box::new(PexpireatCommand));

        // Register atomic integer commands
        command_registry.register(Box::new(IncrCommand));
        command_registry.register(Box::new(DecrCommand));

        // Register hash commands
        command_registry.register(Box::new(HsetCommand));
        command_registry.register(Box::new(HgetCommand));
        command_registry.register(Box::new(HdelCommand));
        command_registry.register(Box::new(HgetallCommand));
        command_registry.register(Box::new(HexistsCommand));
        command_registry.register(Box::new(HmgetCommand));
        command_registry.register(Box::new(HkeysCommand));
        command_registry.register(Box::new(HvalsCommand));
        command_registry.register(Box::new(HlenCommand));

        // Register server-level commands (PING/ECHO/etc.)
        command_registry.register(Box::new(PingCommand));
        command_registry.register(Box::new(EchoCommand));
        command_registry.register(Box::new(DbsizeCommand));
        command_registry.register(Box::new(TypeCommand));
        command_registry.register(Box::new(FlushdbCommand));
        command_registry.register(Box::new(InfoCommand));
        command_registry.register(Box::new(KeysCommand));

        // Register batch string ops (Stage 7 partial 5)
        command_registry.register(Box::new(MgetCommand));
        command_registry.register(Box::new(MsetCommand));

        // QUIT closes the connection cleanly. Registered before
        // COMMAND so COMMAND COUNT sees a stable total.
        command_registry.register(Box::new(QuitCommand));

        // BGREWRITEAOF coordinates with mutating dispatch via a shared
        // RwLock — readers (mutations) hold it as a read guard, the
        // command itself holds it as a write guard. The persistence
        // handle is wired up later by `with_persistence`.
        let rewrite_barrier: crate::network::server::RewriteBarrier =
            Arc::new(tokio::sync::RwLock::new(()));
        let bgrewrite_persistence_slot: Arc<std::sync::OnceLock<Arc<PersistenceManager>>> =
            Arc::new(std::sync::OnceLock::new());
        command_registry.register(Box::new(BgrewriteaofCommand::new(
            Arc::clone(&bgrewrite_persistence_slot),
            Arc::clone(&rewrite_barrier),
        )));

        // COMMAND must go last — it carries the total count, which
        // includes itself.
        let total_with_command = command_registry.command_count() + 1;
        command_registry.register(Box::new(CommandCommand::new(total_with_command)));

        let command_registry = Arc::new(command_registry);

        let tcp_server = TcpServer::with_metrics(
            Arc::clone(&config),
            Arc::clone(&storage),
            Arc::clone(&command_registry),
            Arc::clone(&metrics),
        )
        .with_rewrite_barrier(rewrite_barrier);

        // Capture shutdown signal + connection pool BEFORE start() consumes
        // the TcpServer, so shutdown() remains functional after that.
        let shutdown_tx = tcp_server.shutdown_signal();
        let connection_pool = tcp_server.connection_pool();

        Ok(Self {
            config,
            storage,
            command_registry,
            metrics,
            persistence: None,
            bgrewrite_persistence_slot,
            shutdown_tx,
            connection_pool,
            tcp_server: Some(tcp_server),
        })
    }

    /// Attach a persistence manager to this server. The dispatch layer
    /// will log mutating commands to the manager's AOF after they
    /// successfully execute. Must be called before `start()`.
    pub fn with_persistence(mut self, persistence: Arc<PersistenceManager>) -> Self {
        self.persistence = Some(Arc::clone(&persistence));
        // Wire BGREWRITEAOF up to the same persistence manager. The
        // OnceLock can only be set once; we ignore the result because
        // calling `with_persistence` twice is a programmer error and
        // the second call is silently a no-op for BGREWRITEAOF.
        let _ = self
            .bgrewrite_persistence_slot
            .set(Arc::clone(&persistence));
        if let Some(server) = self.tcp_server.take() {
            self.tcp_server = Some(server.with_persistence(persistence));
        }
        self
    }

    /// Build a server from a config, automatically wiring persistence and
    /// replaying any existing AOF file before returning. The returned
    /// server is ready to `start()`.
    ///
    /// This is the constructor that mirrors what the production binary
    /// does and is what integration tests should use to exercise the AOF
    /// flow end-to-end.
    pub async fn open(config: Config) -> Result<Self> {
        let aof_enabled = config.storage.aof_enabled;
        let aof_path = config.storage.aof_path.clone();
        let mut server = Self::new(config.clone())?;

        if !aof_enabled {
            return Ok(server);
        }

        // Replay existing AOF onto storage before any client touches it.
        // Build a registry mirror that can dispatch every command we know
        // how to log.
        let mut replay_registry = CommandRegistry::new();
        replay_registry.register(Box::new(SetCommand));
        replay_registry.register(Box::new(GetCommand));
        replay_registry.register(Box::new(DelCommand));
        replay_registry.register(Box::new(ExistsCommand));
        replay_registry.register(Box::new(MsetCommand));
        replay_registry.register(Box::new(ExpireCommand));
        replay_registry.register(Box::new(TtlCommand));
        replay_registry.register(Box::new(PexpireCommand));
        replay_registry.register(Box::new(ExpireatCommand));
        replay_registry.register(Box::new(PexpireatCommand));
        replay_registry.register(Box::new(IncrCommand));
        replay_registry.register(Box::new(DecrCommand));
        replay_registry.register(Box::new(HsetCommand));
        replay_registry.register(Box::new(HgetCommand));
        replay_registry.register(Box::new(HdelCommand));
        replay_registry.register(Box::new(HgetallCommand));
        replay_registry.register(Box::new(HexistsCommand));
        // FLUSHDB is the only stage-7-partial command that mutates and
        // can therefore appear in the AOF; the read-only PING/ECHO/
        // DBSIZE/TYPE never get logged.
        replay_registry.register(Box::new(FlushdbCommand));

        let replay_storage = Arc::clone(&server.storage);
        replay_aof_file(&aof_path, |cmd| {
            let storage = Arc::clone(&replay_storage);
            let registry = &replay_registry;
            async move {
                let _ = registry.execute(&cmd, &storage).await;
                Ok(())
            }
        })
        .await?;

        let persistence = Arc::new(PersistenceManager::new(&config).await?);
        server = server.with_persistence(persistence);
        Ok(server)
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

    /// Gracefully shut the server down. Signals the accept loop and any
    /// per-connection handlers to stop, then waits up to 30 seconds for
    /// active connections to drain. If a persistence manager is attached
    /// it is asked to flush+close.
    pub async fn shutdown(&self) -> Result<()> {
        tracing::info!("Server shutdown requested");

        // Tell the accept loop and all connection handlers to exit.
        let _ = self.shutdown_tx.send(());

        // Wait for connection drain.
        let drain_timeout = Duration::from_secs(30);
        let drain_start = std::time::Instant::now();
        loop {
            let active = self.connection_pool.active_connections().await;
            if active == 0 {
                tracing::info!("All connections drained");
                break;
            }
            if drain_start.elapsed() >= drain_timeout {
                tracing::warn!(
                    "Drain timeout reached with {active} connections still active; \
                     proceeding with shutdown anyway"
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Flush persistence on the way out.
        if let Some(p) = &self.persistence {
            if let Err(e) = p.shutdown().await {
                tracing::error!("Persistence shutdown failed: {e}");
            }
        }

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

    /// Get a shared (`Arc`) handle to the storage layer. Useful for
    /// passing into other components (e.g. the health checker) that need
    /// to outlive the server reference.
    pub fn storage_arc(&self) -> Arc<MemoryStore> {
        Arc::clone(&self.storage)
    }

    /// Get a shared (`Arc`) handle to the metrics collector.
    pub fn metrics_arc(&self) -> Arc<MetricsCollector> {
        Arc::clone(&self.metrics)
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

        // 13 base commands + PING/ECHO/DBSIZE/TYPE/FLUSHDB/INFO/KEYS = 20
        // + HMGET/HKEYS/HVALS/HLEN (Stage 8 partial 3) = 24
        // + MGET/MSET (Stage 7 partial 5) = 26
        // + PTTL/PEXPIRE/EXPIREAT/PEXPIREAT (Stage 8 partial 4) = 30
        // + QUIT/COMMAND (Stage 7 partial 6) = 32
        // + BGREWRITEAOF (Stage 11) = 33
        assert_eq!(stats.registered_commands, 33);
        assert!(stats.command_names.contains(&"SET".to_string()));
        assert!(stats.command_names.contains(&"GET".to_string()));
        assert!(stats.command_names.contains(&"INCR".to_string()));
        assert!(stats.command_names.contains(&"PING".to_string()));
        assert!(stats.command_names.contains(&"ECHO".to_string()));
        assert!(stats.command_names.contains(&"HMGET".to_string()));
        assert!(stats.command_names.contains(&"HLEN".to_string()));
        assert!(stats.command_names.contains(&"MGET".to_string()));
        assert!(stats.command_names.contains(&"MSET".to_string()));
        assert!(stats.command_names.contains(&"PTTL".to_string()));
        assert!(stats.command_names.contains(&"PEXPIRE".to_string()));
        assert!(stats.command_names.contains(&"EXPIREAT".to_string()));
        assert!(stats.command_names.contains(&"PEXPIREAT".to_string()));
        assert_eq!(stats.config_summary, "127.0.0.1:6379");
    }

    #[test]
    fn test_server_component_access() {
        let config = Config::default();
        let server = RustyPotatoServer::new(config).unwrap();

        // Test that we can access the storage and command registry
        assert_eq!(server.command_registry().command_count(), 33);
        assert!(server.command_registry().has_command("SET"));
        assert!(server.command_registry().has_command("GET"));
        assert!(server.command_registry().has_command("HSET"));
        assert!(server.command_registry().has_command("HGET"));
        assert!(server.command_registry().has_command("HMGET"));
        assert!(server.command_registry().has_command("HKEYS"));
        assert!(server.command_registry().has_command("HVALS"));
        assert!(server.command_registry().has_command("HLEN"));
        assert!(server.command_registry().has_command("MGET"));
        assert!(server.command_registry().has_command("MSET"));
        assert!(server.command_registry().has_command("PTTL"));
        assert!(server.command_registry().has_command("PEXPIRE"));
        assert!(server.command_registry().has_command("EXPIREAT"));
        assert!(server.command_registry().has_command("PEXPIREAT"));
        assert!(server.command_registry().has_command("PING"));
        assert!(server.command_registry().has_command("ECHO"));
        assert!(server.command_registry().has_command("COMMAND"));
        assert!(server.command_registry().has_command("QUIT"));
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
