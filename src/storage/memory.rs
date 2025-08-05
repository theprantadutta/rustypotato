//! In-memory storage implementation using DashMap

use crate::error::{Result, RustyPotatoError};
use crate::storage::persistence::{AofCommand, PersistenceManager};
use dashmap::DashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

/// Value types supported by RustyPotato
#[derive(Debug, Clone, PartialEq)]
pub enum ValueType {
    String(String),
    Integer(i64),
}

impl ValueType {
    /// Convert value to string representation
    pub fn as_string(&self) -> String {
        match self {
            ValueType::String(s) => s.clone(),
            ValueType::Integer(i) => i.to_string(),
        }
    }

    /// Try to convert value to integer
    pub fn to_integer(&self) -> Result<i64> {
        match self {
            ValueType::Integer(i) => Ok(*i),
            ValueType::String(s) => s
                .parse::<i64>()
                .map_err(|_| RustyPotatoError::NotAnInteger { value: s.clone() }),
        }
    }

    /// Check if value is a string type
    pub fn is_string(&self) -> bool {
        matches!(self, ValueType::String(_))
    }

    /// Check if value is an integer type
    pub fn is_integer(&self) -> bool {
        matches!(self, ValueType::Integer(_))
    }

    /// Get the type name as a string
    pub fn type_name(&self) -> &'static str {
        match self {
            ValueType::String(_) => "string",
            ValueType::Integer(_) => "integer",
        }
    }
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_string())
    }
}

impl From<String> for ValueType {
    fn from(s: String) -> Self {
        ValueType::String(s)
    }
}

impl From<&str> for ValueType {
    fn from(s: &str) -> Self {
        ValueType::String(s.to_string())
    }
}

impl From<i64> for ValueType {
    fn from(i: i64) -> Self {
        ValueType::Integer(i)
    }
}

impl From<i32> for ValueType {
    fn from(i: i32) -> Self {
        ValueType::Integer(i as i64)
    }
}

/// Stored value with metadata
#[derive(Debug, Clone)]
pub struct StoredValue {
    pub value: ValueType,
    pub created_at: Instant,
    pub last_accessed: Instant,
    pub expires_at: Option<Instant>,
}

/// High-performance concurrent in-memory store
#[derive(Debug)]
pub struct MemoryStore {
    data: DashMap<String, StoredValue>,
    expiration_index: DashMap<String, Instant>,
    persistence: Option<Arc<PersistenceManager>>,
}

impl MemoryStore {
    /// Create a new memory store
    pub fn new() -> Self {
        Self {
            data: DashMap::new(),
            expiration_index: DashMap::new(),
            persistence: None,
        }
    }

    /// Create a new memory store with persistence
    pub fn with_persistence(persistence: Arc<PersistenceManager>) -> Self {
        Self {
            data: DashMap::new(),
            expiration_index: DashMap::new(),
            persistence: Some(persistence),
        }
    }

    /// Recover data from persistence layer
    pub async fn recover_from_persistence(&self) -> Result<usize> {
        if let Some(ref persistence) = self.persistence {
            let entries = persistence.recover().await?;
            let mut recovered_count = 0;

            for entry in entries {
                match entry.command {
                    AofCommand::Set { key, value } => {
                        let stored_value = StoredValue::new(value);
                        self.data.insert(key, stored_value);
                        recovered_count += 1;
                    }
                    AofCommand::SetWithExpiration {
                        key,
                        value,
                        expires_at,
                    } => {
                        let expires_instant = self.timestamp_to_instant(expires_at);
                        if expires_instant > Instant::now() {
                            let stored_value =
                                StoredValue::new_with_expiration(value, expires_instant);
                            self.expiration_index.insert(key.clone(), expires_instant);
                            self.data.insert(key, stored_value);
                            recovered_count += 1;
                        }
                        // Skip expired keys during recovery
                    }
                    AofCommand::Delete { key } => {
                        self.data.remove(&key);
                        self.expiration_index.remove(&key);
                    }
                    AofCommand::Expire { key, expires_at } => {
                        let expires_instant = self.timestamp_to_instant(expires_at);
                        if let Some(mut entry) = self.data.get_mut(&key) {
                            if expires_instant > Instant::now() {
                                entry.set_expiration(expires_instant);
                                self.expiration_index.insert(key, expires_instant);
                            } else {
                                // Key should be expired, remove it
                                drop(entry);
                                self.data.remove(&key);
                                self.expiration_index.remove(&key);
                            }
                        }
                    }
                }
            }

            tracing::info!("Recovered {} keys from persistence", recovered_count);
            Ok(recovered_count)
        } else {
            Ok(0)
        }
    }

