//! Concurrency tests using multiple tokio tasks
//!
//! These tests verify that RustyPotato handles concurrent operations correctly,
//! including race conditions, data consistency, and thread safety.

use rustypotato::{MemoryStore, ValueType, CommandRegistry, SetCommand, GetCommand, IncrCommand, DecrCommand};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tokio::sync::Barrier;

#[tokio::test]
async fn test_concurrent_set_get_operations() {
    let store = Arc::new(MemoryStore::new());
    let task_count = 100;
    let mut handles = Vec::new();
    
    // Spawn tasks that concurrently set and get different keys
    for i in 0..task_count {
        let store_clone = Arc::clone(&store);
        let handle = tokio::spawn(async move {
            let key = format!("concurrent_key_{}", i);
            let value = format!("concurrent_value_{}", i);
            
            // Set the key
            store_clone.set(&key, value.clone()).await.unwrap();
            
            // Immediately get it back
            let retrieved = store_clone.get(&key).unwrap().unwrap();
            assert_eq!(retrieved.value.to_string(), value);
            
            // Verify it exists
            assert!(store_clone.exists(&key).unwrap());
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Verify all keys were set
    assert_eq!(store.len(), task_count);
}

#[tokio::test]
async fn test_concurrent_increment_operations() {
    let store = Arc::new(MemoryStore::new());
    let task_count = 50;
    let increments_per_task = 10;
    let barrier = Arc::new(Barrier::new(task_count));
    let mut handles = Vec::new();
    
    // All tasks increment the same counter
    for i in 0..task_count {
        let store_clone = Arc::clone(&store);
        let barrier_clone = Arc::clone(&barrier);
        let handle = tokio::spawn(async move {
            // Wait for all tasks to be ready
            barrier_clone.wait().await;
            
            // Each task performs multiple increments
            for _ in 0..increments_per_task {
                let result = store_clone.incr("shared_counter").await.unwrap();
                assert!(result > 0);
            }
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Verify final counter value
    let final_value = store.get("shared_counter").unwrap().unwrap();
    let expected_value = task_count * increments_per_task;
    assert_eq!(final_value.value.to_integer().unwrap(), expected_value as i64);
}

#[tokio::test]
async fn test_concurrent_mixed_atomic_operations() {
    let store = Arc::new(MemoryStore::new());
    let task_count = 30;
    let barrier = Arc::new(Barrier::new(task_count));
    let mut handles = Vec::new();
    
    // Tasks perform mixed increment and decrement operations
    for i in 0..task_count {
        let store_clone = Arc::clone(&store);
        let barrier_clone = Arc::clone(&barrier);
        let handle = tokio::spawn(async move {
            barrier_clone.wait().await;
            
            if i % 2 == 0 {
                // Even tasks increment
                for _ in 0..5 {
                    store_clone.incr("mixed_counter").await.unwrap();
                }
            } else {
                // Odd tasks decrement
                for _ in 0..3 {
                    store_clone.decr("mixed_counter").await.unwrap();
                }
            }
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Calculate expected final value
    let even_tasks = (task_count + 1) / 2; // Round up for even count
    let odd_tasks = task_count / 2;
    let expected_value = (even_tasks * 5) - (odd_tasks * 3);
    
    let final_value = store.get("mixed_counter").unwrap().unwrap();
    assert_eq!(final_value.value.to_integer().unwrap(), expected_value as i64);
}

#[tokio::test]
async fn test_concurrent_key_overwrite() {
    let store = Arc::new(MemoryStore::new());
    let task_count = 20;
    let barrier = Arc::new(Barrier::new(task_count));
    let mut handles = Vec::new();
    
    // All tasks try to set the same key with different values
    for i in 0..task_count {
        let store_clone = Arc::clone(&store);
        let barrier_clone = Arc::clone(&barrier);
        let handle = tokio::spawn(async move {
            barrier_clone.wait().await;
            
            let value = format!("value_from_task_{}", i);
            store_clone.set("contested_key", value.clone()).await.unwrap();
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Verify that exactly one value won
    assert_eq!(store.len(), 1);
    let final_value = store.get("contested_key").unwrap().unwrap();
    let value_str = final_value.value.to_string();
    assert!(value_str.starts_with("value_from_task_"));
    
    // Extract task number and verify it's valid
    let task_num_str = value_str.strip_prefix("value_from_task_").unwrap();
    let task_num: usize = task_num_str.parse().unwrap();
    assert!(task_num < task_count);
}

#[tokio::test]
async fn test_concurrent_delete_operations() {
    let store = Arc::new(MemoryStore::new());
    let key_count = 50;
    
    // Pre-populate with keys
    for i in 0..key_count {
        let key = format!("delete_key_{}", i);
        store.set(&key, "value").await.unwrap();
    }
    
    assert_eq!(store.len(), key_count);
    
    let mut handles = Vec::new();
    
    // Spawn tasks to delete keys concurrently
    for i in 0..key_count {
        let store_clone = Arc::clone(&store);
        let handle = tokio::spawn(async move {
            let key = format!("delete_key_{}", i);
            let was_deleted = store_clone.delete(&key).await.unwrap();
            
            // Each key should be deleted exactly once
            assert!(was_deleted);
            
            // Verify key no longer exists
            assert!(!store_clone.exists(&key).unwrap());
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all deletions to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Verify all keys were deleted
    assert_eq!(store.len(), 0);
    assert!(store.is_empty());
}

#[tokio::test]
async fn test_concurrent_ttl_operations() {
    let store = Arc::new(MemoryStore::new());
    let key_count = 30;
    let barrier = Arc::new(Barrier::new(key_count));
    let mut handles = Vec::new();
    
    // Pre-populate with keys
    for i in 0..key_count {
        let key = format!("ttl_key_{}", i);
        store.set(&key, "value").await.unwrap();
    }
    
    // Concurrently set TTL on all keys
    for i in 0..key_count {
        let store_clone = Arc::clone(&store);
        let barrier_clone = Arc::clone(&barrier);
        let handle = tokio::spawn(async move {
            barrier_clone.wait().await;
            
            let key = format!("ttl_key_{}", i);
            let ttl_seconds = 60 + (i % 10); // Different TTL values
            
            let was_set = store_clone.expire(&key, ttl_seconds as u64).await.unwrap();
            assert!(was_set);
            
            // Verify TTL was set
            let ttl = store_clone.ttl(&key).unwrap();
            assert!(ttl > 0 && ttl <= ttl_seconds as i64);
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all TTL operations to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Verify all keys still exist and have TTL
    for i in 0..key_count {
        let key = format!("ttl_key_{}", i);
        assert!(store.exists(&key).unwrap());
        let ttl = store.ttl(&key).unwrap();
        assert!(ttl > 0);
    }
}

#[tokio::test]
async fn test_concurrent_expiration_cleanup() {
    let store = Arc::new(MemoryStore::new());
    let key_count = 20;
    let barrier = Arc::new(Barrier::new(key_count));
    let mut handles = Vec::new();
    
    // Concurrently set keys with very short TTL
    for i in 0..key_count {
        let store_clone = Arc::clone(&store);
        let barrier_clone = Arc::clone(&barrier);
        let handle = tokio::spawn(async move {
            barrier_clone.wait().await;
            
            let key = format!("expire_key_{}", i);
            // Set with very short TTL (10ms)
            store_clone.set_with_ttl(&key, "value", 1).await.unwrap();
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all keys to be set
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Wait for expiration
    sleep(Duration::from_millis(1100)).await;
    
    // Concurrently try to access expired keys
    let mut handles = Vec::new();
    for i in 0..key_count {
        let store_clone = Arc::clone(&store);
        let handle = tokio::spawn(async move {
            let key = format!("expire_key_{}", i);
            
            // Key should be expired and automatically cleaned up
            let result = store_clone.get(&key).unwrap();
            assert!(result.is_none());
            
            assert!(!store_clone.exists(&key).unwrap());
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all access attempts to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Store should be empty after cleanup
    assert_eq!(store.len(), 0);
}

#[tokio::test]
async fn test_concurrent_command_execution() {
    let store = Arc::new(MemoryStore::new());
    let mut registry = CommandRegistry::new();
    registry.register(Box::new(SetCommand));
    registry.register(Box::new(GetCommand));
    registry.register(Box::new(IncrCommand));
    registry.register(Box::new(DecrCommand));
    let registry = Arc::new(registry);
    
    let task_count = 25;
    let mut handles = Vec::new();
    
    // Concurrently execute different commands
    for i in 0..task_count {
        let store_clone = Arc::clone(&store);
        let registry_clone = Arc::clone(&registry);
        let handle = tokio::spawn(async move {
            match i % 4 {
                0 => {
                    // SET command
                    let key = format!("cmd_key_{}", i);
                    let value = format!("cmd_value_{}", i);
                    let args = vec![key, value];
                    let set_cmd = SetCommand;
                    let result = set_cmd.execute(&args, &store_clone).await;
                    assert!(result.is_ok());
                }
                1 => {
                    // GET command (may not find anything initially)
                    let key = format!("cmd_key_{}", i - 1);
                    let args = vec![key];
                    let get_cmd = GetCommand;
                    let _result = get_cmd.execute(&args, &store_clone).await;
                    // Result could be Ok or Nil, both are valid
                }
                2 => {
                    // INCR command
                    let counter = format!("cmd_counter_{}", i % 5);
                    let args = vec![counter];
                    let incr_cmd = IncrCommand;
                    let result = incr_cmd.execute(&args, &store_clone).await;
                    assert!(result.is_ok());
                }
                3 => {
                    // DECR command
                    let counter = format!("cmd_counter_{}", i % 5);
                    let args = vec![counter];
                    let decr_cmd = DecrCommand;
                    let result = decr_cmd.execute(&args, &store_clone).await;
                    assert!(result.is_ok());
                }
                _ => unreachable!(),
            }
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all command executions to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Verify store is in a consistent state
    assert!(store.len() > 0);
}

#[tokio::test]
async fn test_concurrent_memory_pressure() {
    let store = Arc::new(MemoryStore::new());
    let task_count = 20;
    let keys_per_task = 100;
    let mut handles = Vec::new();
    
    // Create memory pressure with many concurrent operations
    for i in 0..task_count {
        let store_clone = Arc::clone(&store);
        let handle = tokio::spawn(async move {
            // Each task creates many keys
            for j in 0..keys_per_task {
                let key = format!("pressure_{}_{}", i, j);
                let value = format!("value_{}_{}", i, j);
                store_clone.set(&key, value.clone()).await.unwrap();
                
                // Occasionally read and delete to create churn
                if j % 10 == 0 {
                    let _retrieved = store_clone.get(&key).unwrap();
                    if j % 20 == 0 {
                        store_clone.delete(&key).await.unwrap();
                    }
                }
            }
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Verify store is still functional
    let stats = store.memory_stats();
    assert!(stats.key_count > 0);
    assert!(stats.estimated_memory_bytes > 0);
    
    // Test that we can still perform operations
    store.set("final_test", "final_value").await.unwrap();
    let result = store.get("final_test").unwrap().unwrap();
    assert_eq!(result.value.to_string(), "final_value");
}

#[tokio::test]
async fn test_concurrent_type_conversions() {
    let store = Arc::new(MemoryStore::new());
    let task_count = 15;
    let barrier = Arc::new(Barrier::new(task_count));
    let mut handles = Vec::new();
    
    // Tasks concurrently set different types for the same keys
    for i in 0..task_count {
        let store_clone = Arc::clone(&store);
        let barrier_clone = Arc::clone(&barrier);
        let handle = tokio::spawn(async move {
            barrier_clone.wait().await;
            
            let key = format!("type_key_{}", i % 5); // 5 contested keys
            
            if i % 2 == 0 {
                // Set as string
                let value = format!("string_value_{}", i);
                store_clone.set(&key, value.clone()).await.unwrap();
            } else {
                // Set as integer
                let value = i as i64;
                store_clone.set(&key, value).await.unwrap();
            }
            
            // Try to read it back
            let result = store_clone.get(&key).unwrap();
            assert!(result.is_some());
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Verify all contested keys have some value
    for i in 0..5 {
        let key = format!("type_key_{}", i);
        assert!(store.exists(&key).unwrap());
        let value = store.get(&key).unwrap().unwrap();
        // Value could be string or integer, both are valid
        assert!(value.value.is_string() || value.value.is_integer());
    }
}

#[tokio::test]
async fn test_concurrent_error_conditions() {
    let store = Arc::new(MemoryStore::new());
    let task_count = 20;
    let mut handles = Vec::new();
    
    // Set up some initial data
    store.set("string_key", "not_a_number").await.unwrap();
    store.set("integer_key", 42i64).await.unwrap();
    
    // Tasks concurrently try operations that may fail
    for i in 0..task_count {
        let store_clone = Arc::clone(&store);
        let handle = tokio::spawn(async move {
            match i % 4 {
                0 => {
                    // Try to increment a string (should fail)
                    let result = store_clone.incr("string_key").await;
                    assert!(result.is_err());
                }
                1 => {
                    // Try to increment a non-existent key (should succeed)
                    let key = format!("new_counter_{}", i);
                    let result = store_clone.incr(&key).await;
                    assert!(result.is_ok());
                    assert_eq!(result.unwrap(), 1);
                }
                2 => {
                    // Try to set TTL on non-existent key (should fail)
                    let key = format!("nonexistent_{}", i);
                    let result = store_clone.expire(&key, 60).await;
                    assert!(result.is_ok());
                    assert!(!result.unwrap()); // Should return false
                }
                3 => {
                    // Normal operations (should succeed)
                    let key = format!("normal_key_{}", i);
                    store_clone.set(&key, "normal_value").await.unwrap();
                    assert!(store_clone.exists(&key).unwrap());
                }
                _ => unreachable!(),
            }
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Verify original data is still intact
    let string_value = store.get("string_key").unwrap().unwrap();
    assert_eq!(string_value.value.to_string(), "not_a_number");
    
    let integer_value = store.get("integer_key").unwrap().unwrap();
    assert_eq!(integer_value.value.to_integer().unwrap(), 42);
}

#[tokio::test]
async fn test_concurrent_cleanup_operations() {
    let store = Arc::new(MemoryStore::new());
    let task_count = 10;
    let keys_per_task = 50;
    let mut handles = Vec::new();
    
    // Tasks create keys with expiration and then trigger cleanup
    for i in 0..task_count {
        let store_clone = Arc::clone(&store);
        let handle = tokio::spawn(async move {
            // Create keys with short expiration
            for j in 0..keys_per_task {
                let key = format!("cleanup_{}_{}", i, j);
                if j % 2 == 0 {
                    // Half with very short TTL
                    store_clone.set_with_ttl(&key, "temp_value", 1).await.unwrap();
                } else {
                    // Half with longer TTL
                    store_clone.set_with_ttl(&key, "persistent_value", 60).await.unwrap();
                }
            }
            
            // Wait for short TTL keys to expire
            sleep(Duration::from_millis(1100)).await;
            
            // Trigger cleanup by accessing keys
            for j in 0..keys_per_task {
                let key = format!("cleanup_{}_{}", i, j);
                let _result = store_clone.get(&key).unwrap();
            }
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Verify that approximately half the keys remain (those with longer TTL)
    let remaining_keys = store.len();
    let expected_remaining = (task_count * keys_per_task) / 2;
    
    // Allow some tolerance for timing variations
    let tolerance = expected_remaining / 10;
    assert!(remaining_keys >= expected_remaining - tolerance);
    assert!(remaining_keys <= expected_remaining + tolerance);
}