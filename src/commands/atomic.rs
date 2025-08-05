//! Atomic operation command implementations (INCR, DECR)

use crate::commands::{Command, CommandArity, CommandResult, ResponseValue};
use crate::storage::MemoryStore;
use async_trait::async_trait;

/// INCR command - atomically increment a key's integer value by 1
pub struct IncrCommand;

#[async_trait]
impl Command for IncrCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if args.is_empty() {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'INCR' command".to_string(),
            );
        }

        let key = &args[0];

        match store.incr(key).await {
            Ok(new_value) => CommandResult::Ok(ResponseValue::Integer(new_value)),
            Err(err) => CommandResult::Error(err.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "INCR"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2) // INCR key
    }
}

/// DECR command - atomically decrement a key's integer value by 1
pub struct DecrCommand;

#[async_trait]
impl Command for DecrCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if args.is_empty() {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'DECR' command".to_string(),
            );
        }

        let key = &args[0];

        match store.decr(key).await {
            Ok(new_value) => CommandResult::Ok(ResponseValue::Integer(new_value)),
            Err(err) => CommandResult::Error(err.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "DECR"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2) // DECR key
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStore;
    use std::sync::Arc;
    use std::thread;
    use tokio;

    #[tokio::test]
    async fn test_incr_command_new_key() {
        let store = MemoryStore::new();
        let cmd = IncrCommand;

        let result = cmd.execute(&["test_key".to_string()], &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(value)) => {
                assert_eq!(value, 1);
            }
            _ => panic!("Expected integer response with value 1"),
        }

        // Verify the value was stored
        let stored = store.get("test_key").unwrap().unwrap();
        assert_eq!(stored.as_integer().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_incr_command_existing_integer() {
        let store = MemoryStore::new();
        let cmd = IncrCommand;

        // Set initial value
        store.set("test_key", 5i64).await.unwrap();

        let result = cmd.execute(&["test_key".to_string()], &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(value)) => {
                assert_eq!(value, 6);
            }
            _ => panic!("Expected integer response with value 6"),
        }
    }

    #[tokio::test]
    async fn test_incr_command_existing_string_number() {
        let store = MemoryStore::new();
        let cmd = IncrCommand;

        // Set initial value as string number
        store.set("test_key", "42").await.unwrap();

        let result = cmd.execute(&["test_key".to_string()], &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(value)) => {
                assert_eq!(value, 43);
            }
            _ => panic!("Expected integer response with value 43"),
        }
    }

    #[tokio::test]
    async fn test_incr_command_non_numeric_string() {
        let store = MemoryStore::new();
        let cmd = IncrCommand;

        // Set initial value as non-numeric string
        store.set("test_key", "not_a_number").await.unwrap();

        let result = cmd.execute(&["test_key".to_string()], &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("value is not an integer"));
            }
            _ => panic!("Expected error for non-numeric value"),
        }
    }

    #[tokio::test]
    async fn test_incr_command_no_args() {
        let store = MemoryStore::new();
        let cmd = IncrCommand;

        let result = cmd.execute(&[], &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("wrong number of arguments"));
            }
            _ => panic!("Expected error for missing arguments"),
        }
    }

    #[tokio::test]
    async fn test_decr_command_new_key() {
        let store = MemoryStore::new();
        let cmd = DecrCommand;

        let result = cmd.execute(&["test_key".to_string()], &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(value)) => {
                assert_eq!(value, -1);
            }
            _ => panic!("Expected integer response with value -1"),
        }

        // Verify the value was stored
        let stored = store.get("test_key").unwrap().unwrap();
        assert_eq!(stored.as_integer().unwrap(), -1);
    }

    #[tokio::test]
    async fn test_decr_command_existing_integer() {
        let store = MemoryStore::new();
        let cmd = DecrCommand;

        // Set initial value
        store.set("test_key", 10i64).await.unwrap();

        let result = cmd.execute(&["test_key".to_string()], &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(value)) => {
                assert_eq!(value, 9);
            }
            _ => panic!("Expected integer response with value 9"),
        }
    }

    #[tokio::test]
    async fn test_decr_command_existing_string_number() {
        let store = MemoryStore::new();
        let cmd = DecrCommand;

        // Set initial value as string number
        store.set("test_key", "100").await.unwrap();

        let result = cmd.execute(&["test_key".to_string()], &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(value)) => {
                assert_eq!(value, 99);
            }
            _ => panic!("Expected integer response with value 99"),
        }
    }

    #[tokio::test]
    async fn test_decr_command_non_numeric_string() {
        let store = MemoryStore::new();
        let cmd = DecrCommand;

        // Set initial value as non-numeric string
        store.set("test_key", "invalid").await.unwrap();

        let result = cmd.execute(&["test_key".to_string()], &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("value is not an integer"));
            }
            _ => panic!("Expected error for non-numeric value"),
        }
    }

    #[tokio::test]
    async fn test_decr_command_no_args() {
        let store = MemoryStore::new();
        let cmd = DecrCommand;

        let result = cmd.execute(&[], &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("wrong number of arguments"));
            }
            _ => panic!("Expected error for missing arguments"),
        }
    }

    #[tokio::test]
    async fn test_incr_decr_sequence() {
        let store = MemoryStore::new();
        let incr_cmd = IncrCommand;
        let decr_cmd = DecrCommand;

        // Start with INCR on new key
        let result1 = incr_cmd.execute(&["counter".to_string()], &store).await;
        assert!(matches!(
            result1,
            CommandResult::Ok(ResponseValue::Integer(1))
        ));

        // INCR again
        let result2 = incr_cmd.execute(&["counter".to_string()], &store).await;
        assert!(matches!(
            result2,
            CommandResult::Ok(ResponseValue::Integer(2))
        ));

        // DECR
        let result3 = decr_cmd.execute(&["counter".to_string()], &store).await;
        assert!(matches!(
            result3,
            CommandResult::Ok(ResponseValue::Integer(1))
        ));

        // DECR again
        let result4 = decr_cmd.execute(&["counter".to_string()], &store).await;
        assert!(matches!(
            result4,
            CommandResult::Ok(ResponseValue::Integer(0))
        ));

        // DECR to negative
        let result5 = decr_cmd.execute(&["counter".to_string()], &store).await;
        assert!(matches!(
            result5,
            CommandResult::Ok(ResponseValue::Integer(-1))
        ));
    }

    #[test]
    fn test_concurrent_incr_operations() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = Arc::new(MemoryStore::new());
            let mut handles = vec![];

            // Spawn 10 threads, each incrementing the same key 10 times
            for _ in 0..10 {
                let store_clone = Arc::clone(&store);
                let handle = thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async {
                        let cmd = IncrCommand;
                        for _ in 0..10 {
                            let _ = cmd
                                .execute(&["concurrent_key".to_string()], &store_clone)
                                .await;
                        }
                    });
                });
                handles.push(handle);
            }

            // Wait for all threads to complete
            for handle in handles {
                handle.join().unwrap();
            }

            // Final value should be 100 (10 threads * 10 increments)
            let final_value = store.get("concurrent_key").unwrap().unwrap();
            assert_eq!(final_value.as_integer().unwrap(), 100);
        });
    }

    #[test]
    fn test_concurrent_mixed_operations() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = Arc::new(MemoryStore::new());
            let mut handles = vec![];

            // Initialize counter to 50
            store.set("mixed_key", 50i64).await.unwrap();

            // Spawn threads doing mixed INCR/DECR operations
            for i in 0..20 {
                let store_clone = Arc::clone(&store);
                let handle = thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async {
                        if i % 2 == 0 {
                            // Even threads increment
                            let cmd = IncrCommand;
                            let _ = cmd.execute(&["mixed_key".to_string()], &store_clone).await;
                        } else {
                            // Odd threads decrement
                            let cmd = DecrCommand;
                            let _ = cmd.execute(&["mixed_key".to_string()], &store_clone).await;
                        }
                    });
                });
                handles.push(handle);
            }

            // Wait for all threads to complete
            for handle in handles {
                handle.join().unwrap();
            }

            // Final value should be 50 (10 increments - 10 decrements = 0 net change)
            let final_value = store.get("mixed_key").unwrap().unwrap();
            assert_eq!(final_value.as_integer().unwrap(), 50);
        });
    }

    #[tokio::test]
    async fn test_integer_overflow_protection() {
        let store = MemoryStore::new();
        let cmd = IncrCommand;

        // Set value near i64::MAX
        store.set("overflow_key", i64::MAX).await.unwrap();

        let result = cmd.execute(&["overflow_key".to_string()], &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("value is not an integer"));
            }
            _ => panic!("Expected error for integer overflow"),
        }
    }

    #[tokio::test]
    async fn test_integer_underflow_protection() {
        let store = MemoryStore::new();
        let cmd = DecrCommand;

        // Set value at i64::MIN
        store.set("underflow_key", i64::MIN).await.unwrap();

        let result = cmd.execute(&["underflow_key".to_string()], &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("value is not an integer"));
            }
            _ => panic!("Expected error for integer underflow"),
        }
    }

    #[tokio::test]
    async fn test_expired_key_handling() {
        let store = MemoryStore::new();
        let cmd = IncrCommand;

        // Set a key with very short TTL
        store.set_with_ttl("expired_key", 42i64, 0).await.unwrap(); // Expires immediately

        // Wait a moment to ensure expiration
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;

        // INCR should treat expired key as new key
        let result = cmd.execute(&["expired_key".to_string()], &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(value)) => {
                assert_eq!(value, 1); // Should initialize to 1, not increment 42
            }
            _ => panic!("Expected integer response with value 1"),
        }
    }

    #[tokio::test]
    async fn test_command_names_and_arity() {
        let incr_cmd = IncrCommand;
        let decr_cmd = DecrCommand;

        assert_eq!(incr_cmd.name(), "INCR");
        assert_eq!(decr_cmd.name(), "DECR");

        assert_eq!(incr_cmd.arity(), CommandArity::Fixed(2));
        assert_eq!(decr_cmd.arity(), CommandArity::Fixed(2));
    }
}
