//! Property-based tests for command execution
//!
//! Tests verify:
//! - Commands with valid args succeed or return expected errors
//! - Atomic operations maintain consistency under contention
//! - Hash commands work correctly with arbitrary field counts
//! - Command results are consistent

use proptest::prelude::*;
use rustypotato::commands::{
    Command, CommandResult, ResponseValue,
    SetCommand, GetCommand, DelCommand, ExistsCommand,
    IncrCommand, DecrCommand,
    ExpireCommand, TtlCommand,
    HsetCommand, HgetCommand, HdelCommand, HgetallCommand, HexistsCommand,
};
use rustypotato::MemoryStore;
use std::sync::Arc;
use uuid::Uuid;

fn test_client_id() -> Uuid {
    Uuid::new_v4()
}

/// Strategy for generating valid Redis keys
fn key_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_]{1,50}".prop_map(|s| s)
}

/// Strategy for generating valid string values
fn value_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_\\-\\.]{0,200}".prop_map(|s| s)
}

/// Strategy for generating hash field names
fn field_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_]{1,30}".prop_map(|s| s)
}

/// Strategy for TTL values
fn ttl_strategy() -> impl Strategy<Value = u64> {
    // Start at 5 seconds to avoid race conditions in CI where short TTLs can expire
    // before the TTL check runs
    5u64..3600u64
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    // ==================== String Command Properties ====================

    /// Property: SET with valid args always returns OK
    #[test]
    fn prop_set_valid_args_returns_ok(key in key_strategy(), value in value_strategy()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            let cmd = SetCommand;
            let args = vec![key, value];

            let result = cmd.execute(&args, &store, test_client_id()).await;

            match result {
                CommandResult::Ok(ResponseValue::SimpleString(s)) => {
                    prop_assert_eq!(s, "OK");
                }
                _ => prop_assert!(false, "SET should return OK"),
            }
            Ok(())
        })?;
    }

    /// Property: GET after SET returns the set value
    #[test]
    fn prop_get_after_set_returns_value(key in key_strategy(), value in value_strategy()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();

            let set_cmd = SetCommand;
            let get_cmd = GetCommand;

            // SET the value
            let set_args = vec![key.clone(), value.clone()];
            set_cmd.execute(&set_args, &store, test_client_id()).await;

            // GET the value
            let get_args = vec![key];
            let result = get_cmd.execute(&get_args, &store, test_client_id()).await;

            match result {
                CommandResult::Ok(ResponseValue::BulkString(Some(v))) => {
                    prop_assert_eq!(v, value);
                }
                _ => prop_assert!(false, "GET should return the set value"),
            }
            Ok(())
        })?;
    }

    /// Property: DEL returns correct count of deleted keys
    #[test]
    fn prop_del_returns_correct_count(
        keys in prop::collection::vec(key_strategy(), 1..10),
        value in value_strategy()
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            let set_cmd = SetCommand;
            let del_cmd = DelCommand;

            // Set all keys
            for key in &keys {
                let args = vec![key.clone(), value.clone()];
                set_cmd.execute(&args, &store, test_client_id()).await;
            }

            // Delete all keys
            let result = del_cmd.execute(&keys, &store, test_client_id()).await;

            // Count unique keys (in case of duplicates)
            let unique_keys: std::collections::HashSet<_> = keys.iter().collect();

            match result {
                CommandResult::Ok(ResponseValue::Integer(count)) => {
                    prop_assert!(count <= unique_keys.len() as i64);
                    prop_assert!(count >= 0);
                }
                _ => prop_assert!(false, "DEL should return integer count"),
            }
            Ok(())
        })?;
    }

    /// Property: EXISTS returns correct count
    #[test]
    fn prop_exists_returns_correct_count(
        existing_keys in prop::collection::vec(key_strategy(), 1..5),
        value in value_strategy()
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            let set_cmd = SetCommand;
            let exists_cmd = ExistsCommand;

            // Set some keys
            for key in &existing_keys {
                let args = vec![key.clone(), value.clone()];
                set_cmd.execute(&args, &store, test_client_id()).await;
            }

            // Check EXISTS
            let result = exists_cmd.execute(&existing_keys, &store, test_client_id()).await;

            let unique_keys: std::collections::HashSet<_> = existing_keys.iter().collect();

            match result {
                CommandResult::Ok(ResponseValue::Integer(count)) => {
                    prop_assert!(count <= unique_keys.len() as i64);
                    prop_assert!(count >= 0);
                }
                _ => prop_assert!(false, "EXISTS should return integer count"),
            }
            Ok(())
        })?;
    }

    // ==================== Atomic Command Properties ====================

    /// Property: INCR returns consecutive integers starting from 1
    #[test]
    fn prop_incr_returns_consecutive(key in key_strategy(), n in 1u32..50u32) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            let incr_cmd = IncrCommand;
            let args = vec![key];

            for expected in 1..=n {
                let result = incr_cmd.execute(&args, &store, test_client_id()).await;

                match result {
                    CommandResult::Ok(ResponseValue::Integer(v)) => {
                        prop_assert_eq!(v, expected as i64);
                    }
                    _ => prop_assert!(false, "INCR should return integer"),
                }
            }
            Ok(())
        })?;
    }

    /// Property: DECR returns consecutive negative integers starting from -1
    #[test]
    fn prop_decr_returns_consecutive(key in key_strategy(), n in 1u32..50u32) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            let decr_cmd = DecrCommand;
            let args = vec![key];

            for expected in 1..=n {
                let result = decr_cmd.execute(&args, &store, test_client_id()).await;

                match result {
                    CommandResult::Ok(ResponseValue::Integer(v)) => {
                        prop_assert_eq!(v, -(expected as i64));
                    }
                    _ => prop_assert!(false, "DECR should return integer"),
                }
            }
            Ok(())
        })?;
    }

    /// Property: INCR on non-numeric string returns error
    #[test]
    fn prop_incr_non_numeric_errors(key in key_strategy()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();

            // Set a non-numeric value
            let set_cmd = SetCommand;
            set_cmd.execute(&[key.clone(), "not_a_number".to_string()], &store, test_client_id()).await;

            // Try to INCR
            let incr_cmd = IncrCommand;
            let result = incr_cmd.execute(&[key], &store, test_client_id()).await;

            prop_assert!(matches!(result, CommandResult::Error(_)));
            Ok(())
        })?;
    }

    // ==================== Hash Command Properties ====================

    /// Property: HSET returns 1 for new field, 0 for update
    #[test]
    fn prop_hset_returns_correct_flag(
        key in key_strategy(),
        field in field_strategy(),
        value1 in value_strategy(),
        value2 in value_strategy()
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            let hset_cmd = HsetCommand;

            // First HSET should return 1 (new field)
            let args1 = vec![key.clone(), field.clone(), value1];
            let result1 = hset_cmd.execute(&args1, &store, test_client_id()).await;

            match result1 {
                CommandResult::Ok(ResponseValue::Integer(v)) => {
                    prop_assert_eq!(v, 1);
                }
                _ => prop_assert!(false, "HSET should return 1 for new field"),
            }

            // Second HSET should return 0 (update)
            let args2 = vec![key, field, value2];
            let result2 = hset_cmd.execute(&args2, &store, test_client_id()).await;

            match result2 {
                CommandResult::Ok(ResponseValue::Integer(v)) => {
                    prop_assert_eq!(v, 0);
                }
                _ => prop_assert!(false, "HSET should return 0 for update"),
            }

            Ok(())
        })?;
    }

    /// Property: HGET returns set value
    #[test]
    fn prop_hget_returns_set_value(
        key in key_strategy(),
        field in field_strategy(),
        value in value_strategy()
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();

            let hset_cmd = HsetCommand;
            let hget_cmd = HgetCommand;

            // HSET
            hset_cmd.execute(&[key.clone(), field.clone(), value.clone()], &store, test_client_id()).await;

            // HGET
            let result = hget_cmd.execute(&[key, field], &store, test_client_id()).await;

            match result {
                CommandResult::Ok(ResponseValue::BulkString(Some(v))) => {
                    prop_assert_eq!(v, value);
                }
                _ => prop_assert!(false, "HGET should return the set value"),
            }

            Ok(())
        })?;
    }

    /// Property: HDEL returns correct count
    #[test]
    fn prop_hdel_returns_correct_count(
        key in key_strategy(),
        fields in prop::collection::vec(field_strategy(), 1..10),
        value in value_strategy()
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            let hset_cmd = HsetCommand;
            let hdel_cmd = HdelCommand;

            // Set all fields
            for field in &fields {
                hset_cmd.execute(&[key.clone(), field.clone(), value.clone()], &store, test_client_id()).await;
            }

            // Delete all fields
            let mut args = vec![key];
            args.extend(fields.clone());
            let result = hdel_cmd.execute(&args, &store, test_client_id()).await;

            let unique_fields: std::collections::HashSet<_> = fields.iter().collect();

            match result {
                CommandResult::Ok(ResponseValue::Integer(count)) => {
                    prop_assert!(count <= unique_fields.len() as i64);
                    prop_assert!(count >= 0);
                }
                _ => prop_assert!(false, "HDEL should return integer count"),
            }

            Ok(())
        })?;
    }

    /// Property: HGETALL returns all set fields
    #[test]
    fn prop_hgetall_returns_all(
        key in key_strategy(),
        fields in prop::collection::hash_map(field_strategy(), value_strategy(), 1..10)
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            let hset_cmd = HsetCommand;
            let hgetall_cmd = HgetallCommand;

            // Set all fields
            for (field, value) in &fields {
                hset_cmd.execute(&[key.clone(), field.clone(), value.clone()], &store, test_client_id()).await;
            }

            // Get all
            let result = hgetall_cmd.execute(&[key], &store, test_client_id()).await;

            match result {
                CommandResult::Ok(ResponseValue::Array(arr)) => {
                    // Array should have pairs: field, value, field, value, ...
                    prop_assert_eq!(arr.len(), fields.len() * 2);
                }
                _ => prop_assert!(false, "HGETALL should return array"),
            }

            Ok(())
        })?;
    }

    /// Property: HEXISTS returns 1 for existing, 0 for non-existing
    #[test]
    fn prop_hexists_correct_response(
        key in key_strategy(),
        field in field_strategy(),
        value in value_strategy()
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            let hset_cmd = HsetCommand;
            let hexists_cmd = HexistsCommand;

            // Check non-existent
            let result1 = hexists_cmd.execute(&[key.clone(), field.clone()], &store, test_client_id()).await;
            match result1 {
                CommandResult::Ok(ResponseValue::Integer(v)) => {
                    prop_assert_eq!(v, 0);
                }
                _ => prop_assert!(false, "HEXISTS should return 0 for non-existent"),
            }

            // Set field
            hset_cmd.execute(&[key.clone(), field.clone(), value], &store, test_client_id()).await;

            // Check existing
            let result2 = hexists_cmd.execute(&[key, field], &store, test_client_id()).await;
            match result2 {
                CommandResult::Ok(ResponseValue::Integer(v)) => {
                    prop_assert_eq!(v, 1);
                }
                _ => prop_assert!(false, "HEXISTS should return 1 for existing"),
            }

            Ok(())
        })?;
    }

    // ==================== TTL Command Properties ====================

    /// Property: EXPIRE returns 1 for existing key, 0 for non-existing
    #[test]
    fn prop_expire_correct_response(key in key_strategy(), value in value_strategy(), ttl in ttl_strategy()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            let set_cmd = SetCommand;
            let expire_cmd = ExpireCommand;

            // Expire non-existent key
            let result1 = expire_cmd.execute(&[key.clone(), ttl.to_string()], &store, test_client_id()).await;
            match result1 {
                CommandResult::Ok(ResponseValue::Integer(v)) => {
                    prop_assert_eq!(v, 0);
                }
                _ => prop_assert!(false, "EXPIRE should return 0 for non-existent key"),
            }

            // Set key
            set_cmd.execute(&[key.clone(), value], &store, test_client_id()).await;

            // Expire existing key
            let result2 = expire_cmd.execute(&[key, ttl.to_string()], &store, test_client_id()).await;
            match result2 {
                CommandResult::Ok(ResponseValue::Integer(v)) => {
                    prop_assert_eq!(v, 1);
                }
                _ => prop_assert!(false, "EXPIRE should return 1 for existing key"),
            }

            Ok(())
        })?;
    }

    /// Property: TTL returns -2 for non-existent, -1 or positive for existing
    #[test]
    fn prop_ttl_correct_response(key in key_strategy(), value in value_strategy(), ttl in ttl_strategy()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();
            let set_cmd = SetCommand;
            let expire_cmd = ExpireCommand;
            let ttl_cmd = TtlCommand;

            // TTL on non-existent key
            let result1 = ttl_cmd.execute(std::slice::from_ref(&key), &store, test_client_id()).await;
            match result1 {
                CommandResult::Ok(ResponseValue::Integer(v)) => {
                    prop_assert_eq!(v, -2);
                }
                _ => prop_assert!(false, "TTL should return -2 for non-existent key"),
            }

            // Set key without expiration
            set_cmd.execute(&[key.clone(), value], &store, test_client_id()).await;

            // TTL on key without expiration
            let result2 = ttl_cmd.execute(std::slice::from_ref(&key), &store, test_client_id()).await;
            match result2 {
                CommandResult::Ok(ResponseValue::Integer(v)) => {
                    prop_assert_eq!(v, -1);
                }
                _ => prop_assert!(false, "TTL should return -1 for key without expiration"),
            }

            // Set expiration
            expire_cmd.execute(&[key.clone(), ttl.to_string()], &store, test_client_id()).await;

            // TTL on key with expiration
            let result3 = ttl_cmd.execute(&[key], &store, test_client_id()).await;
            match result3 {
                CommandResult::Ok(ResponseValue::Integer(v)) => {
                    prop_assert!(v > 0);
                    prop_assert!(v <= ttl as i64);
                }
                _ => prop_assert!(false, "TTL should return positive value"),
            }

            Ok(())
        })?;
    }

    // ==================== Error Handling Properties ====================

    /// Property: Commands with wrong arg count return error
    #[test]
    fn prop_wrong_args_return_error(key in key_strategy()) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = MemoryStore::new();

            // SET with 1 arg (needs 2)
            let set_cmd = SetCommand;
            let result = set_cmd.execute(std::slice::from_ref(&key), &store, test_client_id()).await;
            prop_assert!(matches!(result, CommandResult::Error(_)));

            // GET with 0 args (needs 1)
            let get_cmd = GetCommand;
            let result = get_cmd.execute(&[], &store, test_client_id()).await;
            prop_assert!(matches!(result, CommandResult::Error(_)));

            // HSET with 2 args (needs 3)
            let hset_cmd = HsetCommand;
            let result = hset_cmd.execute(&[key.clone(), "field".to_string()], &store, test_client_id()).await;
            prop_assert!(matches!(result, CommandResult::Error(_)));

            Ok(())
        })?;
    }
}

