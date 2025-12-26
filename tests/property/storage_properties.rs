//! Property-based tests for the storage layer
//!
//! Tests verify invariants like:
//! - SET/GET roundtrip returns same value
//! - INCR/DECR operations are consistent
//! - Hash operations maintain consistency
//! - TTL operations behave correctly

use proptest::prelude::*;
use rustypotato::{MemoryStore, ValueType};
use std::collections::HashMap;

/// Strategy for generating valid Redis keys (alphanumeric, reasonable length)
fn key_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_]{1,100}".prop_map(|s| s)
}

/// Strategy for generating valid string values
fn value_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_\\-\\.\\s]{0,1000}".prop_map(|s| s)
}

/// Strategy for generating hash field names
fn field_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_]{1,50}".prop_map(|s| s)
}

/// Strategy for generating TTL values in seconds (minimum 10s to avoid timing issues)
fn ttl_strategy() -> impl Strategy<Value = u64> {
    10u64..3600u64
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    // ==================== Basic SET/GET Properties ====================

    /// Property: SET then GET returns the same value (roundtrip)
    #[test]
    fn prop_set_get_roundtrip(key in key_strategy(), value in value_strategy()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            store.set(key.clone(), value.clone()).await.unwrap();

            let retrieved = store.get(&key).unwrap();
            prop_assert!(retrieved.is_some());
            prop_assert_eq!(retrieved.unwrap().value.to_string(), value);
            Ok(())
        })?;
    }

    /// Property: GET on non-existent key returns None
    #[test]
    fn prop_get_nonexistent_returns_none(key in key_strategy()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            let result = store.get(&key).unwrap();
            prop_assert!(result.is_none());
            Ok(())
        })?;
    }

    /// Property: SET overwrites previous value
    #[test]
    fn prop_set_overwrites(
        key in key_strategy(),
        value1 in value_strategy(),
        value2 in value_strategy()
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            store.set(key.clone(), value1).await.unwrap();
            store.set(key.clone(), value2.clone()).await.unwrap();

            let retrieved = store.get(&key).unwrap();
            prop_assert!(retrieved.is_some());
            prop_assert_eq!(retrieved.unwrap().value.to_string(), value2);
            Ok(())
        })?;
    }

    /// Property: DELETE removes key, subsequent GET returns None
    #[test]
    fn prop_delete_removes_key(key in key_strategy(), value in value_strategy()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            store.set(key.clone(), value).await.unwrap();

            let deleted = store.delete(&key).await.unwrap();
            prop_assert!(deleted);

            let retrieved = store.get(&key).unwrap();
            prop_assert!(retrieved.is_none());
            Ok(())
        })?;
    }

    /// Property: EXISTS returns true for existing keys, false for non-existing
    #[test]
    fn prop_exists_consistency(key in key_strategy(), value in value_strategy()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();

            // Before setting
            prop_assert!(!store.exists(&key).unwrap());

            // After setting
            store.set(key.clone(), value).await.unwrap();
            prop_assert!(store.exists(&key).unwrap());

            // After deleting
            store.delete(&key).await.unwrap();
            prop_assert!(!store.exists(&key).unwrap());

            Ok(())
        })?;
    }

    // ==================== Atomic Operations Properties ====================

    /// Property: INCR n times equals n (starting from 0)
    #[test]
    fn prop_incr_n_times_equals_n(key in key_strategy(), n in 1u32..100u32) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();

            for _ in 0..n {
                store.incr(&key).await.unwrap();
            }

            let value = store.get(&key).unwrap().unwrap();
            prop_assert_eq!(value.value.to_integer().unwrap(), n as i64);
            Ok(())
        })?;
    }

    /// Property: INCR n times then DECR n times equals 0
    #[test]
    fn prop_incr_decr_identity(key in key_strategy(), n in 1u32..50u32) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();

            for _ in 0..n {
                store.incr(&key).await.unwrap();
            }
            for _ in 0..n {
                store.decr(&key).await.unwrap();
            }

            let value = store.get(&key).unwrap().unwrap();
            prop_assert_eq!(value.value.to_integer().unwrap(), 0);
            Ok(())
        })?;
    }

    /// Property: INCR and DECR are inverses
    #[test]
    fn prop_incr_decr_inverse(key in key_strategy(), initial in -1000i64..1000i64) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            store.set(key.clone(), initial.to_string()).await.unwrap();

            store.incr(&key).await.unwrap();
            store.decr(&key).await.unwrap();

            let value = store.get(&key).unwrap().unwrap();
            prop_assert_eq!(value.value.to_integer().unwrap(), initial);
            Ok(())
        })?;
    }

    // ==================== Hash Operations Properties ====================

    /// Property: HSET then HGET returns same value (roundtrip)
    #[test]
    fn prop_hset_hget_roundtrip(
        key in key_strategy(),
        field in field_strategy(),
        value in value_strategy()
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            store.hset(&key, &field, &value).await.unwrap();

            let retrieved = store.hget(&key, &field).await.unwrap();
            prop_assert!(retrieved.is_some());
            prop_assert_eq!(retrieved.unwrap(), value);
            Ok(())
        })?;
    }

    /// Property: HGETALL returns all set fields
    #[test]
    fn prop_hgetall_returns_all_fields(
        key in key_strategy(),
        fields in prop::collection::hash_map(field_strategy(), value_strategy(), 1..20)
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();

            // Set all fields
            for (field, value) in &fields {
                store.hset(&key, field, value).await.unwrap();
            }

            // Get all and verify
            let retrieved = store.hgetall(&key).await.unwrap();
            let retrieved_map: HashMap<_, _> = retrieved.into_iter().collect();

            prop_assert_eq!(retrieved_map.len(), fields.len());
            for (field, value) in &fields {
                prop_assert_eq!(retrieved_map.get(field), Some(value));
            }
            Ok(())
        })?;
    }

    /// Property: HEXISTS returns true for existing fields, false for non-existing
    #[test]
    fn prop_hexists_consistency(
        key in key_strategy(),
        field in field_strategy(),
        value in value_strategy()
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();

            // Before setting
            prop_assert!(!store.hexists(&key, &field).await.unwrap());

            // After setting
            store.hset(&key, &field, &value).await.unwrap();
            prop_assert!(store.hexists(&key, &field).await.unwrap());

            // After deleting
            store.hdel(&key, &field).await.unwrap();
            prop_assert!(!store.hexists(&key, &field).await.unwrap());

            Ok(())
        })?;
    }

    /// Property: HDEL removes only specified fields
    #[test]
    fn prop_hdel_removes_only_specified(
        key in key_strategy(),
        field1 in field_strategy(),
        field2 in field_strategy(),
        value in value_strategy()
    ) {
        prop_assume!(field1 != field2);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();

            store.hset(&key, &field1, &value).await.unwrap();
            store.hset(&key, &field2, &value).await.unwrap();

            // Delete only field1
            store.hdel(&key, &field1).await.unwrap();

            // field1 should be gone, field2 should remain
            prop_assert!(!store.hexists(&key, &field1).await.unwrap());
            prop_assert!(store.hexists(&key, &field2).await.unwrap());

            Ok(())
        })?;
    }

    // ==================== TTL Properties ====================

    /// Property: Key with TTL > 0 exists immediately after setting
    #[test]
    fn prop_key_with_ttl_exists_initially(
        key in key_strategy(),
        value in value_strategy(),
        ttl in ttl_strategy()
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            store.set(key.clone(), value).await.unwrap();
            store.expire(&key, ttl).await.unwrap();

            // Key should exist immediately
            prop_assert!(store.exists(&key).unwrap());

            // TTL should be close to what we set (allowing some tolerance)
            let remaining = store.ttl(&key).unwrap();
            prop_assert!(remaining <= ttl as i64);
            prop_assert!(remaining > 0);

            Ok(())
        })?;
    }

    /// Property: PERSIST removes TTL, key becomes permanent
    #[test]
    fn prop_persist_removes_ttl(
        key in key_strategy(),
        value in value_strategy(),
        ttl in ttl_strategy()
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            store.set(key.clone(), value).await.unwrap();
            store.expire(&key, ttl).await.unwrap();

            // TTL should be set (positive value)
            let ttl_before = store.ttl(&key).unwrap();
            prop_assert!(ttl_before > 0);

            // Persist
            store.persist(&key).unwrap();

            // TTL should be removed (returns -1 for permanent keys)
            let ttl_after = store.ttl(&key).unwrap();
            prop_assert_eq!(ttl_after, -1);

            // Key should still exist
            prop_assert!(store.exists(&key).unwrap());

            Ok(())
        })?;
    }

    // ==================== ValueType Properties ====================

    /// Property: String ValueType roundtrips correctly
    #[test]
    fn prop_value_type_string_roundtrip(s in value_strategy()) {
        let value = ValueType::String(s.clone());
        prop_assert_eq!(value.to_string(), s);
        prop_assert!(value.is_string());
        prop_assert!(!value.is_integer());
    }

    /// Property: Integer ValueType roundtrips correctly
    #[test]
    fn prop_value_type_integer_roundtrip(i in any::<i64>()) {
        let value = ValueType::Integer(i);
        prop_assert_eq!(value.to_integer().unwrap(), i);
        prop_assert!(value.is_integer());
        prop_assert!(!value.is_string());
    }

    /// Property: Numeric string converts to integer correctly
    #[test]
    fn prop_numeric_string_converts(i in -1000000i64..1000000i64) {
        let value = ValueType::String(i.to_string());
        prop_assert_eq!(value.to_integer().unwrap(), i);
    }

    // ==================== Multi-key Properties ====================

    /// Property: Operations on different keys don't interfere
    #[test]
    fn prop_key_isolation(
        key1 in key_strategy(),
        key2 in key_strategy(),
        value1 in value_strategy(),
        value2 in value_strategy()
    ) {
        prop_assume!(key1 != key2);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();

            store.set(key1.clone(), value1).await.unwrap();
            store.set(key2.clone(), value2.clone()).await.unwrap();

            // Deleting key1 shouldn't affect key2
            store.delete(&key1).await.unwrap();

            prop_assert!(store.get(&key1).unwrap().is_none());
            prop_assert!(store.get(&key2).unwrap().is_some());
            prop_assert_eq!(store.get(&key2).unwrap().unwrap().value.to_string(), value2);

            Ok(())
        })?;
    }
}

#[cfg(test)]
mod concurrent_properties {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Barrier;

    /// Property: Concurrent INCR operations are atomic
    #[tokio::test]
    async fn prop_concurrent_incr_atomicity() {
        let store = Arc::new(MemoryStore::new());
        let barrier = Arc::new(Barrier::new(100));
        let mut handles = vec![];

        for _ in 0..100 {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                store.incr("counter").await.unwrap();
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let value = store.get("counter").unwrap().unwrap();
        assert_eq!(value.value.to_integer().unwrap(), 100);
    }

    /// Property: Concurrent SET operations don't lose data
    #[tokio::test]
    async fn prop_concurrent_set_no_data_loss() {
        let store = Arc::new(MemoryStore::new());
        let barrier = Arc::new(Barrier::new(50));
        let mut handles = vec![];

        for i in 0..50 {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            let key = format!("key_{}", i);
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                store.set(key, format!("value_{}", i)).await.unwrap();
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // All 50 keys should exist
        for i in 0..50 {
            let key = format!("key_{}", i);
            assert!(store.exists(&key).unwrap(), "Key {} should exist", key);
        }
    }
}
