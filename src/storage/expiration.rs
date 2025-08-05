//! Key expiration management

use std::collections::BinaryHeap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{debug, error, trace, warn};

use super::MemoryStore;
use crate::error::{Result, RustyPotatoError};

/// Expiration entry for the expiration queue
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpirationEntry {
    pub key: String,
    pub expires_at: Instant,
}

impl PartialOrd for ExpirationEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ExpirationEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering for min-heap behavior
        other.expires_at.cmp(&self.expires_at)
    }
}

/// Commands for the expiration manager
#[derive(Debug)]
pub enum ExpirationCommand {
    /// Add a key to the expiration queue
    AddExpiration { key: String, expires_at: Instant },
    /// Remove a key from the expiration queue
    RemoveExpiration { key: String },
    /// Shutdown the expiration manager
    Shutdown,
}

/// Expiration manager for handling TTL with background cleanup
pub struct ExpirationManager {
    /// Channel sender for expiration commands
    command_sender: mpsc::UnboundedSender<ExpirationCommand>,
    /// Handle to the background cleanup task
    cleanup_handle: Option<tokio::task::JoinHandle<()>>,
}

impl ExpirationManager {
    /// Create a new expiration manager with background cleanup task
    pub fn new(store: Arc<MemoryStore>, cleanup_interval: Duration) -> Self {
        let (command_sender, command_receiver) = mpsc::unbounded_channel();

        let cleanup_handle = tokio::spawn(Self::cleanup_task(
            store,
            command_receiver,
            cleanup_interval,
        ));

        Self {
            command_sender,
            cleanup_handle: Some(cleanup_handle),
        }
    }

    /// Create a new expiration manager with default cleanup interval (1 second)
    pub fn new_with_default_interval(store: Arc<MemoryStore>) -> Self {
        Self::new(store, Duration::from_secs(1))
    }

    /// Add a key to the expiration queue
    pub fn add_expiration(&self, key: String, expires_at: Instant) -> Result<()> {
        self.command_sender
            .send(ExpirationCommand::AddExpiration { key, expires_at })
            .map_err(|_| RustyPotatoError::InternalError {
                message: "Failed to send expiration command".to_string(),
                component: Some("expiration_manager".to_string()),
                source: None,
            })
    }

    /// Remove a key from the expiration queue
    pub fn remove_expiration(&self, key: String) -> Result<()> {
        self.command_sender
            .send(ExpirationCommand::RemoveExpiration { key })
            .map_err(|_| RustyPotatoError::InternalError {
                message: "Failed to send expiration command".to_string(),
                component: Some("expiration_manager".to_string()),
                source: None,
            })
    }

    /// Shutdown the expiration manager
    pub async fn shutdown(&mut self) -> Result<()> {
        // Send shutdown command
        if self.command_sender.send(ExpirationCommand::Shutdown).is_err() {
            warn!("Failed to send shutdown command to expiration manager");
        }

        // Wait for cleanup task to finish
        if let Some(handle) = self.cleanup_handle.take() {
            if let Err(e) = handle.await {
                error!("Expiration manager cleanup task failed: {}", e);
                return Err(RustyPotatoError::InternalError {
                    message: format!("Expiration manager cleanup task failed: {e}"),
                    component: Some("expiration_manager".to_string()),
                    source: None,
                });
            }
        }

        Ok(())
    }

