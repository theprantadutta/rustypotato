//! TTL command implementations (EXPIRE, TTL)

use crate::commands::{Command, CommandArity, CommandResult, ResponseValue};
use crate::storage::MemoryStore;
use async_trait::async_trait;
use std::time::Duration;

/// EXPIRE command implementation
/// Sets a timeout on a key. After the timeout has expired, the key will automatically be deleted.
pub struct ExpireCommand;

#[async_trait]
impl Command for ExpireCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if args.len() != 2 {
            return CommandResult::Error("ERR wrong number of arguments for 'EXPIRE' command".to_string());
        }
        
        let key = &args[0];
        let ttl_str = &args[1];
        
        // Parse TTL seconds
        let ttl_seconds = match ttl_str.parse::<u64>() {
            Ok(seconds) => seconds,
            Err(_) => {
                return CommandResult::Error("ERR value is not an integer or out of range".to_string());
            }
        };
        
        match store.expire(key, ttl_seconds).await {
            Ok(true) => CommandResult::Ok(ResponseValue::Integer(1)), // Key exists and expiration was set
            Ok(false) => CommandResult::Ok(ResponseValue::Integer(0)), // Key doesn't exist
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }
    
    fn name(&self) -> &'static str {
        "EXPIRE"
    }
    
    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(3) // EXPIRE key seconds
    }
}

/// TTL command implementation
/// Returns the remaining time to live of a key that has a timeout.
pub struct TtlCommand;

