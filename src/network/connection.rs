//! Client connection management with connection pooling
//! 
//! This module handles individual client connections and maintains a pool
//! of active connections with configurable limits and statistics tracking.

use crate::error::{RustyPotatoError, Result};
use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tracing::{debug, warn};
use uuid::Uuid;

/// Client connection representation with metadata and stream
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

    /// Get the number of commands processed
    pub fn get_commands_processed(&self) -> u64 {
        self.commands_processed.load(Ordering::Relaxed)
    }

    /// Get connection duration
    pub fn connection_duration(&self) -> std::time::Duration {
        self.connected_at.elapsed()
    }

    /// Get connection info for debugging
    pub async fn connection_info(&self) -> ConnectionInfo {
        ConnectionInfo {
            client_id: self.client_id,
            remote_addr: self.remote_addr,
            connected_at: self.connected_at,
            last_activity: self.get_last_activity().await,
            commands_processed: self.get_commands_processed(),
            connection_duration: self.connection_duration(),
        }
    }
}

// Note: We'll store connections in Arc<Mutex<>> to allow sharing

/// Connection information for monitoring and debugging
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub client_id: Uuid,
    pub remote_addr: SocketAddr,
    pub connected_at: Instant,
    pub last_activity: Instant,
    pub commands_processed: u64,
    pub connection_duration: std::time::Duration,
}

/// Connection pool for managing multiple clients with limits and statistics
pub struct ConnectionPool {
    connections: DashMap<Uuid, Arc<tokio::sync::Mutex<ClientConnection>>>,
    max_connections: usize,
    total_connections_accepted: AtomicU64,
    total_connections_rejected: AtomicU64,
}

impl ConnectionPool {
    /// Create a new connection pool with the specified maximum connections
    pub fn new(max_connections: usize) -> Self {
        Self {
            connections: DashMap::new(),
            max_connections,
            total_connections_accepted: AtomicU64::new(0),
            total_connections_rejected: AtomicU64::new(0),
        }
    }

    /// Check if a new connection can be accepted
    pub async fn can_accept_connection(&self) -> bool {
        self.connections.len() < self.max_connections
    }

    /// Add a new connection to the pool
    pub async fn add_connection(&self, connection: ClientConnection) -> Result<()> {
        if !self.can_accept_connection().await {
            self.total_connections_rejected.fetch_add(1, Ordering::Relaxed);
            return Err(RustyPotatoError::NetworkError(
                "Connection pool is full".to_string()
            ));
        }

        let client_id = connection.client_id;
        self.connections.insert(client_id, Arc::new(tokio::sync::Mutex::new(connection)));
        self.total_connections_accepted.fetch_add(1, Ordering::Relaxed);
        
        debug!("Added connection {} to pool (active: {})", client_id, self.connections.len());
        Ok(())
    }

    /// Remove a connection from the pool
    pub async fn remove_connection(&self, client_id: Uuid) -> Result<()> {
        match self.connections.remove(&client_id) {
            Some(_) => {
                debug!("Removed connection {} from pool (active: {})", client_id, self.connections.len());
                Ok(())
            }
            None => {
                warn!("Attempted to remove non-existent connection: {}", client_id);
                Err(RustyPotatoError::NetworkError(
                    format!("Connection {} not found in pool", client_id)
                ))
            }
        }
    }

