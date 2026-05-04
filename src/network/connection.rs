//! Client connection management with connection pooling.
//!
//! Tracks individual client connections and maintains a semaphore-bounded
//! pool of active connections (Stage 3 DoS hardening). Per-connection
//! buffers are owned by the per-connection read loop in `network::server`;
//! a previous shared `BufferPool` was unused infrastructure and has been
//! removed.

use crate::error::{Result, RustyPotatoError};
use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::{OwnedSemaphorePermit, RwLock, Semaphore};
use tracing::{debug, warn};
use uuid::Uuid;

/// Client connection representation with metadata and stream.
///
/// Tracks just enough state for the dispatch loop and the idle-eviction
/// background task. A previous version of this struct also carried
/// per-connection `bytes_read` / `bytes_written` counters and a
/// `connection_info()` getter that returned a richer `ConnectionInfo`
/// snapshot — neither was ever consumed (the network-level metrics
/// already live in `MetricsCollector::network`), so they were removed
/// in the Stage 13 cleanup.
#[derive(Debug)]
pub struct ClientConnection {
    pub client_id: Uuid,
    pub stream: TcpStream,
    pub remote_addr: SocketAddr,
    pub connected_at: Instant,
    pub last_activity: Arc<RwLock<Instant>>,
    pub commands_processed: Arc<AtomicU64>,
}