    /// Convert timestamp to Instant (helper method)
    fn timestamp_to_instant(&self, timestamp: u64) -> Instant {
        use std::time::{Duration, SystemTime, UNIX_EPOCH};

        let now = Instant::now();
        let system_now = SystemTime::now();
        let target_system_time = UNIX_EPOCH + Duration::from_secs(timestamp);

        if target_system_time > system_now {
            // Future time
            let duration_from_now = target_system_time
                .duration_since(system_now)
                .unwrap_or_default();
            now + duration_from_now
        } else {
            // Past time (treat as expired)
            now - Duration::from_secs(1)
        }
    }

    /// Get the number of keys in the store
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the store is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Set a key-value pair in the store
    pub async fn set<K, V>(&self, key: K, value: V) -> Result<()>
    where
        K: Into<String>,
        V: Into<ValueType>,
    {
        let key_string = key.into();
        let value_type = value.into();
        let stored_value = StoredValue::new(value_type.clone());

        // Remove from expiration index if it exists
        self.expiration_index.remove(&key_string);

        // Insert the new value
        self.data.insert(key_string.clone(), stored_value);

        // Log to persistence
        if let Some(ref persistence) = self.persistence {
            persistence.log_set(key_string, value_type).await?;
        }

        Ok(())
    }

    /// Set a key-value pair with TTL in seconds
    pub async fn set_with_ttl<K, V>(&self, key: K, value: V, ttl_seconds: u64) -> Result<()>
    where
        K: Into<String>,
        V: Into<ValueType>,
    {
        let key_string = key.into();
        let value_type = value.into();
        let stored_value = StoredValue::new_with_ttl(value_type.clone(), ttl_seconds);
        let expires_at = stored_value.expires_at.unwrap();

        // Update expiration index
        self.expiration_index.insert(key_string.clone(), expires_at);

        // Insert the new value
        self.data.insert(key_string.clone(), stored_value);

        // Log to persistence
        if let Some(ref persistence) = self.persistence {
            persistence
                .log_set_with_expiration(key_string, value_type, expires_at)
                .await?;
        }

        Ok(())
    }

    /// Set a key-value pair with specific expiration time
    pub async fn set_with_expiration<K, V>(
        &self,
        key: K,
        value: V,
        expires_at: Instant,
    ) -> Result<()>
    where
        K: Into<String>,
        V: Into<ValueType>,
    {
        let key_string = key.into();
        let value_type = value.into();
        let stored_value = StoredValue::new_with_expiration(value_type.clone(), expires_at);

        // Update expiration index
        self.expiration_index.insert(key_string.clone(), expires_at);

        // Insert the new value
        self.data.insert(key_string.clone(), stored_value);

        // Log to persistence
        if let Some(ref persistence) = self.persistence {
            persistence
                .log_set_with_expiration(key_string, value_type, expires_at)
                .await?;
        }

        Ok(())
    }

    /// Get a value from the store
    pub fn get<K>(&self, key: K) -> Result<Option<StoredValue>>
    where
        K: AsRef<str>,
    {
        let key_str = key.as_ref();

        // Check if key exists and is not expired
        if let Some(mut entry) = self.data.get_mut(key_str) {
            if entry.is_expired() {
                // Remove expired key
                drop(entry); // Release the lock before removing
                self.delete_sync(key_str)?;
                return Ok(None);
            }

            // Update last accessed time
            entry.touch();
            Ok(Some(entry.clone()))
        } else {
            Ok(None)
        }
    }

    /// Delete a key from the store
    pub async fn delete<K>(&self, key: K) -> Result<bool>
    where
        K: AsRef<str>,
    {
        let key_str = key.as_ref();

        // Check if key exists and is not expired first
        if let Some(entry) = self.data.get(key_str) {
            if entry.is_expired() {
                // Remove expired key automatically and return false (key didn't exist)
                drop(entry); // Release the lock before removing
                self.expiration_index.remove(key_str);
                self.data.remove(key_str);
                return Ok(false);
            }
        }

        // Remove from expiration index
        self.expiration_index.remove(key_str);

        // Remove from main data store
        let was_removed = self.data.remove(key_str).is_some();

        // Log to persistence if key was actually removed
        if was_removed {
            if let Some(ref persistence) = self.persistence {
                persistence.log_delete(key_str.to_string()).await?;
            }
        }

        Ok(was_removed)
    }

    /// Internal synchronous delete method for use in sync contexts
    fn delete_sync<K>(&self, key: K) -> Result<bool>
    where
        K: AsRef<str>,
    {
        let key_str = key.as_ref();

        // Check if key exists and is not expired first
        if let Some(entry) = self.data.get(key_str) {
            if entry.is_expired() {
                // Remove expired key automatically and return false (key didn't exist)
                drop(entry); // Release the lock before removing
                self.expiration_index.remove(key_str);
                self.data.remove(key_str);
                return Ok(false);
            }
        }

        // Remove from expiration index
        self.expiration_index.remove(key_str);

        // Remove from main data store
        let was_removed = self.data.remove(key_str).is_some();

        Ok(was_removed)
    }