    /// Background cleanup task that processes expiration commands and removes expired keys
    async fn cleanup_task(
        store: Arc<MemoryStore>,
        mut command_receiver: mpsc::UnboundedReceiver<ExpirationCommand>,
        cleanup_interval: Duration,
    ) {
        let mut expiration_queue = BinaryHeap::new();
        let mut interval_timer = interval(cleanup_interval);

        debug!(
            "Expiration manager cleanup task started with interval {:?}",
            cleanup_interval
        );

        loop {
            tokio::select! {
                // Process expiration commands
                command = command_receiver.recv() => {
                    match command {
                        Some(ExpirationCommand::AddExpiration { key, expires_at }) => {
                            trace!("Adding expiration for key '{}' at {:?}", key, expires_at);
                            expiration_queue.push(ExpirationEntry { key, expires_at });
                        }
                        Some(ExpirationCommand::RemoveExpiration { key }) => {
                            trace!("Removing expiration for key '{}'", key);
                            // Note: We don't actually remove from the heap here for efficiency.
                            // Instead, we'll check if the key still exists when we process it.
                            // This is a common pattern for priority queues.
                        }
                        Some(ExpirationCommand::Shutdown) => {
                            debug!("Expiration manager received shutdown command");
                            break;
                        }
                        None => {
                            warn!("Expiration command channel closed unexpectedly");
                            break;
                        }
                    }
                }

                // Periodic cleanup of expired keys
                _ = interval_timer.tick() => {
                    let now = Instant::now();
                    let mut expired_count = 0;

                    // Process expired keys from the front of the queue
                    while let Some(entry) = expiration_queue.peek() {
                        if entry.expires_at <= now {
                            let entry = expiration_queue.pop().unwrap();

                            // Check if the key still exists and has the same expiration time
                            // This handles the case where a key was updated or removed
                            match store.get(&entry.key) {
                                Ok(Some(stored_value)) => {
                                    if let Some(stored_expires_at) = stored_value.expires_at {
                                        if stored_expires_at == entry.expires_at {
                                            // Key still has the same expiration time, so delete it
                                            if let Err(e) = store.delete(&entry.key).await {
                                                error!("Failed to delete expired key '{}': {}", entry.key, e);
                                            } else {
                                                trace!("Deleted expired key '{}'", entry.key);
                                                expired_count += 1;
                                            }
                                        }
                                        // If expiration times don't match, the key was updated
                                        // and we should ignore this expiration entry
                                    }
                                    // If stored_value has no expiration, the key was persisted
                                    // and we should ignore this expiration entry
                                }
                                Ok(None) => {
                                    // Key doesn't exist anymore, nothing to do
                                    trace!("Key '{}' already removed", entry.key);
                                }
                                Err(e) => {
                                    error!("Error checking expired key '{}': {}", entry.key, e);
                                }
                            }
                        } else {
                            // No more expired keys
                            break;
                        }
                    }

                    if expired_count > 0 {
                        debug!("Cleaned up {} expired keys", expired_count);
                    }
                }
            }
        }

        debug!("Expiration manager cleanup task shutting down");
    }
}

impl Drop for ExpirationManager {
    fn drop(&mut self) {
        // Attempt to shutdown gracefully if not already done
        if self.cleanup_handle.is_some() {
            let _ = self.command_sender.send(ExpirationCommand::Shutdown);
        }
    }
}

