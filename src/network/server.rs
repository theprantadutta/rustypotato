//! TCP server implementation with async connection handling
//!
//! This module implements a high-performance TCP server using tokio for async I/O.
//! It handles multiple concurrent client connections, command processing, and
//! graceful shutdown with connection draining.

use crate::commands::{CommandRegistry, CommandResult};
use crate::config::Config;
use crate::error::{Result, RustyPotatoError};
use crate::metrics::{ConnectionEvent, MetricsCollector, Timer};
use crate::network::{encode_error, ClientConnection, ConnectionPool, RespCodec};
use crate::storage::MemoryStore;
use bytes::BytesMut;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// TCP server for handling client connections
pub struct TcpServer {
    config: Arc<Config>,
    storage: Arc<MemoryStore>,
    command_registry: Arc<CommandRegistry>,
    connection_pool: Arc<ConnectionPool>,
    metrics: Arc<MetricsCollector>,
    shutdown_tx: Option<broadcast::Sender<()>>,
    listening_addr: Option<SocketAddr>,
}

impl TcpServer {
    /// Create a new TCP server with the given configuration and dependencies
    pub fn new(
        config: Arc<Config>,
        storage: Arc<MemoryStore>,
        command_registry: Arc<CommandRegistry>,
    ) -> Self {
        let connection_pool = Arc::new(ConnectionPool::new(config.server.max_connections));
        let metrics = Arc::new(MetricsCollector::new());

        Self {
            config,
            storage,
            command_registry,
            connection_pool,
            metrics,
            shutdown_tx: None,
            listening_addr: None,
        }
    }

    /// Create a new TCP server with custom metrics collector
    pub fn with_metrics(
        config: Arc<Config>,
        storage: Arc<MemoryStore>,
        command_registry: Arc<CommandRegistry>,
        metrics: Arc<MetricsCollector>,
    ) -> Self {
        let connection_pool = Arc::new(ConnectionPool::new(config.server.max_connections));

        Self {
            config,
            storage,
            command_registry,
            connection_pool,
            metrics,
            shutdown_tx: None,
            listening_addr: None,
        }
    }

    /// Get the metrics collector
    pub fn metrics(&self) -> &Arc<MetricsCollector> {
        &self.metrics
    }

    /// Start the TCP server and listen for connections
    pub async fn start(&mut self) -> Result<()> {
        let bind_addr = format!(
            "{}:{}",
            self.config.server.bind_address, self.config.server.port
        );
        let listener =
            TcpListener::bind(&bind_addr)
                .await
                .map_err(|e| RustyPotatoError::NetworkError {
                    message: format!("Failed to bind to {}: {}", bind_addr, e),
                    source: Some(Box::new(e)),
                    connection_id: None,
                })?;

        let local_addr = listener
            .local_addr()
            .map_err(|e| RustyPotatoError::NetworkError {
                message: format!("Failed to get local address: {}", e),
                source: Some(Box::new(e)),
                connection_id: None,
            })?;

        // Store the actual listening address
        self.listening_addr = Some(local_addr);

        info!("RustyPotato TCP server listening on {}", local_addr);

        // Create shutdown channel
        let (shutdown_tx, _) = broadcast::channel(1);
        self.shutdown_tx = Some(shutdown_tx.clone());

        // Run the server loop
        self.run_server_loop(listener, shutdown_tx).await
    }