impl ClientConnection {
    /// Create a new client connection
    pub fn new(client_id: Uuid, stream: TcpStream, remote_addr: SocketAddr) -> Self {
        let now = Instant::now();

        Self {
            client_id,
            stream,
            remote_addr,
            connected_at: now,
            last_activity: Arc::new(RwLock::new(now)),
            commands_processed: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Update the last activity timestamp
    pub async fn update_last_activity(&self) {
        let mut last_activity = self.last_activity.write().await;
        *last_activity = Instant::now();
    }

    /// Get the last activity timestamp
    pub async fn get_last_activity(&self) -> Instant {
        *self.last_activity.read().await
    }

    /// Increment the commands processed counter
    pub fn increment_commands_processed(&self) {
        self.commands_processed.fetch_add(1, Ordering::Relaxed);
    }
}

/// Connection pool for managing multiple clients with limits and statistics.
///
/// Slot accounting is done by a `tokio::sync::Semaphore` rather than the
/// previous check-then-act on a length counter, so the configured
/// `max_connections` is a hard limit even under bursty concurrent
/// accepts.
pub struct ConnectionPool {
    connections: DashMap<Uuid, Arc<tokio::sync::Mutex<ClientConnection>>>,
    /// Holds the per-connection permit alive for the lifetime of the
    /// connection. Dropping the permit releases the slot back to the
    /// semaphore.
    permits: DashMap<Uuid, OwnedSemaphorePermit>,
    max_connections: usize,
    semaphore: Arc<Semaphore>,
    total_connections_accepted: AtomicU64,
}

impl ConnectionPool {
    /// Create a new connection pool with the specified maximum connections
    pub fn new(max_connections: usize) -> Self {
        Self {
            connections: DashMap::new(),
            permits: DashMap::new(),
            max_connections,
            semaphore: Arc::new(Semaphore::new(max_connections)),
            total_connections_accepted: AtomicU64::new(0),
        }
    }

    /// Try to atomically reserve a slot for a new connection. Returns the
    /// permit on success; the caller must hand the permit to
    /// `add_connection` or drop it (which returns the slot).
    ///
    /// Replaces the previous TOCTOU `can_accept_connection() then add`
    /// pattern, which under concurrent accepts could let
    /// `max_connections + N` connections through.
    ///
    /// Rejection metrics live in `MetricsCollector::network` —
    /// `record_connection_event(ConnectionEvent::Rejected)` is invoked
    /// from the accept loop when this returns `None`.
    pub fn try_reserve_slot(&self) -> Option<OwnedSemaphorePermit> {
        Arc::clone(&self.semaphore).try_acquire_owned().ok()
    }

    /// Backwards-compatible peek at remaining capacity. Prefer
    /// `try_reserve_slot` for any decision-making — this method races
    /// against concurrent accepts.
    pub async fn can_accept_connection(&self) -> bool {
        self.semaphore.available_permits() > 0
    }

    /// Add a new connection to the pool. Caller must have already
    /// reserved a permit via `try_reserve_slot`.
    pub async fn add_connection(
        &self,
        connection: ClientConnection,
        permit: OwnedSemaphorePermit,
    ) -> Result<()> {
        let client_id = connection.client_id;
        self.connections
            .insert(client_id, Arc::new(tokio::sync::Mutex::new(connection)));
        self.permits.insert(client_id, permit);
        self.total_connections_accepted
            .fetch_add(1, Ordering::Relaxed);

        debug!(
            "Added connection {} to pool (active: {})",
            client_id,
            self.connections.len()
        );
        Ok(())
    }

    /// Remove a connection from the pool. Drops the associated semaphore
    /// permit, releasing the slot.
    pub async fn remove_connection(&self, client_id: Uuid) -> Result<()> {
        let _permit = self.permits.remove(&client_id);
        match self.connections.remove(&client_id) {
            Some(_) => {
                debug!(
                    "Removed connection {} from pool (active: {})",
                    client_id,
                    self.connections.len()
                );
                Ok(())
            }
            None => {
                warn!("Attempted to remove non-existent connection: {}", client_id);
                Err(RustyPotatoError::NetworkError {
                    message: format!("Connection {client_id} not found in pool"),
                    source: None,
                    connection_id: Some(client_id.to_string()),
                })
            }
        }
    }

    /// Get a connection by client ID
    pub async fn get_connection(
        &self,
        client_id: Uuid,
    ) -> Option<Arc<tokio::sync::Mutex<ClientConnection>>> {
        self.connections
            .get(&client_id)
            .map(|entry| Arc::clone(entry.value()))
    }

    /// Get the number of active connections
    pub async fn active_connections(&self) -> usize {
        self.connections.len()
    }

    /// Get the maximum number of connections allowed
    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    /// Get the total number of connections accepted
    pub async fn total_connections_accepted(&self) -> u64 {
        self.total_connections_accepted.load(Ordering::Relaxed)
    }

    /// Find connections that have been idle for longer than the specified
    /// duration. Uses `try_lock` so this scan never blocks; a connection
    /// whose mutex is currently held (e.g. the handler is in a read) is
    /// treated as not-idle for this tick — by definition it's busy.
    pub async fn find_idle_connections(&self, idle_threshold: std::time::Duration) -> Vec<Uuid> {
        let mut idle_connections = Vec::new();
        let now = Instant::now();

        for entry in self.connections.iter() {
            // try_lock keeps the eviction task non-blocking: if a handler
            // is mid-read we'll re-check on the next tick.
            if let Ok(connection) = entry.value().try_lock() {
                let last_activity = connection.get_last_activity().await;
                if now.duration_since(last_activity) > idle_threshold {
                    idle_connections.push(connection.client_id);
                }
            }
        }

        idle_connections
    }

    /// Close idle connections that haven't seen activity for at least
    /// `idle_threshold`. Best-effort: if the connection's mutex is busy
    /// (handler is in a read) we still remove it from the pool — the
    /// handler will observe the missing entry on its next iteration. If
    /// we can grab the mutex we also shut the socket down so the read
    /// returns EOF immediately.
    pub async fn evict_idle(&self, idle_threshold: std::time::Duration) -> usize {
        let idle_ids = self.find_idle_connections(idle_threshold).await;
        let mut evicted = 0usize;
        for client_id in idle_ids {
            if let Some(entry) = self.connections.get(&client_id) {
                if let Ok(mut conn) = entry.value().try_lock() {
                    let _ = conn.stream.shutdown().await;
                }
            }
            if self.remove_connection(client_id).await.is_ok() {
                evicted += 1;
            }
        }
        if evicted > 0 {
            debug!("Evicted {evicted} idle connections");
        }
        evicted
    }

    /// Close all connections (used during shutdown). Drops the associated
    /// permits so the semaphore returns to its initial capacity.
    pub async fn close_all_connections(&self) -> usize {
        let count = self.connections.len();
        self.connections.clear();
        self.permits.clear();
        debug!("Closed {} connections during shutdown", count);
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::net::{TcpListener, TcpStream};

    async fn create_test_connection() -> (ClientConnection, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let client_stream = TcpStream::connect(addr).await.unwrap();
        let (server_stream, client_addr) = listener.accept().await.unwrap();

        let client_id = Uuid::new_v4();
        let connection = ClientConnection::new(client_id, server_stream, client_addr);

        (connection, client_stream)
    }

    #[tokio::test]
    async fn test_client_connection_creation() {
        let (connection, _) = create_test_connection().await;
        assert_eq!(connection.commands_processed.load(Ordering::Relaxed), 0);
        assert!(connection.connected_at.elapsed().as_millis() < 100);
    }

    #[tokio::test]
    async fn test_client_connection_activity_tracking() {
        let (connection, _) = create_test_connection().await;

        let initial_activity = connection.get_last_activity().await;

        // Wait a bit and update activity
        tokio::time::sleep(Duration::from_millis(10)).await;
        connection.update_last_activity().await;

        let updated_activity = connection.get_last_activity().await;
        assert!(updated_activity > initial_activity);
    }

    #[tokio::test]
    async fn test_client_connection_command_counting() {
        let (connection, _) = create_test_connection().await;
        assert_eq!(connection.commands_processed.load(Ordering::Relaxed), 0);
        connection.increment_commands_processed();
        assert_eq!(connection.commands_processed.load(Ordering::Relaxed), 1);
        connection.increment_commands_processed();
        connection.increment_commands_processed();
        assert_eq!(connection.commands_processed.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn test_connection_pool_creation() {
        let pool = ConnectionPool::new(100);

        assert_eq!(pool.max_connections(), 100);
        assert_eq!(pool.active_connections().await, 0);
        assert_eq!(pool.total_connections_accepted().await, 0);
    }

    #[tokio::test]
    async fn test_connection_pool_add_remove() {
        let pool = ConnectionPool::new(10);
        let (connection, _) = create_test_connection().await;
        let client_id = connection.client_id;

        // Reserve and add connection
        let permit = pool.try_reserve_slot().expect("permit should be available");
        let result = pool.add_connection(connection, permit).await;
        assert!(result.is_ok());
        assert_eq!(pool.active_connections().await, 1);
        assert_eq!(pool.total_connections_accepted().await, 1);

        // Remove connection
        let result = pool.remove_connection(client_id).await;
        assert!(result.is_ok());
        assert_eq!(pool.active_connections().await, 0);
    }

    #[tokio::test]
    async fn test_connection_pool_limits() {
        let pool = ConnectionPool::new(2);

        // Add first connection
        let (connection1, _) = create_test_connection().await;
        let p1 = pool.try_reserve_slot().unwrap();
        assert!(pool.add_connection(connection1, p1).await.is_ok());
        assert!(pool.can_accept_connection().await);

        // Add second connection
        let (connection2, _) = create_test_connection().await;
        let p2 = pool.try_reserve_slot().unwrap();
        assert!(pool.add_connection(connection2, p2).await.is_ok());
        assert!(!pool.can_accept_connection().await);
        assert_eq!(pool.active_connections().await, 2);

        // Try to reserve a third slot — should be denied. The semaphore
        // is the source of truth.
        assert!(pool.try_reserve_slot().is_none());
    }

    #[tokio::test]
    async fn test_connection_pool_get_connection() {
        let pool = ConnectionPool::new(10);
        let (connection, _) = create_test_connection().await;
        let client_id = connection.client_id;

        // Add connection
        pool.add_connection(connection, pool.try_reserve_slot().unwrap())
            .await
            .unwrap();

        // Get connection
        let retrieved = pool.get_connection(client_id).await;
        assert!(retrieved.is_some());
        let retrieved_connection = retrieved.unwrap();
        let retrieved_client_id = {
            let conn = retrieved_connection.lock().await;
            conn.client_id
        };
        assert_eq!(retrieved_client_id, client_id);

        // Try to get non-existent connection
        let non_existent = pool.get_connection(Uuid::new_v4()).await;
        assert!(non_existent.is_none());
    }

    #[tokio::test]
    async fn test_connection_pool_find_idle_connections() {
        let pool = ConnectionPool::new(10);
        let (connection, _) = create_test_connection().await;
        let client_id = connection.client_id;

        pool.add_connection(connection, pool.try_reserve_slot().unwrap())
            .await
            .unwrap();

        // Should not be idle immediately
        let idle = pool.find_idle_connections(Duration::from_millis(100)).await;
        assert!(idle.is_empty());

        // Wait and check again
        tokio::time::sleep(Duration::from_millis(150)).await;
        let idle = pool.find_idle_connections(Duration::from_millis(100)).await;
        assert_eq!(idle.len(), 1);
        assert_eq!(idle[0], client_id);
    }

    #[tokio::test]
    async fn test_connection_pool_close_all() {
        let pool = ConnectionPool::new(10);
        let (connection1, _) = create_test_connection().await;
        let (connection2, _) = create_test_connection().await;

        pool.add_connection(connection1, pool.try_reserve_slot().unwrap())
            .await
            .unwrap();
        pool.add_connection(connection2, pool.try_reserve_slot().unwrap())
            .await
            .unwrap();

        assert_eq!(pool.active_connections().await, 2);

        let closed_count = pool.close_all_connections().await;
        assert_eq!(closed_count, 2);
        assert_eq!(pool.active_connections().await, 0);
    }

    #[tokio::test]
    async fn test_connection_pool_remove_nonexistent() {
        let pool = ConnectionPool::new(10);
        let non_existent_id = Uuid::new_v4();

        let result = pool.remove_connection(non_existent_id).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not found in pool"));
    }
}