impl Default for ExpirationManager {
    fn default() -> Self {
        // Note: This creates a manager without a store, which is not very useful
        // In practice, you should use new() or new_with_default_interval()
        let (command_sender, _) = mpsc::unbounded_channel();
        Self {
            command_sender,
            cleanup_handle: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_expiration_entry_ordering() {
        let now = Instant::now();
        let entry1 = ExpirationEntry {
            key: "key1".to_string(),
            expires_at: now + Duration::from_secs(10),
        };
        let entry2 = ExpirationEntry {
            key: "key2".to_string(),
            expires_at: now + Duration::from_secs(5),
        };

        // entry2 should come before entry1 in the heap (earlier expiration)
        // Due to reverse ordering in Ord implementation, entry2 > entry1
        assert!(entry2 > entry1);

        let mut heap = BinaryHeap::new();
        heap.push(entry1.clone());
        heap.push(entry2.clone());

        // Should pop entry2 first (earlier expiration)
        assert_eq!(heap.pop().unwrap(), entry2);
        assert_eq!(heap.pop().unwrap(), entry1);
    }

    #[tokio::test]
    async fn test_expiration_manager_creation() {
        let store = Arc::new(MemoryStore::new());
        let manager = ExpirationManager::new_with_default_interval(store);

        // Manager should be created successfully
        assert!(manager.cleanup_handle.is_some());
    }

    #[tokio::test]
    async fn test_expiration_manager_add_remove_commands() {
        let store = Arc::new(MemoryStore::new());
        let manager = ExpirationManager::new_with_default_interval(store);

        let expires_at = Instant::now() + Duration::from_secs(60);

        // Should be able to add expiration
        let result = manager.add_expiration("test_key".to_string(), expires_at);
        assert!(result.is_ok());

        // Should be able to remove expiration
        let result = manager.remove_expiration("test_key".to_string());
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_expiration_manager_background_cleanup() {
        let store = Arc::new(MemoryStore::new());
        let cleanup_interval = Duration::from_millis(50); // Fast cleanup for testing
        let mut manager = ExpirationManager::new(store.clone(), cleanup_interval);

        // Set a key with expiration that matches what we tell the manager
        let expires_at = Instant::now() + Duration::from_millis(100);
        store
            .set_with_expiration("short_lived", "value", expires_at)
            .await
            .unwrap();
        assert!(store.exists("short_lived").unwrap());

        // Add expiration to manager with the same expiration time
        manager
            .add_expiration("short_lived".to_string(), expires_at)
            .unwrap();

        // Wait for expiration and cleanup
        sleep(Duration::from_millis(200)).await;

        // Key should be expired and cleaned up
        assert!(!store.exists("short_lived").unwrap());

        // Shutdown manager
        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_expiration_manager_handles_updated_keys() {
        let store = Arc::new(MemoryStore::new());
        let cleanup_interval = Duration::from_millis(50);
        let mut manager = ExpirationManager::new(store.clone(), cleanup_interval);

        // Set a key with TTL
        let expires_at = Instant::now() + Duration::from_millis(100);
        store
            .set_with_expiration("test_key", "value1", expires_at)
            .await
            .unwrap();
        manager
            .add_expiration("test_key".to_string(), expires_at)
            .unwrap();

        // Update the key with a different expiration
        let new_expires_at = Instant::now() + Duration::from_secs(60);
        store
            .set_with_expiration("test_key", "value2", new_expires_at)
            .await
            .unwrap();

        // Wait for the original expiration time to pass
        sleep(Duration::from_millis(150)).await;

        // Key should still exist because it was updated with a new expiration
        assert!(store.exists("test_key").unwrap());
        let stored = store.get("test_key").unwrap().unwrap();
        assert_eq!(stored.value.to_string(), "value2");

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_expiration_manager_handles_deleted_keys() {
        let store = Arc::new(MemoryStore::new());
        let cleanup_interval = Duration::from_millis(50);
        let mut manager = ExpirationManager::new(store.clone(), cleanup_interval);

        // Set a key with TTL
        let expires_at = Instant::now() + Duration::from_millis(200);
        store
            .set_with_expiration("test_key", "value", expires_at)
            .await
            .unwrap();
        manager
            .add_expiration("test_key".to_string(), expires_at)
            .unwrap();

        // Delete the key manually
        store.delete("test_key").await.unwrap();

        // Wait for cleanup cycle
        sleep(Duration::from_millis(100)).await;

        // Key should remain deleted (cleanup should handle missing keys gracefully)
        assert!(!store.exists("test_key").unwrap());

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_expiration_manager_multiple_keys() {
        let store = Arc::new(MemoryStore::new());
        let cleanup_interval = Duration::from_millis(50);
        let mut manager = ExpirationManager::new(store.clone(), cleanup_interval);

        let now = Instant::now();

        // Set multiple keys with different expiration times
        for i in 0..5 {
            let key = format!("key{i}");
            let expires_at = now + Duration::from_millis(100 + i * 50);
            store
                .set_with_expiration(&key, format!("value{i}"), expires_at)
                .await
                .unwrap();
            manager.add_expiration(key, expires_at).unwrap();
        }

        assert_eq!(store.len(), 5);

        // Wait for some keys to expire
        sleep(Duration::from_millis(200)).await;

        // Some keys should be expired
        let remaining = store.len();
        assert!(remaining < 5);

        // Wait for all keys to expire
        sleep(Duration::from_millis(300)).await;

        // All keys should be expired
        assert_eq!(store.len(), 0);

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_expiration_manager_shutdown() {
        let store = Arc::new(MemoryStore::new());
        let mut manager = ExpirationManager::new_with_default_interval(store);

        // Shutdown should complete successfully
        let result = manager.shutdown().await;
        assert!(result.is_ok());

        // Second shutdown should also be safe
        let result = manager.shutdown().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_expiration_manager_drop() {
        let store = Arc::new(MemoryStore::new());
        let manager = ExpirationManager::new_with_default_interval(store);

        // Dropping should not panic
        drop(manager);

        // Give some time for cleanup
        sleep(Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn test_expiration_manager_with_persist() {
        let store = Arc::new(MemoryStore::new());
        let cleanup_interval = Duration::from_millis(50);
        let mut manager = ExpirationManager::new(store.clone(), cleanup_interval);

        // Set a key with TTL
        let expires_at = Instant::now() + Duration::from_millis(100);
        store
            .set_with_expiration("test_key", "value", expires_at)
            .await
            .unwrap();
        manager
            .add_expiration("test_key".to_string(), expires_at)
            .unwrap();

        // Persist the key (remove expiration)
        store.persist("test_key").unwrap();

        // Wait for original expiration time to pass
        sleep(Duration::from_millis(150)).await;

        // Key should still exist because it was persisted
        assert!(store.exists("test_key").unwrap());

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_expiration_manager_stress_test() {
        let store = Arc::new(MemoryStore::new());
        let cleanup_interval = Duration::from_millis(10); // Very fast cleanup
        let mut manager = ExpirationManager::new(store.clone(), cleanup_interval);

        let now = Instant::now();

        // Add many keys with various expiration times
        for i in 0..100 {
            let key = format!("key{i}");
            let expires_at = now + Duration::from_millis(50 + (i % 10) * 10);
            store
                .set_with_expiration(&key, format!("value{i}"), expires_at)
                .await
                .unwrap();
            manager.add_expiration(key, expires_at).unwrap();
        }

        assert_eq!(store.len(), 100);

        // Wait for all keys to expire
        sleep(Duration::from_millis(300)).await;

        // All keys should be expired
        assert_eq!(store.len(), 0);

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_expiration_integration_with_get_operations() {
        let store = Arc::new(MemoryStore::new());
        let cleanup_interval = Duration::from_millis(50);
        let mut manager = ExpirationManager::new(store.clone(), cleanup_interval);

        // Set a key with short expiration
        let expires_at = Instant::now() + Duration::from_millis(100);
        store
            .set_with_expiration("test_key", "value", expires_at)
            .await
            .unwrap();
        manager
            .add_expiration("test_key".to_string(), expires_at)
            .unwrap();

        // Key should exist initially
        assert!(store.exists("test_key").unwrap());
        assert!(store.get("test_key").unwrap().is_some());

        // Wait for expiration
        sleep(Duration::from_millis(150)).await;

        // Get operation should detect expiration and clean up
        assert!(store.get("test_key").unwrap().is_none());
        assert!(!store.exists("test_key").unwrap());

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_expiration_ttl_edge_cases() {
        let store = Arc::new(MemoryStore::new());
        let cleanup_interval = Duration::from_millis(50);
        let mut manager = ExpirationManager::new(store.clone(), cleanup_interval);

        // Test TTL for non-existent key
        assert_eq!(store.ttl("nonexistent").unwrap(), -2);

        // Test TTL for key without expiration
        store.set("persistent_key", "value").await.unwrap();
        assert_eq!(store.ttl("persistent_key").unwrap(), -1);

        // Test TTL for key with expiration
        let expires_at = Instant::now() + Duration::from_secs(60);
        store
            .set_with_expiration("expiring_key", "value", expires_at)
            .await
            .unwrap();
        manager
            .add_expiration("expiring_key".to_string(), expires_at)
            .unwrap();

        let ttl = store.ttl("expiring_key").unwrap();
        assert!(ttl > 0 && ttl <= 60);

        // Test TTL for expired key
        let expired_at = Instant::now() - Duration::from_secs(1);
        store
            .set_with_expiration("expired_key", "value", expired_at)
            .await
            .unwrap();
        assert_eq!(store.ttl("expired_key").unwrap(), -2); // Should be cleaned up

        manager.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_expiration_expire_method_integration() {
        let store = Arc::new(MemoryStore::new());
        let cleanup_interval = Duration::from_millis(50);
        let mut manager = ExpirationManager::new(store.clone(), cleanup_interval);

        // Set a key without expiration
        store.set("test_key", "value").await.unwrap();
        assert_eq!(store.ttl("test_key").unwrap(), -1);

        // Set expiration using expire method
        assert!(store.expire("test_key", 1).await.unwrap()); // 1 second TTL

        let ttl = store.ttl("test_key").unwrap();
        // TTL should be close to 1 second, but allow for some timing variance
        assert!(
            (0..=1).contains(&ttl),
            "Expected TTL between 0 and 1, got {ttl}"
        );

        // Try to expire non-existent key
        assert!(!store.expire("nonexistent", 60).await.unwrap());

        manager.shutdown().await.unwrap();
    }
}
