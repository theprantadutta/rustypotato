//! String command implementations (SET, GET, DEL, EXISTS)

use crate::commands::{Command, CommandArity, CommandResult, ResponseValue};
use crate::storage::MemoryStore;
use async_trait::async_trait;

/// SET command implementation
/// Sets a key to hold the string value
pub struct SetCommand;

#[async_trait]
impl Command for SetCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if args.len() != 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'SET' command".to_string(),
            );
        }

        let key = &args[0];
        let value = &args[1];

        match store.set(key, value.as_str()).await {
            Ok(()) => CommandResult::Ok(ResponseValue::SimpleString("OK".to_string())),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "SET"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(3) // SET key value
    }
}

/// GET command implementation
/// Gets the value of a key
pub struct GetCommand;

#[async_trait]
impl Command for GetCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'GET' command".to_string(),
            );
        }

        let key = &args[0];

        match store.get(key) {
            Ok(Some(stored_value)) => CommandResult::Ok(ResponseValue::BulkString(Some(
                stored_value.value.to_string(),
            ))),
            Ok(None) => CommandResult::Ok(ResponseValue::BulkString(None)), // Redis returns nil for missing keys
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "GET"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2) // GET key
    }
}

/// DEL command implementation
/// Removes the specified keys
pub struct DelCommand;

#[async_trait]
impl Command for DelCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if args.is_empty() {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'DEL' command".to_string(),
            );
        }

        let mut deleted_count = 0i64;

        for key in args {
            match store.delete(key).await {
                Ok(true) => deleted_count += 1,
                Ok(false) => {} // Key didn't exist, continue
                Err(e) => return CommandResult::Error(e.to_client_error()),
            }
        }

        CommandResult::Ok(ResponseValue::Integer(deleted_count))
    }

    fn name(&self) -> &'static str {
        "DEL"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::AtLeast(2) // DEL key [key ...]
    }
}

/// EXISTS command implementation
/// Determines if a key exists
pub struct ExistsCommand;