    /// Start the server and return the listening address (for testing)
    pub async fn start_with_addr(&mut self) -> Result<SocketAddr> {
        let bind_addr = format!(
            "{}:{}",
            self.config.server.bind_address, self.config.server.port
        );
        let listener =
            TcpListener::bind(&bind_addr)
                .await
                .map_err(|e| RustyPotatoError::NetworkError {
                    message: format!("Failed to bind to {}: {}", bind_addr, e),
                    source: Some(Box::new(e)),
                    connection_id: None,
                })?;

        let local_addr = listener
            .local_addr()
            .map_err(|e| RustyPotatoError::NetworkError {
                message: format!("Failed to get local address: {}", e),
                source: Some(Box::new(e)),
                connection_id: None,
            })?;

        // Store the actual listening address
        self.listening_addr = Some(local_addr);

        info!("RustyPotato TCP server listening on {}", local_addr);

        // Create shutdown channel
        let (shutdown_tx, _) = broadcast::channel(1);
        self.shutdown_tx = Some(shutdown_tx.clone());

        // Spawn the server loop in the background
        let storage = Arc::clone(&self.storage);
        let command_registry = Arc::clone(&self.command_registry);
        let connection_pool = Arc::clone(&self.connection_pool);
        let config = Arc::clone(&self.config);

        tokio::spawn(async move {
            let mut temp_server = TcpServer {
                config,
                storage,
                command_registry,
                connection_pool,
                metrics: Arc::new(MetricsCollector::new()),
                shutdown_tx: Some(shutdown_tx.clone()),
                listening_addr: Some(local_addr),
            };
            let _ = temp_server.run_server_loop(listener, shutdown_tx).await;
        });

        Ok(local_addr)
    }

