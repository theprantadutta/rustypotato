//! Comprehensive unit tests for storage layer components
//!
//! Tests cover MemoryStore, ValueType, StoredValue, and all storage operations
//! with edge cases, error conditions, and concurrent access patterns.

use rustypotato::{MemoryStore, StoredValue, ValueType, RustyPotatoError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;

#[cfg(test)]
mod value_type_tests {
    use super::*;

    #[test]
    fn test_value_type_string_operations() {
        let value = ValueType::String("hello world".to_string());
        
        assert!(value.is_string());
        assert!(!value.is_integer());
        assert_eq!(value.to_string(), "hello world");
        assert_eq!(value.type_name(), "string");
        assert_eq!(format!("{}", value), "hello world");
    }

    #[test]
    fn test_value_type_integer_operations() {
        let value = ValueType::Integer(42);
        
        assert!(!value.is_string());
        assert!(value.is_integer());
        assert_eq!(value.to_string(), "42");
        assert_eq!(value.type_name(), "integer");
        assert_eq!(format!("{}", value), "42");
        assert_eq!(value.to_integer().unwrap(), 42);
    }

    #[test]
    fn test_value_type_string_to_integer_conversion() {
        // Valid numeric strings
        let valid_cases = vec![
            ("0", 0),
            ("42", 42),
            ("-42", -42),
            ("9223372036854775807", i64::MAX),
            ("-9223372036854775808", i64::MIN),
        ];

        for (input, expected) in valid_cases {
            let value = ValueType::String(input.to_string());
            assert_eq!(value.to_integer().unwrap(), expected);
        }

        // Invalid numeric strings
        let invalid_cases = vec![
            "not_a_number",
            "42.5",
            "42abc",
            "",
            " 42 ",
            "42 ",
            " 42",
            "0x42",
            "42e10",
        ];

        for input in invalid_cases {
            let value = ValueType::String(input.to_string());
            assert!(value.to_integer().is_err());
        }
    }

    #[test]
    fn test_value_type_from_conversions() {
        // From &str
        let from_str: ValueType = "test".into();
        assert_eq!(from_str, ValueType::String("test".to_string()));

        // From String
        let from_string: ValueType = "test".to_string().into();
        assert_eq!(from_string, ValueType::String("test".to_string()));

        // From i64
        let from_i64: ValueType = 42i64.into();
        assert_eq!(from_i64, ValueType::Integer(42));

        // From i32
        let from_i32: ValueType = 42i32.into();
        assert_eq!(from_i32, ValueType::Integer(42));
    }

    #[test]
    fn test_value_type_edge_cases() {
        // Empty string
        let empty = ValueType::String("".to_string());
        assert_eq!(empty.to_string(), "");
        assert!(empty.to_integer().is_err());

        // Zero integer
        let zero = ValueType::Integer(0);
        assert_eq!(zero.to_string(), "0");
        assert_eq!(zero.to_integer().unwrap(), 0);

        // Negative integer
        let negative = ValueType::Integer(-1);
        assert_eq!(negative.to_string(), "-1");
        assert_eq!(negative.to_integer().unwrap(), -1);

        // Max/min integers
        let max_int = ValueType::Integer(i64::MAX);
        assert_eq!(max_int.to_integer().unwrap(), i64::MAX);

        let min_int = ValueType::Integer(i64::MIN);
        assert_eq!(min_int.to_integer().unwrap(), i64::MIN);
    }
}

#[cfg(test)]
mod stored_value_tests {
    use super::*;

    #[test]
    fn test_stored_value_creation() {
        let value = ValueType::String("test".to_string());
        let stored = StoredValue::new(value.clone());

        assert_eq!(stored.value, value);
        assert!(stored.expires_at.is_none());
        assert!(!stored.is_expired());

        // Timestamps should be recent
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

        let ttl = stored.ttl_seconds().unwrap();
        assert!(ttl >= 59 && ttl <= 60);
    }

    #[test]
    fn test_stored_value_with_ttl() {
        let value = ValueType::String("test".to_string());
        let stored = StoredValue::new_with_ttl(value.clone(), 60);

        assert_eq!(stored.value, value);
        assert!(stored.expires_at.is_some());
        assert!(!stored.is_expired());

        let ttl = stored.ttl_seconds().unwrap();
        assert!(ttl >= 59 && ttl <= 60);
    }

    #[test]
    fn test_stored_value_expiration() {
        let value = ValueType::String("test".to_string());
        let expires_at = Instant::now() - Duration::from_secs(1);
        let stored = StoredValue::new_with_expiration(value, expires_at);

        assert!(stored.is_expired());
        assert_eq!(stored.ttl_seconds().unwrap(), -2);
    }

    #[test]
    fn test_stored_value_touch() {
        let value = ValueType::String("test".to_string());
        let mut stored = StoredValue::new(value);
        let original_access_time = stored.last_accessed;

        std::thread::sleep(Duration::from_millis(2));
        stored.touch();

        assert!(stored.last_accessed > original_access_time);
    }

    #[test]
    fn test_stored_value_expiration_management() {
        let value = ValueType::String("test".to_string());
        let mut stored = StoredValue::new(value);

        // Initially no expiration
        assert!(stored.expires_at.is_none());
        assert_eq!(stored.ttl_seconds().unwrap(), -1);

        // Set expiration
        let expires_at = Instant::now() + Duration::from_secs(30);
        stored.set_expiration(expires_at);
        assert_eq!(stored.expires_at, Some(expires_at));
        assert!(!stored.is_expired());

        // Remove expiration
        stored.remove_expiration();
        assert!(stored.expires_at.is_none());
        assert_eq!(stored.ttl_seconds().unwrap(), -1);
    }
}

#[cfg(test)]
mod memory_store_tests {
    use super::*;

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
    async fn test_basic_set_get_operations() {
        let store = MemoryStore::new();

        // Set string value
        store.set("key1", "value1").await.unwrap();
        assert_eq!(store.len(), 1);
        assert!(!store.is_empty());

        let retrieved = store.get("key1").unwrap().unwrap();
        assert_eq!(retrieved.value.to_string(), "value1");

        // Set integer value
        store.set("key2", 42i64).await.unwrap();
        assert_eq!(store.len(), 2);

        let retrieved_int = store.get("key2").unwrap().unwrap();
        assert_eq!(retrieved_int.value.to_integer().unwrap(), 42);
    }

    #[tokio::test]
    async fn test_overwrite_operations() {
        let store = MemoryStore::new();

        // Set initial value
        store.set("key1", "value1").await.unwrap();
        assert_eq!(store.len(), 1);

        // Overwrite with new value
        store.set("key1", "value2").await.unwrap();
        assert_eq!(store.len(), 1);

        let retrieved = store.get("key1").unwrap().unwrap();
        assert_eq!(retrieved.value.to_string(), "value2");

        // Overwrite with different type
        store.set("key1", 42i64).await.unwrap();
        assert_eq!(store.len(), 1);

        let retrieved_int = store.get("key1").unwrap().unwrap();
        assert_eq!(retrieved_int.value.to_integer().unwrap(), 42);
    }

    #[test]
    fn test_get_nonexistent_key() {
        let store = MemoryStore::new();
        let result = store.get("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_delete_operations() {
        let store = MemoryStore::new();

        // Set a value
        store.set("key1", "value1").await.unwrap();
        assert_eq!(store.len(), 1);

        // Delete existing key
        let deleted = store.delete("key1").await.unwrap();
        assert!(deleted);
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());

        // Try to delete non-existent key
        let not_deleted = store.delete("nonexistent").await.unwrap();
        assert!(!not_deleted);
    }

    #[tokio::test]
    async fn test_exists_operations() {
        let store = MemoryStore::new();

        // Check non-existent key
        assert!(!store.exists("key1").unwrap());

        // Set a value
        store.set("key1", "value1").await.unwrap();
        assert!(store.exists("key1").unwrap());

        // Delete and check again
        store.delete("key1").await.unwrap();
        assert!(!store.exists("key1").unwrap());
    }

    #[tokio::test]
    async fn test_ttl_operations() {
        let store = MemoryStore::new();

        // Set value with TTL
        store.set_with_ttl("key1", "value1", 60).await.unwrap();
        let ttl = store.ttl("key1").unwrap();
        assert!(ttl >= 59 && ttl <= 60);

        // Set expiration on existing key
        store.set("key2", "value2").await.unwrap();
        let expired = store.expire("key2", 30).await.unwrap();
        assert!(expired);

        let ttl2 = store.ttl("key2").unwrap();
        assert!(ttl2 >= 29 && ttl2 <= 30);

        // Try to expire non-existent key
        let not_expired = store.expire("nonexistent", 30).await.unwrap();
        assert!(!not_expired);

        // Check TTL for non-existent key
        assert_eq!(store.ttl("nonexistent").unwrap(), -2);

        // Check TTL for key without expiration
        store.set("key3", "value3").await.unwrap();
        assert_eq!(store.ttl("key3").unwrap(), -1);
    }

    #[tokio::test]
    async fn test_persist_operations() {
        let store = MemoryStore::new();

        // Set value with TTL
        store.set_with_ttl("key1", "value1", 60).await.unwrap();
        assert!(store.ttl("key1").unwrap() > 0);

        // Remove expiration
        let persisted = store.persist("key1").unwrap();
        assert!(persisted);
        assert_eq!(store.ttl("key1").unwrap(), -1);

        // Try to persist key without expiration
        store.set("key2", "value2").await.unwrap();
        let not_persisted = store.persist("key2").unwrap();
        assert!(!not_persisted);

        // Try to persist non-existent key
        let not_persisted = store.persist("nonexistent").unwrap();
        assert!(!not_persisted);
    }

    #[tokio::test]
    async fn test_atomic_increment_operations() {
        let store = MemoryStore::new();

        // INCR on non-existent key
        let result = store.incr("counter").await.unwrap();
        assert_eq!(result, 1);

        // INCR on existing integer
        let result = store.incr("counter").await.unwrap();
        assert_eq!(result, 2);

        // INCR by specific amount
        let result = store.incr_by("counter", 5).await.unwrap();
        assert_eq!(result, 7);

        // DECR operation
        let result = store.decr("counter").await.unwrap();
        assert_eq!(result, 6);

        // DECR on non-existent key
        let result = store.decr("new_counter").await.unwrap();
        assert_eq!(result, -1);
    }

    #[tokio::test]
    async fn test_atomic_operations_error_cases() {
        let store = MemoryStore::new();

        // Set non-numeric value
        store.set("string_key", "not_a_number").await.unwrap();

        // Try to increment non-numeric value
        let result = store.incr("string_key").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RustyPotatoError::NotAnInteger { .. }));

        // Test overflow protection
        store.set("max_int", i64::MAX).await.unwrap();
        let result = store.incr("max_int").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_expiration_handling() {
        let store = MemoryStore::new();

        // Set value with very short TTL
        let expires_at = Instant::now() + Duration::from_millis(10);
        store.set_with_expiration("key1", "value1", expires_at).await.unwrap();

        // Wait for expiration
        sleep(Duration::from_millis(20)).await;

        // Key should be automatically removed when accessed
        let result = store.get("key1").unwrap();
        assert!(result.is_none());

        // Exists should return false
        assert!(!store.exists("key1").unwrap());

        // Delete should return false
        let deleted = store.delete("key1").await.unwrap();
        assert!(!deleted);

        // TTL should return -2
        assert_eq!(store.ttl("key1").unwrap(), -2);
    }

    #[test]
    fn test_cleanup_expired_keys() {
        let store = MemoryStore::new();

        // Add some keys with past expiration times
        let past_time = Instant::now() - Duration::from_secs(1);
        store.expiration_index.insert("expired1".to_string(), past_time);
        store.expiration_index.insert("expired2".to_string(), past_time);
        
        // Add a key with future expiration
        let future_time = Instant::now() + Duration::from_secs(60);
        store.expiration_index.insert("valid".to_string(), future_time);

        // Add corresponding data entries
        store.data.insert("expired1".to_string(), StoredValue::new("value1".into()));
        store.data.insert("expired2".to_string(), StoredValue::new("value2".into()));
        store.data.insert("valid".to_string(), StoredValue::new("value3".into()));

        // Run cleanup
        let removed_count = store.cleanup_expired();
        assert_eq!(removed_count, 2);

        // Verify expired keys are removed
        assert!(!store.data.contains_key("expired1"));
        assert!(!store.data.contains_key("expired2"));
        assert!(store.data.contains_key("valid"));
    }

    #[test]
    fn test_memory_stats() {
        let store = MemoryStore::new();

        let stats = store.memory_stats();
        assert_eq!(stats.key_count, 0);
        assert_eq!(stats.expiration_count, 0);
        assert_eq!(stats.estimated_memory_bytes, 0);
    }

    #[test]
    fn test_keys_operation() {
        let store = MemoryStore::new();

        // Initially empty
        let keys = store.keys();
        assert!(keys.is_empty());

        // Add some keys (using internal methods for testing)
        store.data.insert("key1".to_string(), StoredValue::new("value1".into()));
        store.data.insert("key2".to_string(), StoredValue::new("value2".into()));
        store.data.insert("key3".to_string(), StoredValue::new("value3".into()));

        let keys = store.keys();
        assert_eq!(keys.len(), 3);
        assert!(keys.contains(&"key1".to_string()));
        assert!(keys.contains(&"key2".to_string()));
        assert!(keys.contains(&"key3".to_string()));
    }

    #[test]
    fn test_clear_operation() {
        let store = MemoryStore::new();

        // Add some data
        store.data.insert("key1".to_string(), StoredValue::new("value1".into()));
        store.data.insert("key2".to_string(), StoredValue::new("value2".into()));
        store.expiration_index.insert("key1".to_string(), Instant::now());

        assert_eq!(store.len(), 2);
        assert_eq!(store.expiration_index.len(), 1);

        // Clear all data
        store.clear();

        assert_eq!(store.len(), 0);
        assert_eq!(store.expiration_index.len(), 0);
        assert!(store.is_empty());
    }

    #[tokio::test]
    async fn test_concurrent_access_patterns() {
        let store = Arc::new(MemoryStore::new());
        let mut handles = Vec::new();

        // Spawn multiple tasks performing different operations
        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            let handle = tokio::spawn(async move {
                let key = format!("key_{}", i);
                let value = format!("value_{}", i);

                // Set operation
                store_clone.set(&key, &value).await.unwrap();

                // Get operation
                let retrieved = store_clone.get(&key).unwrap().unwrap();
                assert_eq!(retrieved.value.to_string(), value);

                // Exists operation
                assert!(store_clone.exists(&key).unwrap());

                // Increment operation on a shared counter
                store_clone.incr("shared_counter").await.unwrap();

                i
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify final state
        assert_eq!(store.len(), 11); // 10 individual keys + 1 shared counter
        let counter_value = store.get("shared_counter").unwrap().unwrap();
        assert_eq!(counter_value.value.to_integer().unwrap(), 10);
    }

    #[tokio::test]
    async fn test_mixed_operations_stress() {
        let store = Arc::new(MemoryStore::new());
        let mut handles = Vec::new();

        // Spawn tasks with mixed operations
        for i in 0..20 {
            let store_clone = Arc::clone(&store);
            let handle = tokio::spawn(async move {
                match i % 4 {
                    0 => {
                        // SET operations
                        let key = format!("set_key_{}", i);
                        store_clone.set(key, format!("value_{}", i)).await.unwrap();
                    }
                    1 => {
                        // GET operations (may not find anything initially)
                        let key = format!("set_key_{}", i - 1);
                        let _ = store_clone.get(&key);
                    }
                    2 => {
                        // INCR operations
                        let key = format!("counter_{}", i % 5);
                        store_clone.incr(&key).await.unwrap();
                    }
                    3 => {
                        // TTL operations
                        let key = format!("ttl_key_{}", i);
                        store_clone.set_with_ttl(key, "temp_value", 60).await.unwrap();
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

        // Verify store is in a consistent state
        assert!(store.len() > 0);
        let stats = store.memory_stats();
        assert!(stats.key_count > 0);
    }
}

#[cfg(test)]
mod edge_case_tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_string_keys_and_values() {
        let store = MemoryStore::new();

        // Empty key (should work)
        store.set("", "value").await.unwrap();
        let result = store.get("").unwrap().unwrap();
        assert_eq!(result.value.to_string(), "value");

        // Empty value (should work)
        store.set("key", "").await.unwrap();
        let result = store.get("key").unwrap().unwrap();
        assert_eq!(result.value.to_string(), "");
    }

    #[tokio::test]
    async fn test_unicode_keys_and_values() {
        let store = MemoryStore::new();

        // Unicode key and value
        let unicode_key = "🔑_key_🗝️";
        let unicode_value = "🎯_value_💎";

        store.set(unicode_key, unicode_value).await.unwrap();
        let result = store.get(unicode_key).unwrap().unwrap();
        assert_eq!(result.value.to_string(), unicode_value);
    }

    #[tokio::test]
    async fn test_very_long_keys_and_values() {
        let store = MemoryStore::new();

        // Very long key and value
        let long_key = "k".repeat(1000);
        let long_value = "v".repeat(10000);

        store.set(&long_key, &long_value).await.unwrap();
        let result = store.get(&long_key).unwrap().unwrap();
        assert_eq!(result.value.to_string(), long_value);
    }

    #[tokio::test]
    async fn test_special_character_keys() {
        let store = MemoryStore::new();

        let special_keys = vec![
            " ",
            "\n",
            "\t",
            "\r\n",
            "key with spaces",
            "key:with:colons",
            "key/with/slashes",
            "key\\with\\backslashes",
            "key\"with\"quotes",
            "key'with'apostrophes",
        ];

        for key in special_keys {
            store.set(key, format!("value_for_{}", key)).await.unwrap();
            let result = store.get(key).unwrap().unwrap();
            assert_eq!(result.value.to_string(), format!("value_for_{}", key));
        }
    }

    #[tokio::test]
    async fn test_integer_boundary_values() {
        let store = MemoryStore::new();

        let boundary_values = vec![
            i64::MIN,
            i64::MIN + 1,
            -1,
            0,
            1,
            i64::MAX - 1,
            i64::MAX,
        ];

        for value in boundary_values {
            let key = format!("int_{}", value);
            store.set(&key, value).await.unwrap();
            let result = store.get(&key).unwrap().unwrap();
            assert_eq!(result.value.to_integer().unwrap(), value);
        }
    }

    #[tokio::test]
    async fn test_rapid_expiration_cycles() {
        let store = MemoryStore::new();

        // Set key with short TTL, let it expire, then set again
        for i in 0..5 {
            let key = format!("cycle_key_{}", i);
            
            // Set with very short TTL
            store.set_with_ttl(&key, format!("value_{}", i), 1).await.unwrap();
            assert!(store.exists(&key).unwrap());
            
            // Wait for expiration
            sleep(Duration::from_millis(1100)).await;
            
            // Should be expired
            assert!(!store.exists(&key).unwrap());
            
            // Set again
            store.set(&key, format!("new_value_{}", i)).await.unwrap();
            assert!(store.exists(&key).unwrap());
        }
    }
}