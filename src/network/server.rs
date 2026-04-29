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
use crate::storage::{MemoryStore, PersistenceManager};
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

/// Asynchronous read/write barrier coordinating BGREWRITEAOF with
/// in-flight mutations. Mutating commands acquire the read side
/// (concurrent), BGREWRITEAOF acquires the write side (exclusive).
/// Read-only commands skip the lock entirely so they continue to
/// flow during a rewrite.
pub type RewriteBarrier = Arc<tokio::sync::RwLock<()>>;

/// TCP server for handling client connections
pub struct TcpServer {
    config: Arc<Config>,
    storage: Arc<MemoryStore>,
    command_registry: Arc<CommandRegistry>,
    connection_pool: Arc<ConnectionPool>,
    metrics: Arc<MetricsCollector>,
    persistence: Option<Arc<PersistenceManager>>,
    rewrite_barrier: RewriteBarrier,
    shutdown_tx: broadcast::Sender<()>,
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
        let (shutdown_tx, _) = broadcast::channel(1);

        Self {
            config,
            storage,
            command_registry,
            connection_pool,
            metrics,
            persistence: None,
            rewrite_barrier: Arc::new(tokio::sync::RwLock::new(())),
            shutdown_tx,
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
        let (shutdown_tx, _) = broadcast::channel(1);

        Self {
            config,
            storage,
            command_registry,
            connection_pool,
            metrics,
            persistence: None,
            rewrite_barrier: Arc::new(tokio::sync::RwLock::new(())),
            shutdown_tx,
            listening_addr: None,
        }
    }

    /// Replace the rewrite barrier with one shared across the rest of
    /// the server. Used by `RustyPotatoServer` to give the BGREWRITEAOF
    /// command and the dispatch path a common coordination handle.
    pub fn with_rewrite_barrier(mut self, barrier: RewriteBarrier) -> Self {
        self.rewrite_barrier = barrier;
        self
    }

    /// Get a clone of the rewrite barrier — used by callers (the lib's
    /// BGREWRITEAOF wiring) that need to acquire the write side.
    pub fn rewrite_barrier(&self) -> RewriteBarrier {
        Arc::clone(&self.rewrite_barrier)
    }

    /// Attach a persistence manager. Mutating commands will be logged to its
    /// AOF after they execute successfully.
    pub fn with_persistence(mut self, persistence: Arc<PersistenceManager>) -> Self {
        self.persistence = Some(persistence);
        self
    }

    /// Get a clone of the shutdown signal sender so a coordinator (e.g.
    /// `RustyPotatoServer`) can trigger graceful shutdown after `start()`
    /// has consumed `self`.
    pub fn shutdown_signal(&self) -> broadcast::Sender<()> {
        self.shutdown_tx.clone()
    }