    /// Get a connection by client ID
    pub async fn get_connection(&self, client_id: Uuid) -> Option<Arc<tokio::sync::Mutex<ClientConnection>>> {
        self.connections.get(&client_id).map(|entry| Arc::clone(entry.value()))
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

    /// Get the total number of connections rejected
    pub async fn total_connections_rejected(&self) -> u64 {
        self.total_connections_rejected.load(Ordering::Relaxed)
    }

    /// Get connection pool statistics
    pub async fn stats(&self) -> ConnectionPoolStats {
        ConnectionPoolStats {
            active_connections: self.active_connections().await,
            max_connections: self.max_connections,
            total_connections_accepted: self.total_connections_accepted().await,
            total_connections_rejected: self.total_connections_rejected().await,
            utilization_percentage: (self.active_connections().await as f64 / self.max_connections as f64) * 100.0,
        }
    }

    /// Get information about all active connections
    pub async fn list_connections(&self) -> Vec<ConnectionInfo> {
        let mut connections = Vec::new();
        
        for entry in self.connections.iter() {
            let connection = entry.value().lock().await;
            connections.push(connection.connection_info().await);
        }
        
        connections.sort_by(|a, b| a.connected_at.cmp(&b.connected_at));
        connections
    }

    /// Find connections that have been idle for longer than the specified duration
    pub async fn find_idle_connections(&self, idle_threshold: std::time::Duration) -> Vec<Uuid> {
        let mut idle_connections = Vec::new();
        let now = Instant::now();
        
        for entry in self.connections.iter() {
            let connection = entry.value().lock().await;
            let last_activity = connection.get_last_activity().await;
            
            if now.duration_since(last_activity) > idle_threshold {
                idle_connections.push(connection.client_id);
            }
        }
        
        idle_connections
    }

    /// Close all connections (used during shutdown)
    pub async fn close_all_connections(&self) -> usize {
        let count = self.connections.len();
        self.connections.clear();
        debug!("Closed {} connections during shutdown", count);
        count
    }

    /// Check if the pool is empty
    pub async fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    /// Check if the pool is full
    pub async fn is_full(&self) -> bool {
        self.connections.len() >= self.max_connections
    }
}

impl Default for ConnectionPool {
    fn default() -> Self {
        Self::new(10000) // Default max connections
    }
}

/// Connection pool statistics
#[derive(Debug, Clone)]
pub struct ConnectionPoolStats {
    pub active_connections: usize,
    pub max_connections: usize,
    pub total_connections_accepted: u64,
    pub total_connections_rejected: u64,
    pub utilization_percentage: f64,
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
        
        assert_eq!(connection.get_commands_processed(), 0);
        assert!(connection.connection_duration().as_millis() < 100); // Should be very recent
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
        
        assert_eq!(connection.get_commands_processed(), 0);
        
        connection.increment_commands_processed();
        assert_eq!(connection.get_commands_processed(), 1);
        
        connection.increment_commands_processed();
        connection.increment_commands_processed();
        assert_eq!(connection.get_commands_processed(), 3);
    }

    #[tokio::test]
    async fn test_client_connection_info() {
        let (connection, _) = create_test_connection().await;
        
        connection.increment_commands_processed();
        let info = connection.connection_info().await;
        
        assert_eq!(info.client_id, connection.client_id);
        assert_eq!(info.remote_addr, connection.remote_addr);
        assert_eq!(info.commands_processed, 1);
        assert!(info.connection_duration.as_millis() < 100);
    }

    #[tokio::test]
    async fn test_connection_pool_creation() {
        let pool = ConnectionPool::new(100);
        
        assert_eq!(pool.max_connections(), 100);
        assert_eq!(pool.active_connections().await, 0);
        assert_eq!(pool.total_connections_accepted().await, 0);
        assert_eq!(pool.total_connections_rejected().await, 0);
        assert!(pool.is_empty().await);
        assert!(!pool.is_full().await);
    }

    #[tokio::test]
    async fn test_connection_pool_add_remove() {
        let pool = ConnectionPool::new(10);
        let (connection, _) = create_test_connection().await;
        let client_id = connection.client_id;
        
        // Add connection
        let result = pool.add_connection(connection).await;
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
        assert!(pool.add_connection(connection1).await.is_ok());
        assert!(pool.can_accept_connection().await);
        
        // Add second connection
        let (connection2, _) = create_test_connection().await;
        assert!(pool.add_connection(connection2).await.is_ok());
        assert!(!pool.can_accept_connection().await);
        assert!(pool.is_full().await);
        
        // Try to add third connection (should fail)
        let (connection3, _) = create_test_connection().await;
        let result = pool.add_connection(connection3).await;
        assert!(result.is_err());
        assert_eq!(pool.total_connections_rejected().await, 1);
    }