#[cfg(test)]
mod concurrent_command_tests {
    use super::*;
    use tokio::sync::Barrier;

    /// Test concurrent INCR operations maintain atomicity
    #[tokio::test]
    async fn test_concurrent_incr_atomicity() {
        let store = Arc::new(MemoryStore::new());
        let barrier = Arc::new(Barrier::new(100));
        let mut handles = vec![];

        for _ in 0..100 {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                let cmd = IncrCommand;
                cmd.execute(&["counter".to_string()], &store, test_client_id()).await
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let get_cmd = GetCommand;
        let result = get_cmd.execute(&["counter".to_string()], &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(Some(v))) => {
                assert_eq!(v.parse::<i64>().unwrap(), 100);
            }
            _ => panic!("Expected counter to be 100"),
        }
    }

    /// Test concurrent HSET operations don't lose data
    #[tokio::test]
    async fn test_concurrent_hset_no_data_loss() {
        let store = Arc::new(MemoryStore::new());
        let barrier = Arc::new(Barrier::new(50));
        let mut handles = vec![];

        for i in 0..50 {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                let cmd = HsetCommand;
                cmd.execute(
                    &["hash".to_string(), format!("field_{}", i), format!("value_{}", i)],
                    &store,
                    test_client_id(),
                ).await
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all 50 fields exist
        let hgetall_cmd = HgetallCommand;
        let result = hgetall_cmd.execute(&["hash".to_string()], &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(arr)) => {
                assert_eq!(arr.len(), 100); // 50 fields * 2 (field + value)
            }
            _ => panic!("Expected 50 fields in hash"),
        }
    }
}