    /// Get a reference to the connection pool. Useful for shutdown
    /// coordinators that need to wait for the active-connection count to
    /// hit zero.
    pub fn connection_pool(&self) -> Arc<ConnectionPool> {
        Arc::clone(&self.connection_pool)
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
                    message: format!("Failed to bind to {bind_addr}: {e}"),
                    source: Some(Box::new(e)),
                    connection_id: None,
                })?;

        let local_addr = listener
            .local_addr()
            .map_err(|e| RustyPotatoError::NetworkError {
                message: format!("Failed to get local address: {e}"),
                source: Some(Box::new(e)),
                connection_id: None,
            })?;

        // Store the actual listening address
        self.listening_addr = Some(local_addr);

        info!("RustyPotato TCP server listening on {}", local_addr);

        let shutdown_tx = self.shutdown_tx.clone();
        Self::spawn_idle_evictor(
            Arc::clone(&self.connection_pool),
            self.config.network.idle_timeout,
            shutdown_tx.subscribe(),
        );
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
                    message: format!("Failed to bind to {bind_addr}: {e}"),
                    source: Some(Box::new(e)),
                    connection_id: None,
                })?;

        let local_addr = listener
            .local_addr()
            .map_err(|e| RustyPotatoError::NetworkError {
                message: format!("Failed to get local address: {e}"),
                source: Some(Box::new(e)),
                connection_id: None,
            })?;

        // Store the actual listening address
        self.listening_addr = Some(local_addr);

        info!("RustyPotato TCP server listening on {}", local_addr);

        let shutdown_tx = self.shutdown_tx.clone();
        Self::spawn_idle_evictor(
            Arc::clone(&self.connection_pool),
            self.config.network.idle_timeout,
            shutdown_tx.subscribe(),
        );

        // Spawn the server loop in the background
        let storage = Arc::clone(&self.storage);
        let command_registry = Arc::clone(&self.command_registry);
        let connection_pool = Arc::clone(&self.connection_pool);
        let config = Arc::clone(&self.config);
        let metrics = Arc::clone(&self.metrics);
        let persistence = self.persistence.clone();
        let rewrite_barrier = Arc::clone(&self.rewrite_barrier);
        let bg_shutdown_tx = shutdown_tx.clone();

        tokio::spawn(async move {
            let mut temp_server = TcpServer {
                config,
                storage,
                command_registry,
                connection_pool,
                metrics,
                persistence,
                rewrite_barrier,
                shutdown_tx: bg_shutdown_tx.clone(),
                listening_addr: Some(local_addr),
            };
            let _ = temp_server.run_server_loop(listener, bg_shutdown_tx).await;
        });

        Ok(local_addr)
    }

    /// Background task: every `idle_timeout / 4` seconds, scan the
    /// connection pool for connections that haven't seen any activity
    /// within `idle_timeout` and evict them. Exits on shutdown.
    fn spawn_idle_evictor(
        pool: Arc<ConnectionPool>,
        idle_timeout_secs: u64,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) {
        let idle_timeout = Duration::from_secs(idle_timeout_secs);
        // Scan four times per idle window — fine-grained enough that an
        // idle client is closed within ~25% of `idle_timeout`, but coarse
        // enough that the scan cost is negligible.
        let scan_interval = Duration::from_secs((idle_timeout_secs / 4).max(1));
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(scan_interval);
            loop {
                tokio::select! {
                    _ = tick.tick() => {
                        let _ = pool.evict_idle(idle_timeout).await;
                    }
                    _ = shutdown_rx.recv() => {
                        debug!("Idle evictor task shutting down");
                        return;
                    }
                }
            }
        });
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
        // Atomically reserve a slot from the semaphore. If we can't, the
        // pool is at `max_connections`; reject before doing any other
        // work. Replaces the previous TOCTOU `can_accept` + `add` pair.
        let permit = match self.connection_pool.try_reserve_slot() {
            Some(p) => p,
            None => {
                warn!(
                    "Connection limit reached ({} active), rejecting connection from {}",
                    self.connection_pool.active_connections().await,
                    addr
                );
                self.metrics
                    .record_connection_event(ConnectionEvent::Rejected)
                    .await;
                let error_msg = b"-ERR server connection limit reached\r\n";
                let _ = stream.write_all(error_msg).await;
                let _ = stream.flush().await;
                let _ = stream.shutdown().await;
                return Ok(());
            }
        };

        // Configure socket options for better performance
        if let Err(e) = Self::configure_socket(&stream, &self.config) {
            warn!("Failed to configure socket options for {}: {}", addr, e);
        }

        // Create client connection
        let client_id = Uuid::new_v4();
        let connection = ClientConnection::new(client_id, stream, addr);

        // Add to connection pool with the previously-reserved permit
        if let Err(e) = self
            .connection_pool
            .add_connection(connection, permit)
            .await
        {
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
        let persistence = self.persistence.clone();
        let rewrite_barrier = Arc::clone(&self.rewrite_barrier);

        tokio::spawn(async move {
            let connection_result =
                if let Some(connection_arc) = connection_pool.get_connection(client_id).await {
                    Self::handle_connection(
                        connection_arc,
                        storage,
                        command_registry,
                        config,
                        metrics.clone(),
                        persistence,
                        rewrite_barrier,
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

    /// Configure socket options for optimal performance.
    ///
    /// Uses `socket2::SockRef` to apply `SO_KEEPALIVE` so the OS detects
    /// dead peers (e.g. crashed clients with no FIN) without waiting for
    /// the default 2-hour TCP timeout. Defaults: idle 60s, interval 10s,
    /// retries 6 — i.e. a half-dead connection is reaped in roughly two
    /// minutes. `set_nodelay` is also applied here when `tcp_nodelay` is
    /// enabled.
    fn configure_socket(stream: &TcpStream, config: &Config) -> Result<()> {
        if let Err(e) = stream.set_nodelay(config.network.tcp_nodelay) {
            return Err(RustyPotatoError::NetworkError {
                message: format!("Failed to set TCP_NODELAY: {e}"),
                source: Some(Box::new(e)),
                connection_id: None,
            });
        }

        if config.network.tcp_keepalive {
            let sock_ref = socket2::SockRef::from(stream);
            let keepalive = socket2::TcpKeepalive::new()
                .with_time(Duration::from_secs(60))
                .with_interval(Duration::from_secs(10));
            // `with_retries` is unavailable on some platforms; use the
            // platform-native default count (Linux: 9, macOS: 8) when the
            // method isn't there. Linux/glibc and recent Windows do
            // support it, which we annotate here.
            #[cfg(any(target_os = "linux", target_vendor = "apple", target_os = "freebsd"))]
            let keepalive = keepalive.with_retries(6);
            if let Err(e) = sock_ref.set_tcp_keepalive(&keepalive) {
                return Err(RustyPotatoError::NetworkError {
                    message: format!("Failed to set SO_KEEPALIVE: {e}"),
                    source: Some(Box::new(e)),
                    connection_id: None,
                });
            }
        }

        Ok(())
    }

    /// Handle a single client connection
    #[allow(clippy::too_many_arguments)]
    async fn handle_connection(
        connection: Arc<tokio::sync::Mutex<ClientConnection>>,
        storage: Arc<MemoryStore>,
        command_registry: Arc<CommandRegistry>,
        config: Arc<Config>,
        metrics: Arc<MetricsCollector>,
        persistence: Option<Arc<PersistenceManager>>,
        rewrite_barrier: RewriteBarrier,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) -> Result<()> {
        let mut codec = RespCodec::with_limits(crate::network::protocol::CodecLimits {
            max_bulk_size: config.network.max_bulk_size,
            max_array_length: config.network.max_array_length,
            max_buffer_size: config.network.max_buffer_size,
        });
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
                                persistence.as_ref(),
                                &rewrite_barrier,
                            ).await {
                                Ok((processed_count, should_close)) => {
                                    commands_processed += processed_count;

                                    // Log periodic statistics for active connections
                                    if commands_processed.is_multiple_of(1000) {
                                        debug!("Client {} from {} has processed {} commands in {:?}",
                                               client_id, remote_addr, commands_processed, connection_start.elapsed());
                                    }

                                    if should_close {
                                        debug!(
                                            "Client {} from {} sent QUIT after {} commands; closing connection",
                                            client_id, remote_addr, commands_processed
                                        );
                                        break;
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

    /// Process data in the buffer and execute commands.
    ///
    /// Returns `(processed_count, should_close)`. `should_close` is set
    /// when a terminal command (currently only `QUIT`) was successfully
    /// dispatched in this batch — the caller breaks the connection
    /// loop after the response is sent.
    #[allow(clippy::too_many_arguments)]
    async fn process_buffer(
        codec: &mut RespCodec,
        buffer: &mut BytesMut,
        connection: &Arc<tokio::sync::Mutex<ClientConnection>>,
        storage: &MemoryStore,
        command_registry: &CommandRegistry,
        config: &Config,
        metrics: &MetricsCollector,
        persistence: Option<&Arc<PersistenceManager>>,
        rewrite_barrier: &RewriteBarrier,
    ) -> Result<(u64, bool)> {
        let mut commands_processed = 0u64;
        let mut should_close = false;

        // Try to decode commands from the buffer
        while !buffer.is_empty() {
            let buffer_data = buffer.clone().freeze();

            match codec.decode_with_frame(&buffer_data) {
                Ok(Some((mut command, frame))) => {
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

                    // Execute the command with timing.
                    //
                    // Mutating commands hold the rewrite barrier as a
                    // read guard so a concurrent BGREWRITEAOF (which
                    // takes the write side) can pause writes while
                    // it snapshots+swaps. Read-only commands skip the
                    // lock so reads keep flowing during a rewrite.
                    let is_mutation = command_registry.is_mutation(&command.name);
                    let timer = Timer::start();
                    let result = if is_mutation {
                        let _guard = rewrite_barrier.read().await;
                        command_registry.execute(&command, storage).await
                    } else {
                        command_registry.execute(&command, storage).await
                    };
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

                    // Persist mutating commands to AOF on success. Logging
                    // happens after execution but before the response is
                    // sent — this matches Redis' AOF-after-execute model
                    // for `appendfsync everysec`/`no` policies and gives
                    // `always` policy real durability.
                    let is_ok = matches!(result, CommandResult::Ok(_));
                    if is_ok && command_registry.is_mutation(&command.name) {
                        if let Some(p) = persistence {
                            if let Err(e) = p.log_command(frame.clone()).await {
                                error!("AOF log failed for command '{}': {}", command.name, e);
                            }
                        }
                    }

                    // Capture the terminal flag before `result` is moved
                    // into send_response — we only act on it after the
                    // response is successfully written.
                    let is_terminal = command_registry.is_terminal(&command.name);

                    // Send response
                    match Self::send_response(connection, result, config, metrics).await {
                        Ok(()) => {
                            commands_processed += 1;
                            debug!(
                                "Successfully processed command '{}' for client {} in {:?}",
                                command.name, client_id, command_duration
                            );
                            if is_terminal {
                                // QUIT: break out of the parse loop so
                                // the caller closes the connection.
                                should_close = true;
                                buffer.clear();
                                break;
                            }
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

        Ok((commands_processed, should_close))
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
                        message: format!("Failed to encode response: {e}"),
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
                        message: format!("Failed to flush stream: {e}"),
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
                message: format!("Failed to write response: {e}"),
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

    /// Initiate graceful shutdown by signaling the accept loop and any
    /// per-connection handlers. Callers that need to wait for connections
    /// to drain should follow up by polling
    /// `connection_pool().active_connections()`.
    pub async fn shutdown(&self) -> Result<()> {
        let _ = self.shutdown_tx.send(());
        info!("Shutdown signal sent to all connections");
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

    /// Check if the server has been started (i.e. has a bound listener).
    pub fn is_running(&self) -> bool {
        self.listening_addr.is_some()
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