#[async_trait]
impl Command for TtlCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error("ERR wrong number of arguments for 'TTL' command".to_string());
        }
        
        let key = &args[0];
        
        match store.ttl(key) {
            Ok(ttl) => CommandResult::Ok(ResponseValue::Integer(ttl)),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }
    
    fn name(&self) -> &'static str {
        "TTL"
    }
    
    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2) // TTL key
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStore;
    use crate::commands::ResponseValue;
    use std::time::Duration;

    // Helper function to create a test store
    fn create_test_store() -> MemoryStore {
        MemoryStore::new()
    }

    // EXPIRE command tests
    #[tokio::test]
    async fn test_expire_command_existing_key() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();
        
        let cmd = ExpireCommand;
        let args = vec!["test_key".to_string(), "60".to_string()];
        
        let result = cmd.execute(&args, &store).await;
        
        match result {
            CommandResult::Ok(ResponseValue::Integer(1)) => {
                // Expected: key exists and expiration was set
            }
            _ => panic!("Expected integer 1 for successful EXPIRE on existing key"),
        }
        
        // Verify TTL was set
        let ttl = store.ttl("test_key").unwrap();
        assert!(ttl > 0 && ttl <= 60, "Expected TTL between 1 and 60, got {}", ttl);
    }
    
    #[tokio::test]
    async fn test_expire_command_nonexistent_key() {
        let store = create_test_store();
        
        let cmd = ExpireCommand;
        let args = vec!["nonexistent_key".to_string(), "60".to_string()];
        
        let result = cmd.execute(&args, &store).await;
        
        match result {
            CommandResult::Ok(ResponseValue::Integer(0)) => {
                // Expected: key doesn't exist
            }
            _ => panic!("Expected integer 0 for EXPIRE on nonexistent key"),
        }
    }
    
    #[tokio::test]
    async fn test_expire_command_zero_ttl() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();
        
        let cmd = ExpireCommand;
        let args = vec!["test_key".to_string(), "0".to_string()];
        
        let result = cmd.execute(&args, &store).await;
        
        match result {
            CommandResult::Ok(ResponseValue::Integer(1)) => {
                // Expected: key exists and expiration was set (immediate expiration)
            }
            _ => panic!("Expected integer 1 for EXPIRE with 0 TTL"),
        }
        
        // Key should be expired immediately or very soon
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!store.exists("test_key").unwrap(), "Key should be expired with 0 TTL");
    }
    
    #[tokio::test]
    async fn test_expire_command_large_ttl() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();
        
        let cmd = ExpireCommand;
        let args = vec!["test_key".to_string(), "2147483647".to_string()]; // Large but valid u64
        
        let result = cmd.execute(&args, &store).await;
        
        match result {
            CommandResult::Ok(ResponseValue::Integer(1)) => {
                // Expected: key exists and expiration was set
            }
            _ => panic!("Expected integer 1 for EXPIRE with large TTL"),
        }
        
        let ttl = store.ttl("test_key").unwrap();
        assert!(ttl > 2147483640, "Expected large TTL, got {}", ttl);
    }
    
    #[tokio::test]
    async fn test_expire_command_invalid_ttl_non_numeric() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();
        
        let cmd = ExpireCommand;
        let args = vec!["test_key".to_string(), "not_a_number".to_string()];
        
        let result = cmd.execute(&args, &store).await;
        
        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("value is not an integer"));
            }
            _ => panic!("Expected error for non-numeric TTL"),
        }
    }
    
    #[tokio::test]
    async fn test_expire_command_invalid_ttl_negative() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();
        
        let cmd = ExpireCommand;
        let args = vec!["test_key".to_string(), "-1".to_string()];
        
        let result = cmd.execute(&args, &store).await;
        
        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("value is not an integer"));
            }
            _ => panic!("Expected error for negative TTL"),
        }
    }
    
    #[tokio::test]
    async fn test_expire_command_wrong_args_too_few() {
        let store = create_test_store();
        let cmd = ExpireCommand;
        let args = vec!["only_key".to_string()];
        
        let result = cmd.execute(&args, &store).await;
        
        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("wrong number of arguments"));
            }
            _ => panic!("Expected error for wrong number of arguments"),
        }
    }
    
    #[tokio::test]
    async fn test_expire_command_wrong_args_too_many() {
        let store = create_test_store();
        let cmd = ExpireCommand;
        let args = vec!["key".to_string(), "60".to_string(), "extra".to_string()];
        
        let result = cmd.execute(&args, &store).await;
        
        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("wrong number of arguments"));
            }
            _ => panic!("Expected error for wrong number of arguments"),
        }
    }
    
    #[tokio::test]
    async fn test_expire_command_overwrite_existing_expiration() {
        let store = create_test_store();
        store.set_with_ttl("test_key", "test_value", 120).await.unwrap();
        
        // Verify initial TTL
        let initial_ttl = store.ttl("test_key").unwrap();
        assert!(initial_ttl > 100, "Expected initial TTL > 100, got {}", initial_ttl);
        
        // Set new expiration
        let cmd = ExpireCommand;
        let args = vec!["test_key".to_string(), "30".to_string()];
        
        let result = cmd.execute(&args, &store).await;
        
        match result {
            CommandResult::Ok(ResponseValue::Integer(1)) => {
                // Expected: key exists and expiration was updated
            }
            _ => panic!("Expected integer 1 for EXPIRE overwriting existing expiration"),
        }
        
        // Verify TTL was updated
        let new_ttl = store.ttl("test_key").unwrap();
        assert!(new_ttl <= 30 && new_ttl > 0, "Expected new TTL <= 30, got {}", new_ttl);
    }
    
    #[test]
    fn test_expire_command_properties() {
        let cmd = ExpireCommand;
        assert_eq!(cmd.name(), "EXPIRE");
        assert_eq!(cmd.arity(), CommandArity::Fixed(3));
    }

    // TTL command tests
    #[tokio::test]
    async fn test_ttl_command_key_with_expiration() {
        let store = create_test_store();
        store.set_with_ttl("test_key", "test_value", 60).await.unwrap();
        
        let cmd = TtlCommand;
        let args = vec!["test_key".to_string()];
        
        let result = cmd.execute(&args, &store).await;
        
        match result {
            CommandResult::Ok(ResponseValue::Integer(ttl)) => {
                assert!(ttl > 0 && ttl <= 60, "Expected TTL between 1 and 60, got {}", ttl);
            }
            _ => panic!("Expected integer TTL for key with expiration"),
        }
    }
    
    #[tokio::test]
    async fn test_ttl_command_key_without_expiration() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();
        
        let cmd = TtlCommand;
        let args = vec!["test_key".to_string()];
        
        let result = cmd.execute(&args, &store).await;
        
        match result {
            CommandResult::Ok(ResponseValue::Integer(-1)) => {
                // Expected: -1 for key without expiration
            }
            _ => panic!("Expected integer -1 for key without expiration"),
        }
    }
    
    #[tokio::test]
    async fn test_ttl_command_nonexistent_key() {
        let store = create_test_store();
        
        let cmd = TtlCommand;
        let args = vec!["nonexistent_key".to_string()];
        
        let result = cmd.execute(&args, &store).await;
        
        match result {
            CommandResult::Ok(ResponseValue::Integer(-2)) => {
                // Expected: -2 for nonexistent key
            }
            _ => panic!("Expected integer -2 for nonexistent key"),
        }
    }
    
    #[tokio::test]
    async fn test_ttl_command_expired_key() {
        let store = create_test_store();
        
        // Set key with very short TTL
        let expires_at = std::time::Instant::now() + Duration::from_millis(1);
        store.set_with_expiration("expired_key", "value", expires_at).await.unwrap();
        
        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(10)).await;
        
        let cmd = TtlCommand;
        let args = vec!["expired_key".to_string()];
        
        let result = cmd.execute(&args, &store).await;
        
        match result {
            CommandResult::Ok(ResponseValue::Integer(-2)) => {
                // Expected: -2 for expired key (treated as nonexistent)
            }
            _ => panic!("Expected integer -2 for expired key"),
        }
    }
    
    #[tokio::test]
    async fn test_ttl_command_wrong_args_too_few() {
        let store = create_test_store();
        let cmd = TtlCommand;
        let args = vec![];
        
        let result = cmd.execute(&args, &store).await;
        
        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("wrong number of arguments"));
            }
            _ => panic!("Expected error for wrong number of arguments"),
        }
    }
    
    #[tokio::test]
    async fn test_ttl_command_wrong_args_too_many() {
        let store = create_test_store();
        let cmd = TtlCommand;
        let args = vec!["key1".to_string(), "key2".to_string()];
        
        let result = cmd.execute(&args, &store).await;
        
        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("wrong number of arguments"));
            }
            _ => panic!("Expected error for wrong number of arguments"),
        }
    }
    
    #[test]
    fn test_ttl_command_properties() {
        let cmd = TtlCommand;
        assert_eq!(cmd.name(), "TTL");
        assert_eq!(cmd.arity(), CommandArity::Fixed(2));
    }

    // Integration tests between EXPIRE and TTL commands
    #[tokio::test]
    async fn test_expire_ttl_integration() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();
        
        // Initially no expiration
        let ttl_cmd = TtlCommand;
        let ttl_args = vec!["test_key".to_string()];
        let result = ttl_cmd.execute(&ttl_args, &store).await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(-1)));
        
        // Set expiration
        let expire_cmd = ExpireCommand;
        let expire_args = vec!["test_key".to_string(), "30".to_string()];
        let result = expire_cmd.execute(&expire_args, &store).await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(1)));
        
        // Check TTL is now positive
        let result = ttl_cmd.execute(&ttl_args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Integer(ttl)) => {
                assert!(ttl > 0 && ttl <= 30, "Expected TTL between 1 and 30, got {}", ttl);
            }
            _ => panic!("Expected positive TTL after EXPIRE"),
        }
    }
    
    #[tokio::test]
    async fn test_expire_ttl_timing_accuracy() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();
        
        // Set 5 second expiration
        let expire_cmd = ExpireCommand;
        let expire_args = vec!["test_key".to_string(), "5".to_string()];
        expire_cmd.execute(&expire_args, &store).await;
        
        // Check TTL immediately
        let ttl_cmd = TtlCommand;
        let ttl_args = vec!["test_key".to_string()];
        let result = ttl_cmd.execute(&ttl_args, &store).await;
        
        match result {
            CommandResult::Ok(ResponseValue::Integer(ttl)) => {
                assert!(ttl >= 4 && ttl <= 5, "Expected TTL between 4 and 5, got {}", ttl);
            }
            _ => panic!("Expected TTL between 4 and 5 seconds"),
        }
        
        // Wait 1 second and check again
        tokio::time::sleep(Duration::from_secs(1)).await;
        let result = ttl_cmd.execute(&ttl_args, &store).await;
        
        match result {
            CommandResult::Ok(ResponseValue::Integer(ttl)) => {
                assert!(ttl >= 3 && ttl <= 4, "Expected TTL between 3 and 4 after 1 second, got {}", ttl);
            }
            _ => panic!("Expected TTL between 3 and 4 seconds after waiting"),
        }
    }
    
    #[tokio::test]
    async fn test_expire_ttl_with_key_updates() {
        let store = create_test_store();
        store.set("test_key", "original_value").await.unwrap();
        
        // Set expiration
        let expire_cmd = ExpireCommand;
        let expire_args = vec!["test_key".to_string(), "60".to_string()];
        expire_cmd.execute(&expire_args, &store).await;
        
        // Verify TTL is set
        let ttl_cmd = TtlCommand;
        let ttl_args = vec!["test_key".to_string()];
        let result = ttl_cmd.execute(&ttl_args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Integer(ttl)) => {
                assert!(ttl > 0, "Expected positive TTL, got {}", ttl);
            }
            _ => panic!("Expected positive TTL"),
        }
        
        // Update the key value (should preserve expiration in our implementation)
        store.set("test_key", "new_value").await.unwrap();
        
        // Check if TTL is preserved or reset (depends on implementation)
        // In our current implementation, SET removes expiration
        let result = ttl_cmd.execute(&ttl_args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Integer(-1)) => {
                // Expected: SET removes expiration in our implementation
            }
            _ => panic!("Expected TTL to be reset after SET operation"),
        }
    }
    
    #[tokio::test]
    async fn test_expire_ttl_edge_cases() {
        let store = create_test_store();
        
        // Test with empty key name
        store.set("", "empty_key_value").await.unwrap();
        
        let expire_cmd = ExpireCommand;
        let expire_args = vec!["".to_string(), "60".to_string()];
        let result = expire_cmd.execute(&expire_args, &store).await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(1)));
        
        let ttl_cmd = TtlCommand;
        let ttl_args = vec!["".to_string()];
        let result = ttl_cmd.execute(&ttl_args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Integer(ttl)) => {
                assert!(ttl > 0, "Expected positive TTL for empty key, got {}", ttl);
            }
            _ => panic!("Expected positive TTL for empty key"),
        }
    }
    
    #[tokio::test]
    async fn test_expire_ttl_unicode_keys() {
        let store = create_test_store();
        let unicode_key = "🔑测试";
        store.set(unicode_key, "unicode_value").await.unwrap();
        
        // Set expiration on unicode key
        let expire_cmd = ExpireCommand;
        let expire_args = vec![unicode_key.to_string(), "45".to_string()];
        let result = expire_cmd.execute(&expire_args, &store).await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(1)));
        
        // Check TTL for unicode key
        let ttl_cmd = TtlCommand;
        let ttl_args = vec![unicode_key.to_string()];
        let result = ttl_cmd.execute(&ttl_args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Integer(ttl)) => {
                assert!(ttl > 0 && ttl <= 45, "Expected TTL between 1 and 45 for unicode key, got {}", ttl);
            }
            _ => panic!("Expected positive TTL for unicode key"),
        }
    }

    // Concurrent access tests
    #[tokio::test]
    async fn test_concurrent_expire_ttl_operations() {
        use std::sync::Arc;
        use tokio::task::JoinSet;
        
        let store = Arc::new(create_test_store());
        
        // Pre-populate with keys
        for i in 0..10 {
            let key = format!("key{}", i);
            store.set(&key, format!("value{}", i)).await.unwrap();
        }
        
        let mut join_set = JoinSet::new();
        
        // Spawn tasks that set expiration concurrently
        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            join_set.spawn(async move {
                let key = format!("key{}", i);
                let ttl = 60 + i; // Different TTL for each key
                
                let expire_cmd = ExpireCommand;
                let expire_args = vec![key.clone(), ttl.to_string()];
                let result = expire_cmd.execute(&expire_args, &store_clone).await;
                assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(1)));
                
                // Check TTL
                let ttl_cmd = TtlCommand;
                let ttl_args = vec![key.clone()];
                let result = ttl_cmd.execute(&ttl_args, &store_clone).await;
                match result {
                    CommandResult::Ok(ResponseValue::Integer(actual_ttl)) => {
                        assert!(actual_ttl > 0, "Expected positive TTL for key {}, got {}", key, actual_ttl);
                    }
                    _ => panic!("Expected positive TTL for key {}", key),
                }
            });
        }
        
        // Wait for all tasks to complete
        while let Some(result) = join_set.join_next().await {
            result.unwrap(); // Panic if any task failed
        }
        
        // Verify all keys have expiration set
        for i in 0..10 {
            let key = format!("key{}", i);
            let ttl = store.ttl(&key).unwrap();
            assert!(ttl > 0, "Expected positive TTL for key {}, got {}", key, ttl);
        }
    }
}