    /// Check if a key exists in the store
    pub fn exists<K>(&self, key: K) -> Result<bool>
    where
        K: AsRef<str>,
    {
        let key_str = key.as_ref();

        if let Some(entry) = self.data.get(key_str) {
            if entry.is_expired() {
                // Remove expired key
                drop(entry); // Release the lock before removing
                self.delete_sync(key_str)?;
                return Ok(false);
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Set expiration for an existing key
    pub async fn expire<K>(&self, key: K, ttl_seconds: u64) -> Result<bool>
    where
        K: AsRef<str>,
    {
        let key_str = key.as_ref();

        if let Some(mut entry) = self.data.get_mut(key_str) {
            if entry.is_expired() {
                // Remove expired key
                drop(entry); // Release the lock before removing
                self.delete(key_str).await?;
                return Ok(false);
            }

            let expires_at = Instant::now() + std::time::Duration::from_secs(ttl_seconds);
            entry.set_expiration(expires_at);

            // Update expiration index
            self.expiration_index
                .insert(key_str.to_string(), expires_at);

            // Log to persistence
            if let Some(ref persistence) = self.persistence {
                persistence
                    .log_expire(key_str.to_string(), expires_at)
                    .await?;
            }

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get TTL for a key in seconds
    pub fn ttl<K>(&self, key: K) -> Result<i64>
    where
        K: AsRef<str>,
    {
        let key_str = key.as_ref();

        if let Some(entry) = self.data.get(key_str) {
            if entry.is_expired() {
                // Remove expired key
                drop(entry); // Release the lock before removing
                self.delete_sync(key_str)?;
                return Ok(-2); // Key doesn't exist
            }

            Ok(entry.ttl_seconds().unwrap_or(-1)) // -1 means no expiration
        } else {
            Ok(-2) // Key doesn't exist
        }
    }

    /// Remove expiration from a key
    pub fn persist<K>(&self, key: K) -> Result<bool>
    where
        K: AsRef<str>,
    {
        let key_str = key.as_ref();

        if let Some(mut entry) = self.data.get_mut(key_str) {
            if entry.is_expired() {
                // Remove expired key
                drop(entry); // Release the lock before removing
                self.delete_sync(key_str)?;
                return Ok(false);
            }

            let had_expiration = entry.expires_at.is_some();
            entry.remove_expiration();

            // Remove from expiration index
            self.expiration_index.remove(key_str);

            Ok(had_expiration)
        } else {
            Ok(false)
        }
    }

    /// Get all keys in the store (for debugging/admin purposes)
    pub fn keys(&self) -> Vec<String> {
        self.data.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Clear all data from the store
    pub fn clear(&self) {
        self.data.clear();
        self.expiration_index.clear();
    }

    /// Atomically increment a key's integer value by 1
    /// If key doesn't exist, initialize it to 1
    /// Returns the new value after increment
    pub async fn incr<K>(&self, key: K) -> Result<i64>
    where
        K: Into<String>,
    {
        self.incr_by(key, 1).await
    }

    /// Atomically decrement a key's integer value by 1
    /// If key doesn't exist, initialize it to -1
    /// Returns the new value after decrement
    pub async fn decr<K>(&self, key: K) -> Result<i64>
    where
        K: Into<String>,
    {
        self.incr_by(key, -1).await
    }

    /// Atomically increment a key's integer value by the specified amount
    /// If key doesn't exist, initialize it to the increment value
    /// Returns the new value after increment
    pub async fn incr_by<K>(&self, key: K, increment: i64) -> Result<i64>
    where
        K: Into<String>,
    {
        let key_string = key.into();

        // Use entry API for atomic operation
        let result = match self.data.entry(key_string.clone()) {
            dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                // Check if expired first
                if entry.get().is_expired() {
                    // Remove expired key and treat as new
                    entry.remove();
                    self.expiration_index.remove(&key_string);

                    // Insert new value with increment
                    let new_value = increment;
                    let stored_value = StoredValue::new(ValueType::Integer(new_value));
                    self.data.insert(key_string.clone(), stored_value);
                    new_value
                } else {
                    // Try to convert existing value to integer and increment
                    let current_value = entry.get().value.to_integer()?;
                    let new_value = current_value.checked_add(increment).ok_or(
                        RustyPotatoError::NotAnInteger {
                            value: format!("overflow adding {increment} to {current_value}"),
                        },
                    )?;

                    // Update the value in place
                    let mut stored_value = entry.get().clone();
                    stored_value.value = ValueType::Integer(new_value);
                    stored_value.touch();
                    entry.insert(stored_value);

                    new_value
                }
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                // Key doesn't exist, initialize with increment value
                let new_value = increment;
                let stored_value = StoredValue::new(ValueType::Integer(new_value));
                entry.insert(stored_value);
                new_value
            }
        };

        // Log to persistence
        if let Some(ref persistence) = self.persistence {
            persistence
                .log_set(key_string, ValueType::Integer(result))
                .await?;
        }

        Ok(result)
    }

    /// Get memory usage statistics
    pub fn memory_stats(&self) -> MemoryStats {
        let key_count = self.data.len();
        let expiration_count = self.expiration_index.len();

        // Rough estimation of memory usage
        let estimated_memory = key_count * 100; // Rough estimate per key-value pair

        MemoryStats {
            key_count,
            expiration_count,
            estimated_memory_bytes: estimated_memory,
        }
    }

    /// Internal method to delete a key without expiration checking
    fn delete_internal(&self, key: &str) -> bool {
        // Remove from expiration index
        self.expiration_index.remove(key);

        // Remove from main data store
        self.data.remove(key).is_some()
    }

    /// Clean up expired keys (should be called periodically)
    pub fn cleanup_expired(&self) -> usize {
        let mut removed_count = 0;
        let now = Instant::now();

        // Collect expired keys to avoid holding locks during iteration
        let expired_keys: Vec<String> = self
            .expiration_index
            .iter()
            .filter_map(|entry| {
                if *entry.value() <= now {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();

        // Remove expired keys using internal delete to avoid expiration check
        for key in expired_keys {
            if self.delete_internal(&key) {
                removed_count += 1;
            }
        }

        removed_count
    }
}

/// Memory usage statistics
#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub key_count: usize,
    pub expiration_count: usize,
    pub estimated_memory_bytes: usize,
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl StoredValue {
    /// Create a new stored value
    pub fn new(value: ValueType) -> Self {
        let now = Instant::now();
        Self {
            value,
            created_at: now,
            last_accessed: now,
            expires_at: None,
        }
    }

    /// Create a new stored value with expiration
    pub fn new_with_expiration(value: ValueType, expires_at: Instant) -> Self {
        let now = Instant::now();
        Self {
            value,
            created_at: now,
            last_accessed: now,
            expires_at: Some(expires_at),
        }
    }

    /// Create a new stored value with TTL in seconds
    pub fn new_with_ttl(value: ValueType, ttl_seconds: u64) -> Self {
        let now = Instant::now();
        let expires_at = now + std::time::Duration::from_secs(ttl_seconds);
        Self {
            value,
            created_at: now,
            last_accessed: now,
            expires_at: Some(expires_at),
        }
    }

    /// Check if the value has expired
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires| Instant::now() > expires)
    }

    /// Update last accessed time
    pub fn touch(&mut self) {
        self.last_accessed = Instant::now();
    }

    /// Set expiration time
    pub fn set_expiration(&mut self, expires_at: Instant) {
        self.expires_at = Some(expires_at);
    }

    /// Set TTL in seconds
    pub fn set_ttl(&mut self, ttl_seconds: u64) {
        let expires_at = Instant::now() + std::time::Duration::from_secs(ttl_seconds);
        self.expires_at = Some(expires_at);
    }

    /// Remove expiration
    pub fn remove_expiration(&mut self) {
        self.expires_at = None;
    }

    /// Get remaining TTL in seconds, returns None if no expiration set
    pub fn ttl_seconds(&self) -> Option<i64> {
        self.expires_at.map(|expires| {
            let now = Instant::now();
            if expires > now {
                (expires - now).as_secs() as i64
            } else {
                -2 // Expired
            }
        })
    }

    /// Get age of the value in seconds
    pub fn age_seconds(&self) -> u64 {
        self.created_at.elapsed().as_secs()
    }

    /// Get time since last access in seconds
    pub fn idle_time_seconds(&self) -> u64 {
        self.last_accessed.elapsed().as_secs()
    }

    /// Get the value as a string
    pub fn as_string(&self) -> String {
        self.value.as_string()
    }

    /// Try to get the value as an integer
    pub fn as_integer(&self) -> Result<i64> {
        self.value.to_integer()
    }

    /// Get the value type
    pub fn value_type(&self) -> &ValueType {
        &self.value
    }

    /// Get the type name
    pub fn type_name(&self) -> &'static str {
        self.value.type_name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    // Value type tests
    #[test]
    fn test_value_type_string_creation() {
        let value = ValueType::String("hello".to_string());
        assert!(value.is_string());
        assert!(!value.is_integer());
        assert_eq!(value.as_string(), "hello");
        assert_eq!(value.type_name(), "string");
    }

    #[test]
    fn test_value_type_integer_creation() {
        let value = ValueType::Integer(42);
        assert!(!value.is_string());
        assert!(value.is_integer());
        assert_eq!(value.as_string(), "42");
        assert_eq!(value.type_name(), "integer");
    }

    #[test]
    fn test_value_type_conversions() {
        // String to integer conversion
        let string_value = ValueType::String("123".to_string());
        assert_eq!(string_value.to_integer().unwrap(), 123);

        let invalid_string = ValueType::String("not_a_number".to_string());
        assert!(invalid_string.to_integer().is_err());

        // Integer to string conversion
        let int_value = ValueType::Integer(456);
        assert_eq!(int_value.to_string(), "456");
        assert_eq!(int_value.to_integer().unwrap(), 456);
    }

    #[test]
    fn test_value_type_from_conversions() {
        let from_string: ValueType = "test".into();
        assert_eq!(from_string, ValueType::String("test".to_string()));

        let from_string_owned: ValueType = "test".to_string().into();
        assert_eq!(from_string_owned, ValueType::String("test".to_string()));

        let from_i64: ValueType = 42i64.into();
        assert_eq!(from_i64, ValueType::Integer(42));

        let from_i32: ValueType = 42i32.into();
        assert_eq!(from_i32, ValueType::Integer(42));
    }

    #[test]
    fn test_value_type_display() {
        let string_value = ValueType::String("hello".to_string());
        assert_eq!(format!("{string_value}"), "hello");

        let int_value = ValueType::Integer(42);
        assert_eq!(format!("{int_value}"), "42");
    }

    // StoredValue tests
    #[test]
    fn test_stored_value_creation() {
        let value = ValueType::String("test".to_string());
        let stored = StoredValue::new(value.clone());

        assert_eq!(stored.value, value);
        assert!(stored.expires_at.is_none());
        assert!(!stored.is_expired());

        // Check that created_at and last_accessed are recent
        let now = Instant::now();
        assert!(stored.created_at <= now);
        assert!(stored.last_accessed <= now);
        assert!(now.duration_since(stored.created_at) < Duration::from_millis(100));
    }

    #[test]
    fn test_stored_value_with_expiration() {
        let value = ValueType::String("test".to_string());
        let expires_at = Instant::now() + Duration::from_secs(60);
        let stored = StoredValue::new_with_expiration(value.clone(), expires_at);

        assert_eq!(stored.value, value);
        assert_eq!(stored.expires_at, Some(expires_at));
        assert!(!stored.is_expired());
    }

    #[test]
    fn test_stored_value_with_ttl() {
        let value = ValueType::String("test".to_string());
        let stored = StoredValue::new_with_ttl(value.clone(), 60);

        assert_eq!(stored.value, value);
        assert!(stored.expires_at.is_some());
        assert!(!stored.is_expired());

        // TTL should be approximately 60 seconds
        let ttl = stored.ttl_seconds().unwrap();
        assert!((59..=60).contains(&ttl));
    }

    #[test]
    fn test_stored_value_expiration() {
        let value = ValueType::String("test".to_string());
        let expires_at = Instant::now() - Duration::from_secs(1); // Already expired
        let stored = StoredValue::new_with_expiration(value, expires_at);

        assert!(stored.is_expired());
        assert_eq!(stored.ttl_seconds().unwrap(), -2); // Expired
    }

    #[test]
    fn test_stored_value_touch() {
        let value = ValueType::String("test".to_string());
        let mut stored = StoredValue::new(value);
        let original_access_time = stored.last_accessed;

        // Wait a small amount to ensure time difference
        std::thread::sleep(Duration::from_millis(1));
        stored.touch();

        assert!(stored.last_accessed > original_access_time);
    }

    // MemoryStore basic operation tests
    #[test]
    fn test_memory_store_creation() {
        let store = MemoryStore::new();
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());

        let default_store = MemoryStore::default();
        assert_eq!(default_store.len(), 0);
        assert!(default_store.is_empty());
    }

    #[tokio::test]
    async fn test_memory_store_set_and_get() {
        let store = MemoryStore::new();

        // Set a string value
        store.set("key1", "value1").await.unwrap();
        assert_eq!(store.len(), 1);
        assert!(!store.is_empty());

        // Get the value
        let retrieved = store.get("key1").unwrap().unwrap();
        assert_eq!(retrieved.value.to_string(), "value1");

        // Set an integer value
        store.set("key2", 42i64).await.unwrap();
        assert_eq!(store.len(), 2);

        let retrieved_int = store.get("key2").unwrap().unwrap();
        assert_eq!(retrieved_int.value.to_integer().unwrap(), 42);
    }

    #[tokio::test]
    async fn test_memory_store_overwrite() {
        let store = MemoryStore::new();

        // Set initial value
        store.set("key1", "value1").await.unwrap();
        assert_eq!(store.len(), 1);

        // Overwrite with new value
        store.set("key1", "value2").await.unwrap();
        assert_eq!(store.len(), 1); // Should still be 1

        let retrieved = store.get("key1").unwrap().unwrap();
        assert_eq!(retrieved.value.to_string(), "value2");
    }

    #[test]
    fn test_memory_store_get_nonexistent() {
        let store = MemoryStore::new();

        let result = store.get("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_memory_store_delete() {
        let store = MemoryStore::new();

        // Set a value
        store.set("key1", "value1").await.unwrap();
        assert_eq!(store.len(), 1);

        // Delete the value
        let deleted = store.delete("key1").await.unwrap();
        assert!(deleted);
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());

        // Try to delete non-existent key
        let not_deleted = store.delete("nonexistent").await.unwrap();
        assert!(!not_deleted);
    }

    #[tokio::test]
    async fn test_memory_store_exists() {
        let store = MemoryStore::new();

        // Check non-existent key
        assert!(!store.exists("key1").unwrap());

        // Set a value
        store.set("key1", "value1").await.unwrap();

        // Check existing key
        assert!(store.exists("key1").unwrap());

        // Delete and check again
        store.delete("key1").await.unwrap();
        assert!(!store.exists("key1").unwrap());
    }

    #[tokio::test]
    async fn test_memory_store_ttl_operations() {
        let store = MemoryStore::new();

        // Set value with TTL
        store.set_with_ttl("key1", "value1", 60).await.unwrap();
        assert_eq!(store.len(), 1);

        // Check TTL
        let ttl = store.ttl("key1").unwrap();
        assert!((59..=60).contains(&ttl));

        // Set expiration on existing key
        store.set("key2", "value2").await.unwrap();
        let expired = store.expire("key2", 30).await.unwrap();
        assert!(expired);

        let ttl2 = store.ttl("key2").unwrap();
        assert!((29..=30).contains(&ttl2));

        // Try to expire non-existent key
        let not_expired = store.expire("nonexistent", 30).await.unwrap();
        assert!(!not_expired);
    }

    #[tokio::test]
    async fn test_memory_store_persist() {
        let store = MemoryStore::new();

        // Set value with TTL
        store.set_with_ttl("key1", "value1", 60).await.unwrap();

        // Remove expiration
        let persisted = store.persist("key1").unwrap();
        assert!(persisted);

        // Check that TTL is now -1 (no expiration)
        let ttl = store.ttl("key1").unwrap();
        assert_eq!(ttl, -1);

        // Try to persist key without expiration
        store.set("key2", "value2").await.unwrap();
        let not_persisted = store.persist("key2").unwrap();
        assert!(!not_persisted);
    }

    #[tokio::test]
    async fn test_memory_store_expiration_handling() {
        let store = MemoryStore::new();

        // Set value with very short TTL
        let expires_at = Instant::now() + Duration::from_millis(1);
        store
            .set_with_expiration("key1", "value1", expires_at)
            .await
            .unwrap();

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Try to get expired key - should return None and clean up
        let result = store.get("key1").unwrap();
        assert!(result.is_none());
        assert_eq!(store.len(), 0);

        // exists() should also handle expiration
        store
            .set_with_expiration("key2", "value2", Instant::now() + Duration::from_millis(1))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;

        let exists = store.exists("key2").unwrap();
        assert!(!exists);
        assert_eq!(store.len(), 0);
    }

    #[tokio::test]
    async fn test_memory_store_cleanup_expired() {
        let store = MemoryStore::new();

        // Set some values with short TTL
        let expires_at = Instant::now() + Duration::from_millis(1);
        store
            .set_with_expiration("key1", "value1", expires_at)
            .await
            .unwrap();
        store
            .set_with_expiration("key2", "value2", expires_at)
            .await
            .unwrap();
        store.set("key3", "value3").await.unwrap(); // No expiration

        assert_eq!(store.len(), 3);

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Clean up expired keys
        let removed_count = store.cleanup_expired();
        assert_eq!(removed_count, 2);
        assert_eq!(store.len(), 1);

        // Only key3 should remain
        assert!(store.exists("key3").unwrap());
        assert!(!store.exists("key1").unwrap());
        assert!(!store.exists("key2").unwrap());
    }

    #[tokio::test]
    async fn test_memory_store_keys_and_clear() {
        let store = MemoryStore::new();

        // Set some values
        store.set("key1", "value1").await.unwrap();
        store.set("key2", "value2").await.unwrap();
        store.set("key3", "value3").await.unwrap();

        // Get all keys
        let keys = store.keys();
        assert_eq!(keys.len(), 3);
        assert!(keys.contains(&"key1".to_string()));
        assert!(keys.contains(&"key2".to_string()));
        assert!(keys.contains(&"key3".to_string()));

        // Clear all data
        store.clear();
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());
        assert!(store.keys().is_empty());
    }

    #[tokio::test]
    async fn test_memory_store_memory_stats() {
        let store = MemoryStore::new();

        // Initial stats
        let stats = store.memory_stats();
        assert_eq!(stats.key_count, 0);
        assert_eq!(stats.expiration_count, 0);

        // Add some data
        store.set("key1", "value1").await.unwrap();
        store.set_with_ttl("key2", "value2", 60).await.unwrap();

        let stats = store.memory_stats();
        assert_eq!(stats.key_count, 2);
        assert_eq!(stats.expiration_count, 1);
        assert!(stats.estimated_memory_bytes > 0);
    }

    // Concurrent access tests
    #[tokio::test]
    async fn test_concurrent_set_get() {
        let store = Arc::new(MemoryStore::new());
        let mut handles = vec![];

        // Spawn multiple tasks that set and get values
        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            let handle = tokio::spawn(async move {
                let key = format!("key{i}");
                let value = format!("value{i}");

                // Set value
                store_clone.set(&key, value.as_str()).await.unwrap();

                // Get value back
                let retrieved = store_clone.get(&key).unwrap().unwrap();
                assert_eq!(retrieved.value.to_string(), value);
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all values are present
        assert_eq!(store.len(), 10);
        for i in 0..10 {
            let key = format!("key{i}");
            let expected_value = format!("value{i}");
            let retrieved = store.get(&key).unwrap().unwrap();
            assert_eq!(retrieved.value.to_string(), expected_value);
        }
    }

    #[tokio::test]
    async fn test_concurrent_same_key_access() {
        let store = Arc::new(MemoryStore::new());
        let mut handles = vec![];

        // Multiple tasks accessing the same key
        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            let handle = tokio::spawn(async move {
                let value = format!("value{i}");

                // Set value (last one wins)
                store_clone.set("shared_key", value.as_str()).await.unwrap();

                // Try to get the value
                let _retrieved = store_clone.get("shared_key").unwrap();
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Should have exactly one key
        assert_eq!(store.len(), 1);
        assert!(store.exists("shared_key").unwrap());
    }

    #[tokio::test]
    async fn test_concurrent_delete_operations() {
        let store = Arc::new(MemoryStore::new());

        // Pre-populate with data
        for i in 0..20 {
            let key = format!("key{i}");
            let value = format!("value{i}");
            store.set(&key, value.as_str()).await.unwrap();
        }

        assert_eq!(store.len(), 20);

        let mut handles = vec![];

        // Spawn tasks to delete half the keys
        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            let handle = tokio::spawn(async move {
                let key = format!("key{i}");
                store_clone.delete(&key).await.unwrap();
            });
            handles.push(handle);
        }

        // Wait for all deletions to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Should have 10 keys remaining
        assert_eq!(store.len(), 10);

        // Verify which keys remain
        for i in 0..20 {
            let key = format!("key{i}");
            let exists = store.exists(&key).unwrap();
            if i < 10 {
                assert!(!exists, "Key {key} should have been deleted");
            } else {
                assert!(exists, "Key {key} should still exist");
            }
        }
    }

    #[tokio::test]
    async fn test_concurrent_expiration_operations() {
        let store = Arc::new(MemoryStore::new());
        let mut handles = vec![];

        // Pre-populate with data
        for i in 0..10 {
            let key = format!("key{i}");
            let value = format!("value{i}");
            store.set(&key, value.as_str()).await.unwrap();
        }

        // Spawn tasks to set expiration on keys
        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            let handle = tokio::spawn(async move {
                let key = format!("key{i}");
                let ttl = 60 + i as u64; // Different TTL for each key
                let _expired = store_clone.expire(&key, ttl).await.unwrap();
            });
            handles.push(handle);
        }

        // Wait for all operations to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all keys have expiration set
        for i in 0..10 {
            let key = format!("key{i}");
            let ttl = store.ttl(&key).unwrap();
            assert!(ttl > 0, "Key {key} should have positive TTL");
        }
    }

    #[tokio::test]
    async fn test_concurrent_mixed_operations() {
        let store = Arc::new(MemoryStore::new());
        let mut handles = vec![];

        // Spawn tasks doing different operations
        for i in 0..20 {
            let store_clone = Arc::clone(&store);
            let handle = tokio::spawn(async move {
                let key = format!("key{}", i % 10); // Use 10 different keys

                match i % 4 {
                    0 => {
                        // Set operation
                        let value = format!("value{i}");
                        store_clone.set(&key, value.as_str()).await.unwrap();
                    }
                    1 => {
                        // Get operation
                        let _result = store_clone.get(&key).unwrap();
                    }
                    2 => {
                        // Exists operation
                        let _exists = store_clone.exists(&key).unwrap();
                    }
                    3 => {
                        // Expire operation
                        let _expired = store_clone.expire(&key, 60).await.unwrap();
                    }
                    _ => unreachable!(),
                }
            });
            handles.push(handle);
        }

        // Wait for all operations to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Store should be in a consistent state
        assert!(store.len() <= 10); // At most 10 keys

        // All operations should complete without panics
        let stats = store.memory_stats();
        assert!(stats.key_count <= 10);
    }

    #[tokio::test]
    async fn test_concurrent_cleanup_expired() {
        let store = Arc::new(MemoryStore::new());

        // Set values with very short TTL
        for i in 0..20 {
            let key = format!("key{i}");
            let value = format!("value{i}");
            let expires_at = Instant::now() + Duration::from_millis(1);
            store
                .set_with_expiration(&key, value.as_str(), expires_at)
                .await
                .unwrap();
        }

        assert_eq!(store.len(), 20);

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(10)).await;

        let mut handles = vec![];

        // Spawn multiple tasks to clean up expired keys
        for _ in 0..5 {
            let store_clone = Arc::clone(&store);
            let handle = tokio::spawn(async move { store_clone.cleanup_expired() });
            handles.push(handle);
        }

        // Wait for all cleanup operations to complete
        let mut total_removed = 0;
        for handle in handles {
            total_removed += handle.await.unwrap();
        }

        // All keys should be removed (some tasks might not find any to remove)
        assert!(total_removed <= 20);
        assert_eq!(store.len(), 0);
    }

    // Edge case tests
    #[test]
    fn test_value_type_edge_cases() {
        // Test empty string
        let empty_string = ValueType::String("".to_string());
        assert_eq!(empty_string.to_string(), "");
        assert!(empty_string.to_integer().is_err());

        // Test zero integer
        let zero_int = ValueType::Integer(0);
        assert_eq!(zero_int.to_string(), "0");
        assert_eq!(zero_int.to_integer().unwrap(), 0);

        // Test negative integer
        let negative_int = ValueType::Integer(-42);
        assert_eq!(negative_int.to_string(), "-42");
        assert_eq!(negative_int.to_integer().unwrap(), -42);

        // Test string with negative number
        let negative_string = ValueType::String("-123".to_string());
        assert_eq!(negative_string.to_integer().unwrap(), -123);
    }

    #[test]
    fn test_stored_value_metadata_consistency() {
        let value = ValueType::String("test".to_string());
        let stored = StoredValue::new(value);

        // created_at should be <= last_accessed (they're set to the same time)
        assert!(stored.created_at <= stored.last_accessed);

        // Both should be very recent
        let now = Instant::now();
        assert!(now.duration_since(stored.created_at) < Duration::from_millis(100));
        assert!(now.duration_since(stored.last_accessed) < Duration::from_millis(100));
    }

    // Atomic operation tests
    #[tokio::test]
    async fn test_incr_new_key() {
        let store = MemoryStore::new();

        let result = store.incr("new_key").await.unwrap();
        assert_eq!(result, 1);

        let stored = store.get("new_key").unwrap().unwrap();
        assert_eq!(stored.value.to_integer().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_incr_existing_integer() {
        let store = MemoryStore::new();

        store.set("key1", 5i64).await.unwrap();
        let result = store.incr("key1").await.unwrap();
        assert_eq!(result, 6);

        let stored = store.get("key1").unwrap().unwrap();
        assert_eq!(stored.value.to_integer().unwrap(), 6);
    }

    #[tokio::test]
    async fn test_incr_existing_string_number() {
        let store = MemoryStore::new();

        store.set("key1", "42").await.unwrap();
        let result = store.incr("key1").await.unwrap();
        assert_eq!(result, 43);

        let stored = store.get("key1").unwrap().unwrap();
        assert_eq!(stored.value.to_integer().unwrap(), 43);
    }

    #[tokio::test]
    async fn test_incr_non_numeric_string() {
        let store = MemoryStore::new();

        store.set("key1", "not_a_number").await.unwrap();
        let result = store.incr("key1").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RustyPotatoError::NotAnInteger { .. }
        ));
    }

    #[tokio::test]
    async fn test_decr_new_key() {
        let store = MemoryStore::new();

        let result = store.decr("new_key").await.unwrap();
        assert_eq!(result, -1);

        let stored = store.get("new_key").unwrap().unwrap();
        assert_eq!(stored.value.to_integer().unwrap(), -1);
    }
}