    #[tokio::test]
    async fn test_connection_pool_get_connection() {
        let pool = ConnectionPool::new(10);
        let (connection, _) = create_test_connection().await;
        let client_id = connection.client_id;
        
        // Add connection
        pool.add_connection(connection).await.unwrap();
        
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
    async fn test_connection_pool_stats() {
        let pool = ConnectionPool::new(10);
        let (connection1, _) = create_test_connection().await;
        let (connection2, _) = create_test_connection().await;
        
        pool.add_connection(connection1).await.unwrap();
        pool.add_connection(connection2).await.unwrap();
        
        let stats = pool.stats().await;
        assert_eq!(stats.active_connections, 2);
        assert_eq!(stats.max_connections, 10);
        assert_eq!(stats.total_connections_accepted, 2);
        assert_eq!(stats.total_connections_rejected, 0);
        assert_eq!(stats.utilization_percentage, 20.0);
    }

    #[tokio::test]
    async fn test_connection_pool_list_connections() {
        let pool = ConnectionPool::new(10);
        let (connection1, _) = create_test_connection().await;
        let (connection2, _) = create_test_connection().await;
        
        pool.add_connection(connection1).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await; // Ensure different timestamps
        pool.add_connection(connection2).await.unwrap();
        
        let connections = pool.list_connections().await;
        assert_eq!(connections.len(), 2);
        
        // Should be sorted by connection time
        assert!(connections[0].connected_at <= connections[1].connected_at);
    }

    #[tokio::test]
    async fn test_connection_pool_find_idle_connections() {
        let pool = ConnectionPool::new(10);
        let (connection, _) = create_test_connection().await;
        let client_id = connection.client_id;
        
        pool.add_connection(connection).await.unwrap();
        
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
        
        pool.add_connection(connection1).await.unwrap();
        pool.add_connection(connection2).await.unwrap();
        
        assert_eq!(pool.active_connections().await, 2);
        
        let closed_count = pool.close_all_connections().await;
        assert_eq!(closed_count, 2);
        assert_eq!(pool.active_connections().await, 0);
        assert!(pool.is_empty().await);
    }

    #[tokio::test]
    async fn test_connection_pool_remove_nonexistent() {
        let pool = ConnectionPool::new(10);
        let non_existent_id = Uuid::new_v4();
        
        let result = pool.remove_connection(non_existent_id).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found in pool"));
    }

    #[tokio::test]
    async fn test_connection_pool_default() {
        let pool = ConnectionPool::default();
        assert_eq!(pool.max_connections(), 10000);
        assert!(pool.is_empty().await);
    }

    #[test]
    fn test_connection_pool_stats_structure() {
        let stats = ConnectionPoolStats {
            active_connections: 5,
            max_connections: 10,
            total_connections_accepted: 15,
            total_connections_rejected: 2,
            utilization_percentage: 50.0,
        };
        
        assert_eq!(stats.active_connections, 5);
        assert_eq!(stats.max_connections, 10);
        assert_eq!(stats.total_connections_accepted, 15);
        assert_eq!(stats.total_connections_rejected, 2);
        assert_eq!(stats.utilization_percentage, 50.0);
    }

    #[test]
    fn test_connection_info_structure() {
        let client_id = Uuid::new_v4();
        let remote_addr = "127.0.0.1:12345".parse().unwrap();
        let now = Instant::now();
        
        let info = ConnectionInfo {
            client_id,
            remote_addr,
            connected_at: now,
            last_activity: now,
            commands_processed: 42,
            connection_duration: Duration::from_secs(10),
        };
        
        assert_eq!(info.client_id, client_id);
        assert_eq!(info.remote_addr, remote_addr);
        assert_eq!(info.commands_processed, 42);
        assert_eq!(info.connection_duration, Duration::from_secs(10));
    }
}