#[async_trait]
impl Command for ExistsCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if args.is_empty() {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'EXISTS' command".to_string(),
            );
        }

        let mut exists_count = 0i64;

        for key in args {
            match store.exists(key) {
                Ok(true) => exists_count += 1,
                Ok(false) => {} // Key doesn't exist, continue
                Err(e) => return CommandResult::Error(e.to_client_error()),
            }
        }

        CommandResult::Ok(ResponseValue::Integer(exists_count))
    }

    fn name(&self) -> &'static str {
        "EXISTS"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::AtLeast(2) // EXISTS key [key ...]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::ResponseValue;
    use crate::storage::MemoryStore;

    // Helper function to create a test store
    fn create_test_store() -> MemoryStore {
        MemoryStore::new()
    }

    // SET command tests
    #[tokio::test]
    async fn test_set_command_success() {
        let store = create_test_store();
        let cmd = SetCommand;
        let args = vec!["test_key".to_string(), "test_value".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::SimpleString(msg)) => {
                assert_eq!(msg, "OK");
            }
            _ => panic!("Expected OK response from SET command"),
        }

        // Verify the value was actually stored
        let stored = store.get("test_key").unwrap().unwrap();
        assert_eq!(stored.value.to_string(), "test_value");
    }

    #[tokio::test]
    async fn test_set_command_overwrite() {
        let store = create_test_store();
        let cmd = SetCommand;

        // Set initial value
        let args1 = vec!["key1".to_string(), "value1".to_string()];
        let result1 = cmd.execute(&args1, &store).await;
        assert!(matches!(
            result1,
            CommandResult::Ok(ResponseValue::SimpleString(_))
        ));

        // Overwrite with new value
        let args2 = vec!["key1".to_string(), "value2".to_string()];
        let result2 = cmd.execute(&args2, &store).await;
        assert!(matches!(
            result2,
            CommandResult::Ok(ResponseValue::SimpleString(_))
        ));

        // Verify the new value
        let stored = store.get("key1").unwrap().unwrap();
        assert_eq!(stored.value.to_string(), "value2");
    }

    #[tokio::test]
    async fn test_set_command_wrong_args_too_few() {
        let store = create_test_store();
        let cmd = SetCommand;
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
    async fn test_set_command_wrong_args_too_many() {
        let store = create_test_store();
        let cmd = SetCommand;
        let args = vec!["key".to_string(), "value".to_string(), "extra".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("wrong number of arguments"));
            }
            _ => panic!("Expected error for wrong number of arguments"),
        }
    }

    #[tokio::test]
    async fn test_set_command_empty_key_and_value() {
        let store = create_test_store();
        let cmd = SetCommand;
        let args = vec!["".to_string(), "".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::SimpleString(msg)) => {
                assert_eq!(msg, "OK");
            }
            _ => panic!("Expected OK response even for empty key/value"),
        }

        // Verify empty key and value were stored
        let stored = store.get("").unwrap().unwrap();
        assert_eq!(stored.value.to_string(), "");
    }

    #[test]
    fn test_set_command_properties() {
        let cmd = SetCommand;
        assert_eq!(cmd.name(), "SET");
        assert_eq!(cmd.arity(), CommandArity::Fixed(3));
    }

    // GET command tests
    #[tokio::test]
    async fn test_get_command_existing_key() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();

        let cmd = GetCommand;
        let args = vec!["test_key".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(Some(value))) => {
                assert_eq!(value, "test_value");
            }
            _ => panic!("Expected bulk string response from GET command"),
        }
    }

    #[tokio::test]
    async fn test_get_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = GetCommand;
        let args = vec!["nonexistent_key".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(None)) => {
                // This is the expected behavior for missing keys
            }
            _ => panic!("Expected nil (BulkString(None)) response for nonexistent key"),
        }
    }

    #[tokio::test]
    async fn test_get_command_integer_value() {
        let store = create_test_store();
        store.set("int_key", 42i64).await.unwrap();

        let cmd = GetCommand;
        let args = vec!["int_key".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(Some(value))) => {
                assert_eq!(value, "42");
            }
            _ => panic!("Expected bulk string response from GET command for integer"),
        }
    }

    #[tokio::test]
    async fn test_get_command_wrong_args_too_few() {
        let store = create_test_store();
        let cmd = GetCommand;
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
    async fn test_get_command_wrong_args_too_many() {
        let store = create_test_store();
        let cmd = GetCommand;
        let args = vec!["key1".to_string(), "key2".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("wrong number of arguments"));
            }
            _ => panic!("Expected error for wrong number of arguments"),
        }
    }

    #[tokio::test]
    async fn test_get_command_empty_key() {
        let store = create_test_store();
        store.set("", "empty_key_value").await.unwrap();

        let cmd = GetCommand;
        let args = vec!["".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(Some(value))) => {
                assert_eq!(value, "empty_key_value");
            }
            _ => panic!("Expected bulk string response for empty key"),
        }
    }

    #[test]
    fn test_get_command_properties() {
        let cmd = GetCommand;
        assert_eq!(cmd.name(), "GET");
        assert_eq!(cmd.arity(), CommandArity::Fixed(2));
    }

    // DEL command tests
    #[tokio::test]
    async fn test_del_command_single_existing_key() {
        let store = create_test_store();
        store.set("key1", "value1").await.unwrap();

        let cmd = DelCommand;
        let args = vec!["key1".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 1);
            }
            _ => panic!("Expected integer response from DEL command"),
        }

        // Verify key was deleted
        assert!(!store.exists("key1").unwrap());
    }

    #[tokio::test]
    async fn test_del_command_single_nonexistent_key() {
        let store = create_test_store();
        let cmd = DelCommand;
        let args = vec!["nonexistent".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 0);
            }
            _ => panic!("Expected integer response from DEL command"),
        }
    }

    #[tokio::test]
    async fn test_del_command_multiple_keys() {
        let store = create_test_store();
        store.set("key1", "value1").await.unwrap();
        store.set("key2", "value2").await.unwrap();
        store.set("key3", "value3").await.unwrap();

        let cmd = DelCommand;
        let args = vec![
            "key1".to_string(),
            "key2".to_string(),
            "nonexistent".to_string(),
            "key3".to_string(),
        ];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 3); // key1, key2, key3 deleted; nonexistent didn't exist
            }
            _ => panic!("Expected integer response from DEL command"),
        }

        // Verify keys were deleted
        assert!(!store.exists("key1").unwrap());
        assert!(!store.exists("key2").unwrap());
        assert!(!store.exists("key3").unwrap());
    }

    #[tokio::test]
    async fn test_del_command_duplicate_keys() {
        let store = create_test_store();
        store.set("key1", "value1").await.unwrap();

        let cmd = DelCommand;
        let args = vec!["key1".to_string(), "key1".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 1); // Only counted once since second attempt finds key already deleted
            }
            _ => panic!("Expected integer response from DEL command"),
        }
    }

    #[tokio::test]
    async fn test_del_command_no_args() {
        let store = create_test_store();
        let cmd = DelCommand;
        let args = vec![];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("wrong number of arguments"));
            }
            _ => panic!("Expected error for no arguments"),
        }
    }

    #[test]
    fn test_del_command_properties() {
        let cmd = DelCommand;
        assert_eq!(cmd.name(), "DEL");
        assert_eq!(cmd.arity(), CommandArity::AtLeast(2));
    }

    // EXISTS command tests
    #[tokio::test]
    async fn test_exists_command_single_existing_key() {
        let store = create_test_store();
        store.set("key1", "value1").await.unwrap();

        let cmd = ExistsCommand;
        let args = vec!["key1".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 1);
            }
            _ => panic!("Expected integer response from EXISTS command"),
        }
    }

    #[tokio::test]
    async fn test_exists_command_single_nonexistent_key() {
        let store = create_test_store();
        let cmd = ExistsCommand;
        let args = vec!["nonexistent".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 0);
            }
            _ => panic!("Expected integer response from EXISTS command"),
        }
    }

    #[tokio::test]
    async fn test_exists_command_multiple_keys() {
        let store = create_test_store();
        store.set("key1", "value1").await.unwrap();
        store.set("key3", "value3").await.unwrap();

        let cmd = ExistsCommand;
        let args = vec!["key1".to_string(), "key2".to_string(), "key3".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 2); // key1 and key3 exist, key2 doesn't
            }
            _ => panic!("Expected integer response from EXISTS command"),
        }
    }

    #[tokio::test]
    async fn test_exists_command_duplicate_keys() {
        let store = create_test_store();
        store.set("key1", "value1").await.unwrap();

        let cmd = ExistsCommand;
        let args = vec!["key1".to_string(), "key1".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 2); // Each key existence is counted separately
            }
            _ => panic!("Expected integer response from EXISTS command"),
        }
    }

    #[tokio::test]
    async fn test_exists_command_no_args() {
        let store = create_test_store();
        let cmd = ExistsCommand;
        let args = vec![];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("wrong number of arguments"));
            }
            _ => panic!("Expected error for no arguments"),
        }
    }

    #[test]
    fn test_exists_command_properties() {
        let cmd = ExistsCommand;
        assert_eq!(cmd.name(), "EXISTS");
        assert_eq!(cmd.arity(), CommandArity::AtLeast(2));
    }

    // Integration tests with expired keys
    #[tokio::test]
    async fn test_get_command_with_expired_key() {
        let store = create_test_store();

        // Set a key with very short TTL
        let expires_at = std::time::Instant::now() + std::time::Duration::from_millis(1);
        store
            .set_with_expiration("expired_key", "value", expires_at)
            .await
            .unwrap();

        // Wait for expiration
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let cmd = GetCommand;
        let args = vec!["expired_key".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(None)) => {
                // Expected: expired key should return nil
            }
            _ => panic!("Expected nil response for expired key"),
        }
    }

    #[tokio::test]
    async fn test_exists_command_with_expired_key() {
        let store = create_test_store();

        // Set a key with very short TTL
        let expires_at = std::time::Instant::now() + std::time::Duration::from_millis(1);
        store
            .set_with_expiration("expired_key", "value", expires_at)
            .await
            .unwrap();

        // Wait for expiration
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let cmd = ExistsCommand;
        let args = vec!["expired_key".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 0); // Expired key should not exist
            }
            _ => panic!("Expected integer response from EXISTS command"),
        }
    }

    #[tokio::test]
    async fn test_del_command_with_expired_key() {
        let store = create_test_store();

        // Set a key with very short TTL
        let expires_at = std::time::Instant::now() + std::time::Duration::from_millis(1);
        store
            .set_with_expiration("expired_key", "value", expires_at)
            .await
            .unwrap();

        // Wait for expiration
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let cmd = DelCommand;
        let args = vec!["expired_key".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 0); // Expired key can't be deleted (already gone)
            }
            _ => panic!("Expected integer response from DEL command"),
        }
    }

    // Edge case tests
    #[tokio::test]
    async fn test_commands_with_unicode_keys_and_values() {
        let store = create_test_store();

        // Test SET with unicode
        let set_cmd = SetCommand;
        let unicode_key = "🔑";
        let unicode_value = "🎯 测试值";
        let args = vec![unicode_key.to_string(), unicode_value.to_string()];

        let result = set_cmd.execute(&args, &store).await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::SimpleString(_))
        ));

        // Test GET with unicode
        let get_cmd = GetCommand;
        let args = vec![unicode_key.to_string()];

        let result = get_cmd.execute(&args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::BulkString(Some(value))) => {
                assert_eq!(value, unicode_value);
            }
            _ => panic!("Expected unicode value from GET command"),
        }

        // Test EXISTS with unicode
        let exists_cmd = ExistsCommand;
        let args = vec![unicode_key.to_string()];

        let result = exists_cmd.execute(&args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 1);
            }
            _ => panic!("Expected EXISTS to work with unicode keys"),
        }

        // Test DEL with unicode
        let del_cmd = DelCommand;
        let args = vec![unicode_key.to_string()];

        let result = del_cmd.execute(&args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 1);
            }
            _ => panic!("Expected DEL to work with unicode keys"),
        }
    }

    #[tokio::test]
    async fn test_commands_with_very_long_keys_and_values() {
        let store = create_test_store();

        // Create very long key and value
        let long_key = "k".repeat(1000);
        let long_value = "v".repeat(10000);

        // Test SET with long data
        let set_cmd = SetCommand;
        let args = vec![long_key.clone(), long_value.clone()];

        let result = set_cmd.execute(&args, &store).await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::SimpleString(_))
        ));

        // Test GET with long key
        let get_cmd = GetCommand;
        let args = vec![long_key.clone()];

        let result = get_cmd.execute(&args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::BulkString(Some(value))) => {
                assert_eq!(value, long_value);
            }
            _ => panic!("Expected long value from GET command"),
        }
    }

    // Concurrent access tests
    #[tokio::test]
    async fn test_concurrent_command_execution() {
        use std::sync::Arc;
        use tokio::task::JoinSet;

        let store = Arc::new(create_test_store());
        let mut join_set = JoinSet::new();

        // Spawn multiple tasks executing different commands concurrently
        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            join_set.spawn(async move {
                let key = format!("key{i}");
                let value = format!("value{i}");

                // SET
                let set_cmd = SetCommand;
                let args = vec![key.clone(), value.clone()];
                let result = set_cmd.execute(&args, &store_clone).await;
                assert!(matches!(
                    result,
                    CommandResult::Ok(ResponseValue::SimpleString(_))
                ));

                // GET
                let get_cmd = GetCommand;
                let args = vec![key.clone()];
                let result = get_cmd.execute(&args, &store_clone).await;
                match result {
                    CommandResult::Ok(ResponseValue::BulkString(Some(retrieved_value))) => {
                        assert_eq!(retrieved_value, value);
                    }
                    _ => panic!("Expected value from concurrent GET"),
                }

                // EXISTS
                let exists_cmd = ExistsCommand;
                let args = vec![key.clone()];
                let result = exists_cmd.execute(&args, &store_clone).await;
                match result {
                    CommandResult::Ok(ResponseValue::Integer(count)) => {
                        assert_eq!(count, 1);
                    }
                    _ => panic!("Expected EXISTS to return 1"),
                }
            });
        }

        // Wait for all tasks to complete
        while let Some(result) = join_set.join_next().await {
            result.unwrap(); // Panic if any task failed
        }

        // Verify all keys exist
        assert_eq!(store.len(), 10);
    }
}