    /// Run the main server loop
    async fn run_server_loop(
        &mut self,
        listener: TcpListener,
        shutdown_tx: broadcast::Sender<()>,
    ) -> Result<()> {
        // Accept connections in a loop
        let mut shutdown_rx = shutdown_tx.subscribe();
        loop {
            tokio::select! {
                // Accept new connections
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            if let Err(e) = self.handle_new_connection(stream, addr, shutdown_tx.subscribe()).await {
                                error!("Failed to handle new connection from {}: {}", addr, e);
                            }
                        }
                        Err(e) => {
                            error!("Failed to accept connection: {}", e);
                            // Continue accepting other connections
                        }
                    }
                }
                // Handle shutdown signal
                _ = shutdown_rx.recv() => {
                    info!("Received shutdown signal, stopping server");
                    break;
                }
            }
        }

        // Wait for all connections to drain
        self.drain_connections().await;
        info!("TCP server stopped");
        Ok(())
    }

    /// Handle a new client connection
    async fn handle_new_connection(
        &self,
        mut stream: TcpStream,
        addr: SocketAddr,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<()> {
        // Check connection limits
        if !self.connection_pool.can_accept_connection().await {
            warn!(
                "Connection limit reached ({} active), rejecting connection from {}",
                self.connection_pool.active_connections().await,
                addr
            );

            // Record rejected connection
            self.metrics
                .record_connection_event(ConnectionEvent::Rejected)
                .await;

            // Send a proper error response before closing
            let error_msg = b"-ERR server connection limit reached\r\n";
            let _ = stream.write_all(error_msg).await;
            let _ = stream.flush().await;
            let _ = stream.shutdown().await;
            return Ok(());
        }

        // Configure socket options for better performance
        if let Err(e) = Self::configure_socket(&stream, &self.config) {
            warn!("Failed to configure socket options for {}: {}", addr, e);
        }

        // Create client connection
        let client_id = Uuid::new_v4();
        let connection = ClientConnection::new(client_id, stream, addr);

        // Add to connection pool
        if let Err(e) = self.connection_pool.add_connection(connection).await {
            error!(
                "Failed to add connection {} from {} to pool: {}",
                client_id, addr, e
            );
            self.metrics
                .record_connection_event(ConnectionEvent::Rejected)
                .await;
            return Err(e);
        }

        // Record successful connection
        self.metrics
            .record_connection_event(ConnectionEvent::Connected)
            .await;

        info!(
            "New client connected: {} from {} (active connections: {})",
            client_id,
            addr,
            self.connection_pool.active_connections().await
        );

        // Spawn connection handler with proper error handling
        let storage = Arc::clone(&self.storage);
        let command_registry = Arc::clone(&self.command_registry);
        let connection_pool = Arc::clone(&self.connection_pool);
        let config = Arc::clone(&self.config);
        let metrics = Arc::clone(&self.metrics);

        tokio::spawn(async move {
            let connection_result =
                if let Some(connection_arc) = connection_pool.get_connection(client_id).await {
                    Self::handle_connection(
                        connection_arc,
                        storage,
                        command_registry,
                        config,
                        metrics.clone(),
                        &mut shutdown_rx,
                    )
                    .await
                } else {
                    error!(
                        "Connection {} not found in pool immediately after adding",
                        client_id
                    );
                    Err(RustyPotatoError::ConnectionError {
                        message: "Connection not found in pool".to_string(),
                        connection_id: Some(client_id.to_string()),
                        source: None,
                    })
                };

            // Record disconnection and log connection result
            metrics
                .record_connection_event(ConnectionEvent::Disconnected)
                .await;

            match connection_result {
                Ok(()) => info!("Client {} from {} disconnected cleanly", client_id, addr),
                Err(e) => {
                    if e.is_client_error() {
                        info!(
                            "Client {} from {} disconnected due to client error: {}",
                            client_id, addr, e
                        );
                    } else {
                        warn!(
                            "Client {} from {} disconnected due to server error: {}",
                            client_id, addr, e
                        );
                    }
                }
            }

            // Remove connection from pool with retry logic
            for attempt in 1..=3 {
                match connection_pool.remove_connection(client_id).await {
                    Ok(()) => {
                        debug!(
                            "Successfully removed connection {} from pool on attempt {}",
                            client_id, attempt
                        );
                        break;
                    }
                    Err(e) => {
                        if attempt == 3 {
                            error!(
                                "Failed to remove connection {} from pool after {} attempts: {}",
                                client_id, attempt, e
                            );
                        } else {
                            warn!("Failed to remove connection {} from pool on attempt {}: {}, retrying...", 
                                  client_id, attempt, e);
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                    }
                }
            }

            info!("Connection handler cleanup completed for client {} from {} (active connections: {})", 
                  client_id, addr, connection_pool.active_connections().await);
        });

        Ok(())
    }

    /// Configure socket options for optimal performance
    fn configure_socket(stream: &TcpStream, config: &Config) -> Result<()> {
        // For now, we'll use tokio's built-in socket configuration
        // In a production system, we would use socket2 crate for cross-platform socket options

        if let Err(e) = stream.set_nodelay(config.network.tcp_nodelay) {
            return Err(RustyPotatoError::NetworkError {
                message: format!("Failed to set TCP_NODELAY: {}", e),
                source: Some(Box::new(e)),
                connection_id: None,
            });
        }

        // Note: SO_KEEPALIVE configuration would require socket2 crate
        // For now, we'll log that it's configured but not actually set it
        if config.network.tcp_keepalive {
            debug!("TCP keepalive is configured (would be set with socket2 crate in production)");
        }

        Ok(())
    }

    /// Handle a single client connection
    async fn handle_connection(
        connection: Arc<tokio::sync::Mutex<ClientConnection>>,
        storage: Arc<MemoryStore>,
        command_registry: Arc<CommandRegistry>,
        config: Arc<Config>,
        metrics: Arc<MetricsCollector>,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) -> Result<()> {
        let mut codec = RespCodec::new();
        let mut buffer = BytesMut::with_capacity(4096);
        let (client_id, remote_addr) = {
            let conn = connection.lock().await;
            (conn.client_id, conn.remote_addr)
        };

        info!(
            "Starting connection handler for client {} from {}",
            client_id, remote_addr
        );

        let mut commands_processed = 0u64;
        let connection_start = std::time::Instant::now();

        loop {
            tokio::select! {
                // Read data from client
                result = timeout(
                    Duration::from_secs(config.network.read_timeout),
                    async {
                        let mut conn = connection.lock().await;
                        conn.stream.read_buf(&mut buffer).await
                    }
                ) => {
                    match result {
                        Ok(Ok(0)) => {
                            // Client closed connection gracefully
                            info!(
                                "Client {} from {} closed connection gracefully after {} commands in {:?}",
                                client_id, remote_addr, commands_processed, connection_start.elapsed()
                            );
                            break;
                        }
                        Ok(Ok(bytes_read)) => {
                            // Process received data
                            debug!("Read {} bytes from client {}", bytes_read, client_id);

                            // Record network bytes read
                            metrics.record_network_bytes(bytes_read as u64, 0).await;

                            match Self::process_buffer(
                                &mut codec,
                                &mut buffer,
                                &connection,
                                &storage,
                                &command_registry,
                                &config,
                                &metrics,
                            ).await {
                                Ok(processed_count) => {
                                    commands_processed += processed_count;

                                    // Log periodic statistics for active connections
                                    if commands_processed % 1000 == 0 {
                                        debug!("Client {} from {} has processed {} commands in {:?}",
                                               client_id, remote_addr, commands_processed, connection_start.elapsed());
                                    }
                                }
                                Err(e) => {
                                    // Enhanced error handling with connection context
                                    let error_context = format!("client_id={}, remote_addr={}, commands_processed={}, connection_duration={:?}",
                                                               client_id, remote_addr, commands_processed, connection_start.elapsed());

                                    if e.is_client_error() {
                                        warn!("Client error [{}]: {}", error_context, e);

                                        // Send error response and continue (don't break connection for client errors)
                                        let error_response = encode_error(&e.to_client_error());
                                        if let Err(write_err) = Self::write_response(&connection, &error_response, &config, &metrics).await {
                                            error!("Failed to send error response [{}]: {}", error_context, write_err);
                                            break;
                                        }

                                        // For protocol errors, we might want to be more aggressive about closing connections
                                        if matches!(e, RustyPotatoError::ProtocolError { .. }) {
                                            warn!("Protocol error detected [{}], closing connection after {} commands",
                                                  error_context, commands_processed);
                                            break;
                                        }
                                    } else {
                                        error!("Server error [{}]: {}", error_context, e);

                                        // Try to send a generic error response before closing
                                        let error_response = encode_error("ERR internal server error");
                                        let _ = Self::write_response(&connection, &error_response, &config, &metrics).await;
                                        break; // Server errors should close the connection
                                    }
                                }
                            }
                        }
                        Ok(Err(e)) => {
                            error!("Read error for client {} from {}: {}", client_id, remote_addr, e);
                            break;
                        }
                        Err(_) => {
                            warn!("Read timeout for client {} from {} after {} commands",
                                  client_id, remote_addr, commands_processed);
                            break;
                        }
                    }
                }
                // Handle shutdown signal
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal received for client {} from {}, processed {} commands",
                          client_id, remote_addr, commands_processed);
                    break;
                }
            }
        }

        // Graceful connection shutdown with proper error handling
        {
            let mut conn = connection.lock().await;
            if let Err(e) = conn.stream.shutdown().await {
                debug!("Error shutting down stream for client {}: {}", client_id, e);
            }
        }

        info!(
            "Connection handler finished for client {} from {} after processing {} commands in {:?}",
            client_id, remote_addr, commands_processed, connection_start.elapsed()
        );

        Ok(())
    }

    /// Process data in the buffer and execute commands
    /// Returns the number of commands successfully processed
    async fn process_buffer(
        codec: &mut RespCodec,
        buffer: &mut BytesMut,
        connection: &Arc<tokio::sync::Mutex<ClientConnection>>,
        storage: &MemoryStore,
        command_registry: &CommandRegistry,
        config: &Config,
        metrics: &MetricsCollector,
    ) -> Result<u64> {
        let mut commands_processed = 0u64;

        // Try to decode commands from the buffer
        while !buffer.is_empty() {
            let buffer_data = buffer.clone().freeze();

            match codec.decode(&buffer_data) {
                Ok(Some(mut command)) => {
                    let (client_id, remote_addr) = {
                        let conn = connection.lock().await;
                        (conn.client_id, conn.remote_addr)
                    };

                    // Set the client ID for the command
                    command.client_id = client_id;

                    debug!(
                        "Executing command '{}' with {} args for client {} from {}",
                        command.name,
                        command.args.len(),
                        client_id,
                        remote_addr
                    );

                    // Update connection stats
                    {
                        let conn = connection.lock().await;
                        conn.update_last_activity().await;
                        conn.increment_commands_processed();
                    }

                    // Execute the command with timing
                    let timer = Timer::start();
                    let result = command_registry.execute(&command, storage).await;
                    let command_duration = timer.stop();

                    // Record command metrics
                    metrics
                        .record_command_latency(&command.name, command_duration)
                        .await;

                    // Log slow commands
                    if command_duration.as_millis() > 100 {
                        warn!(
                            "Slow command '{}' for client {} took {:?}",
                            command.name, client_id, command_duration
                        );
                    }

                    // Send response
                    match Self::send_response(connection, result, config, metrics).await {
                        Ok(()) => {
                            commands_processed += 1;
                            debug!(
                                "Successfully processed command '{}' for client {} in {:?}",
                                command.name, client_id, command_duration
                            );
                        }
                        Err(e) => {
                            error!(
                                "Failed to send response for command '{}' to client {} from {}: {}",
                                command.name, client_id, remote_addr, e
                            );
                            return Err(e);
                        }
                    }

                    // Clear processed data from buffer
                    buffer.clear();
                }
                Ok(None) => {
                    // Need more data - this is normal
                    debug!(
                        "Need more data to complete command parsing for client {}",
                        {
                            let conn = connection.lock().await;
                            conn.client_id
                        }
                    );
                    break;
                }
                Err(e) => {
                    let (client_id, remote_addr) = {
                        let conn = connection.lock().await;
                        (conn.client_id, conn.remote_addr)
                    };

                    warn!(
                        "Protocol error for client {} from {}: {}",
                        client_id, remote_addr, e
                    );

                    // Convert to RustyPotatoError for consistent error handling
                    let protocol_error = RustyPotatoError::ProtocolError {
                        message: e.to_string(),
                        command: None,
                        source: Some(Box::new(e)),
                    };

                    // Clear buffer to prevent repeated errors
                    buffer.clear();

                    // Return the error to be handled by the caller
                    return Err(protocol_error);
                }
            }
        }

        Ok(commands_processed)
    }

    /// Send a command result as a response
    async fn send_response(
        connection: &Arc<tokio::sync::Mutex<ClientConnection>>,
        result: CommandResult,
        config: &Config,
        metrics: &MetricsCollector,
    ) -> Result<()> {
        let response_data = match result {
            CommandResult::Ok(value) => {
                let mut codec = RespCodec::new();
                codec
                    .encode(&value)
                    .map_err(|e| RustyPotatoError::NetworkError {
                        message: format!("Failed to encode response: {}", e),
                        source: Some(Box::new(e)),
                        connection_id: None,
                    })?
            }
            CommandResult::Error(msg) => encode_error(&msg),
        };

        Self::write_response(connection, &response_data, config, metrics).await
    }

    /// Write response data to the client
    async fn write_response(
        connection: &Arc<tokio::sync::Mutex<ClientConnection>>,
        data: &[u8],
        config: &Config,
        metrics: &MetricsCollector,
    ) -> Result<()> {
        let mut conn = connection.lock().await;
        match timeout(
            Duration::from_secs(config.network.write_timeout),
            conn.stream.write_all(data),
        )
        .await
        {
            Ok(Ok(())) => {
                // Record bytes written
                metrics.record_network_bytes(0, data.len() as u64).await;

                // Flush the stream
                match timeout(
                    Duration::from_secs(config.network.write_timeout),
                    conn.stream.flush(),
                )
                .await
                {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(e)) => Err(RustyPotatoError::NetworkError {
                        message: format!("Failed to flush stream: {}", e),
                        source: Some(Box::new(e)),
                        connection_id: None,
                    }),
                    Err(_) => Err(RustyPotatoError::NetworkError {
                        message: "Write flush timeout".to_string(),
                        source: None,
                        connection_id: None,
                    }),
                }
            }
            Ok(Err(e)) => Err(RustyPotatoError::NetworkError {
                message: format!("Failed to write response: {}", e),
                source: Some(Box::new(e)),
                connection_id: None,
            }),
            Err(_) => Err(RustyPotatoError::NetworkError {
                message: "Write timeout".to_string(),
                source: None,
                connection_id: None,
            }),
        }
    }

    /// Initiate graceful shutdown
    pub async fn shutdown(&self) -> Result<()> {
        if let Some(shutdown_tx) = &self.shutdown_tx {
            let _ = shutdown_tx.send(());
            info!("Shutdown signal sent to all connections");
        }
        Ok(())
    }

    /// Wait for all connections to drain
    async fn drain_connections(&self) {
        let max_wait = Duration::from_secs(30);
        let start = std::time::Instant::now();

        while self.connection_pool.active_connections().await > 0 {
            if start.elapsed() > max_wait {
                warn!("Connection drain timeout reached, forcing shutdown");
                break;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        info!("All connections drained");
    }

    /// Get server statistics
    pub async fn stats(&self) -> ServerStats {
        ServerStats {
            active_connections: self.connection_pool.active_connections().await,
            max_connections: self.config.server.max_connections,
            total_connections_accepted: self.connection_pool.total_connections_accepted().await,
            bind_address: format!(
                "{}:{}",
                self.config.server.bind_address, self.config.server.port
            ),
        }
    }

    /// Get the server's bind address
    pub fn bind_address(&self) -> String {
        format!(
            "{}:{}",
            self.config.server.bind_address, self.config.server.port
        )
    }

    /// Check if the server is running
    pub fn is_running(&self) -> bool {
        self.shutdown_tx.is_some()
    }

    /// Get the actual listening address (only available after start() is called)
    pub fn listening_addr(&self) -> Option<SocketAddr> {
        self.listening_addr
    }
}

/// Server statistics
#[derive(Debug, Clone)]
pub struct ServerStats {
    pub active_connections: usize,
    pub max_connections: usize,
    pub total_connections_accepted: u64,
    pub bind_address: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{GetCommand, SetCommand};

    async fn create_test_server() -> (TcpServer, Arc<Config>) {
        let config = Arc::new(Config::default());
        let storage = Arc::new(MemoryStore::new());
        let mut command_registry = CommandRegistry::new();

        // Register basic commands for testing
        command_registry.register(Box::new(SetCommand));
        command_registry.register(Box::new(GetCommand));

        let command_registry = Arc::new(command_registry);
        let server = TcpServer::new(config.clone(), storage, command_registry);

        (server, config)
    }

    #[tokio::test]
    async fn test_server_creation() {
        let (server, config) = create_test_server().await;

        assert_eq!(
            server.bind_address(),
            format!("{}:{}", config.server.bind_address, config.server.port)
        );
        assert!(!server.is_running());
    }

    #[tokio::test]
    async fn test_server_stats() {
        let (server, _) = create_test_server().await;
        let stats = server.stats().await;

        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.max_connections, 10000);
        assert_eq!(stats.total_connections_accepted, 0);
    }

    #[tokio::test]
    async fn test_server_bind_address() {
        let (server, _) = create_test_server().await;
        assert_eq!(server.bind_address(), "127.0.0.1:6379");
    }

    // Note: Complex unit tests with mock streams removed for simplicity.
    // The TCP server functionality is thoroughly tested in integration tests.

    #[tokio::test]
    async fn test_server_stats_structure() {
        let stats = ServerStats {
            active_connections: 5,
            max_connections: 100,
            total_connections_accepted: 150,
            bind_address: "127.0.0.1:6379".to_string(),
        };

        assert_eq!(stats.active_connections, 5);
        assert_eq!(stats.max_connections, 100);
        assert_eq!(stats.total_connections_accepted, 150);
        assert_eq!(stats.bind_address, "127.0.0.1:6379");
    }
}
