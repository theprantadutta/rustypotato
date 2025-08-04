//! Comprehensive unit tests for command system
//!
//! Tests cover command registry, individual commands, parsing, and execution
//! with various input scenarios and error conditions.

use rustypotato::{
    CommandRegistry, CommandResult, ResponseValue, ParsedCommand,
    SetCommand, GetCommand, DelCommand, ExistsCommand,
    ExpireCommand, TtlCommand, IncrCommand, DecrCommand,
    MemoryStore, ValueType
};
use std::sync::Arc;

#[cfg(test)]
mod command_registry_tests {
    use super::*;

    #[test]
    fn test_command_registry_creation() {
        let registry = CommandRegistry::new();
        assert_eq!(registry.command_count(), 0);
        assert!(registry.command_names().is_empty());
    }

    #[test]
    fn test_command_registration() {
        let mut registry = CommandRegistry::new();
        
        registry.register(Box::new(SetCommand));
        assert_eq!(registry.command_count(), 1);
        assert!(registry.has_command("SET"));
        assert!(registry.command_names().contains(&"SET".to_string()));

        registry.register(Box::new(GetCommand));
        assert_eq!(registry.command_count(), 2);
        assert!(registry.has_command("GET"));
    }

    #[test]
    fn test_command_lookup() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(SetCommand));
        registry.register(Box::new(GetCommand));

        // Case insensitive lookup
        assert!(registry.has_command("SET"));
        assert!(registry.has_command("set"));
        assert!(registry.has_command("Set"));
        assert!(registry.has_command("GET"));
        assert!(registry.has_command("get"));

        // Non-existent command
        assert!(!registry.has_command("UNKNOWN"));
    }

    #[test]
    fn test_full_command_registry() {
        let mut registry = CommandRegistry::new();
        
        // Register all commands
        registry.register(Box::new(SetCommand));
        registry.register(Box::new(GetCommand));
        registry.register(Box::new(DelCommand));
        registry.register(Box::new(ExistsCommand));
        registry.register(Box::new(ExpireCommand));
        registry.register(Box::new(TtlCommand));
        registry.register(Box::new(IncrCommand));
        registry.register(Box::new(DecrCommand));

        assert_eq!(registry.command_count(), 8);
        
        let expected_commands = vec![
            "SET", "GET", "DEL", "EXISTS", "EXPIRE", "TTL", "INCR", "DECR"
        ];
        
        for cmd in expected_commands {
            assert!(registry.has_command(cmd));
        }
    }
}

#[cfg(test)]
mod parsed_command_tests {
    use super::*;

    #[test]
    fn test_parsed_command_creation() {
        let cmd = ParsedCommand::new("SET", vec!["key".to_string(), "value".to_string()]);
        assert_eq!(cmd.name(), "SET");
        assert_eq!(cmd.args(), &vec!["key".to_string(), "value".to_string()]);
        assert_eq!(cmd.arg_count(), 2);
    }

    #[test]
    fn test_parsed_command_arg_access() {
        let cmd = ParsedCommand::new("SET", vec!["key".to_string(), "value".to_string()]);
        
        assert_eq!(cmd.arg(0), Some("key"));
        assert_eq!(cmd.arg(1), Some("value"));
        assert_eq!(cmd.arg(2), None);
    }

    #[test]
    fn test_parsed_command_edge_cases() {
        // Empty args
        let cmd = ParsedCommand::new("PING", vec![]);
        assert_eq!(cmd.arg_count(), 0);
        assert_eq!(cmd.arg(0), None);

        // Single arg
        let cmd = ParsedCommand::new("GET", vec!["key".to_string()]);
        assert_eq!(cmd.arg_count(), 1);
        assert_eq!(cmd.arg(0), Some("key"));
        assert_eq!(cmd.arg(1), None);
    }
}

#[cfg(test)]
mod response_value_tests {
    use super::*;

    #[test]
    fn test_response_value_types() {
        // Simple string
        let simple = ResponseValue::SimpleString("OK".to_string());
        assert!(matches!(simple, ResponseValue::SimpleString(_)));

        // Bulk string
        let bulk = ResponseValue::BulkString(Some("value".to_string()));
        assert!(matches!(bulk, ResponseValue::BulkString(Some(_))));

        // Null bulk string
        let null_bulk = ResponseValue::BulkString(None);
        assert!(matches!(null_bulk, ResponseValue::BulkString(None)));

        // Integer
        let integer = ResponseValue::Integer(42);
        assert!(matches!(integer, ResponseValue::Integer(42)));

        // Array
        let array = ResponseValue::Array(vec![
            ResponseValue::SimpleString("OK".to_string()),
            ResponseValue::Integer(1),
        ]);
        assert!(matches!(array, ResponseValue::Array(_)));

        // Nil
        let nil = ResponseValue::Nil;
        assert!(matches!(nil, ResponseValue::Nil));
    }

    #[test]
    fn test_response_value_equality() {
        let simple1 = ResponseValue::SimpleString("OK".to_string());
        let simple2 = ResponseValue::SimpleString("OK".to_string());
        let simple3 = ResponseValue::SimpleString("ERROR".to_string());

        assert_eq!(simple1, simple2);
        assert_ne!(simple1, simple3);

        let int1 = ResponseValue::Integer(42);
        let int2 = ResponseValue::Integer(42);
        let int3 = ResponseValue::Integer(43);

        assert_eq!(int1, int2);
        assert_ne!(int1, int3);
    }
}

#[cfg(test)]
mod string_command_tests {
    use super::*;

    #[tokio::test]
    async fn test_set_command() {
        let store = Arc::new(MemoryStore::new());
        let command = SetCommand;

        // Valid SET command
        let args = vec!["key1".to_string(), "value1".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::SimpleString(_))));

        // Verify value was set
        let stored = store.get("key1").unwrap().unwrap();
        assert_eq!(stored.value.to_string(), "value1");

        // Wrong arity
        let args = vec!["key1".to_string()]; // Missing value
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));

        let args = vec!["key1".to_string(), "value1".to_string(), "extra".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn test_get_command() {
        let store = Arc::new(MemoryStore::new());
        let command = GetCommand;

        // Get non-existent key
        let args = vec!["nonexistent".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::BulkString(None))));

        // Set a value and get it
        store.set("key1", "value1").await.unwrap();
        let args = vec!["key1".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::BulkString(Some(_)))));

        // Wrong arity
        let args = vec![];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn test_del_command() {
        let store = Arc::new(MemoryStore::new());
        let command = DelCommand;

        // Delete non-existent key
        let args = vec!["nonexistent".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(0))));

        // Set a value and delete it
        store.set("key1", "value1").await.unwrap();
        let args = vec!["key1".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        // Verify key was deleted
        assert!(!store.exists("key1").unwrap());

        // Wrong arity
        let args = vec![];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn test_exists_command() {
        let store = Arc::new(MemoryStore::new());
        let command = ExistsCommand;

        // Check non-existent key
        let args = vec!["nonexistent".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(0))));

        // Set a value and check existence
        store.set("key1", "value1").await.unwrap();
        let args = vec!["key1".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        // Wrong arity
        let args = vec![];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }
}

#[cfg(test)]
mod ttl_command_tests {
    use super::*;
    use tokio::time::sleep;
    use std::time::Duration;

    #[tokio::test]
    async fn test_expire_command() {
        let store = Arc::new(MemoryStore::new());
        let command = ExpireCommand;

        // Expire non-existent key
        let args = vec!["nonexistent".to_string(), "60".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(0))));

        // Set a value and expire it
        store.set("key1", "value1").await.unwrap();
        let args = vec!["key1".to_string(), "60".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        // Verify TTL was set
        let ttl = store.ttl("key1").unwrap();
        assert!(ttl > 0 && ttl <= 60);

        // Invalid TTL value
        let args = vec!["key1".to_string(), "invalid".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));

        // Wrong arity
        let args = vec!["key1".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn test_ttl_command() {
        let store = Arc::new(MemoryStore::new());
        let command = TtlCommand;

        // TTL for non-existent key
        let args = vec!["nonexistent".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(-2))));

        // TTL for key without expiration
        store.set("key1", "value1").await.unwrap();
        let args = vec!["key1".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(-1))));

        // TTL for key with expiration
        store.expire("key1", 60).await.unwrap();
        let args = vec!["key1".to_string()];
        let result = command.execute(&args, &store).await;
        if let CommandResult::Ok(ResponseValue::Integer(ttl)) = result {
            assert!(ttl > 0 && ttl <= 60);
        } else {
            panic!("Expected integer TTL response");
        }

        // Wrong arity
        let args = vec![];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn test_expire_edge_cases() {
        let store = Arc::new(MemoryStore::new());
        let command = ExpireCommand;

        // Zero TTL (should expire immediately)
        store.set("key1", "value1").await.unwrap();
        let args = vec!["key1".to_string(), "0".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        // Key should be expired almost immediately
        sleep(Duration::from_millis(10)).await;
        assert!(!store.exists("key1").unwrap());

        // Negative TTL (should be rejected)
        store.set("key2", "value2").await.unwrap();
        let args = vec!["key2".to_string(), "-1".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }
}

#[cfg(test)]
mod atomic_command_tests {
    use super::*;

    #[tokio::test]
    async fn test_incr_command() {
        let store = Arc::new(MemoryStore::new());
        let command = IncrCommand;

        // INCR on non-existent key
        let args = vec!["counter".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        // INCR on existing integer
        let args = vec!["counter".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(2))));

        // INCR on string value (should fail)
        store.set("string_key", "not_a_number").await.unwrap();
        let args = vec!["string_key".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));

        // Wrong arity
        let args = vec![];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn test_decr_command() {
        let store = Arc::new(MemoryStore::new());
        let command = DecrCommand;

        // DECR on non-existent key
        let args = vec!["counter".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(-1))));

        // DECR on existing integer
        let args = vec!["counter".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(-2))));

        // DECR on string value (should fail)
        store.set("string_key", "not_a_number").await.unwrap();
        let args = vec!["string_key".to_string()];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));

        // Wrong arity
        let args = vec![];
        let result = command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn test_atomic_operations_boundary_values() {
        let store = Arc::new(MemoryStore::new());
        let incr_command = IncrCommand;
        let decr_command = DecrCommand;

        // Test near max value
        store.set("max_test", i64::MAX - 1).await.unwrap();
        let args = vec!["max_test".to_string()];
        let result = incr_command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(_))));

        // Test overflow
        let args = vec!["max_test".to_string()];
        let result = incr_command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));

        // Test near min value
        store.set("min_test", i64::MIN + 1).await.unwrap();
        let args = vec!["min_test".to_string()];
        let result = decr_command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(_))));

        // Test underflow
        let args = vec!["min_test".to_string()];
        let result = decr_command.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }
}

#[cfg(test)]
mod command_integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_command_workflow() {
        let store = Arc::new(MemoryStore::new());
        let mut registry = CommandRegistry::new();
        
        // Register all commands
        registry.register(Box::new(SetCommand));
        registry.register(Box::new(GetCommand));
        registry.register(Box::new(DelCommand));
        registry.register(Box::new(ExistsCommand));
        registry.register(Box::new(ExpireCommand));
        registry.register(Box::new(TtlCommand));
        registry.register(Box::new(IncrCommand));
        registry.register(Box::new(DecrCommand));

        // Test complete workflow
        
        // 1. SET a value
        let set_cmd = ParsedCommand::new("SET", vec!["test_key".to_string(), "test_value".to_string()]);
        let result = registry.execute(&set_cmd, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::SimpleString(_))));

        // 2. GET the value
        let get_cmd = ParsedCommand::new("GET", vec!["test_key".to_string()]);
        let result = registry.execute(&get_cmd, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::BulkString(Some(_)))));

        // 3. Check EXISTS
        let exists_cmd = ParsedCommand::new("EXISTS", vec!["test_key".to_string()]);
        let result = registry.execute(&exists_cmd, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        // 4. Set EXPIRE
        let expire_cmd = ParsedCommand::new("EXPIRE", vec!["test_key".to_string(), "60".to_string()]);
        let result = registry.execute(&expire_cmd, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        // 5. Check TTL
        let ttl_cmd = ParsedCommand::new("TTL", vec!["test_key".to_string()]);
        let result = registry.execute(&ttl_cmd, &store).await;
        if let CommandResult::Ok(ResponseValue::Integer(ttl)) = result {
            assert!(ttl > 0 && ttl <= 60);
        } else {
            panic!("Expected integer TTL response");
        }

        // 6. DELETE the key
        let del_cmd = ParsedCommand::new("DEL", vec!["test_key".to_string()]);
        let result = registry.execute(&del_cmd, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        // 7. Verify key is gone
        let exists_cmd = ParsedCommand::new("EXISTS", vec!["test_key".to_string()]);
        let result = registry.execute(&exists_cmd, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(0))));
    }

    #[tokio::test]
    async fn test_atomic_operations_workflow() {
        let store = Arc::new(MemoryStore::new());
        let mut registry = CommandRegistry::new();
        
        registry.register(Box::new(IncrCommand));
        registry.register(Box::new(DecrCommand));
        registry.register(Box::new(GetCommand));

        // Test atomic operations workflow
        
        // 1. INCR non-existent key
        let incr_cmd = ParsedCommand::new("INCR", vec!["counter".to_string()]);
        let result = registry.execute(&incr_cmd, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        // 2. INCR again
        let incr_cmd = ParsedCommand::new("INCR", vec!["counter".to_string()]);
        let result = registry.execute(&incr_cmd, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(2))));

        // 3. DECR
        let decr_cmd = ParsedCommand::new("DECR", vec!["counter".to_string()]);
        let result = registry.execute(&decr_cmd, &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        // 4. GET to verify final value
        let get_cmd = ParsedCommand::new("GET", vec!["counter".to_string()]);
        let result = registry.execute(&get_cmd, &store).await;
        if let CommandResult::Ok(ResponseValue::BulkString(Some(value))) = result {
            assert_eq!(value, "1");
        } else {
            panic!("Expected bulk string response with value '1'");
        }
    }

    #[tokio::test]
    async fn test_error_handling_workflow() {
        let store = Arc::new(MemoryStore::new());
        let mut registry = CommandRegistry::new();
        
        registry.register(Box::new(SetCommand));
        registry.register(Box::new(IncrCommand));

        // Test error handling
        
        // 1. SET a string value
        let set_cmd = ParsedCommand::new("SET", vec!["string_key".to_string(), "not_a_number".to_string()]);
        let result = registry.execute(&set_cmd, &store).await;
        assert!(matches!(result, CommandResult::Ok(_)));

        // 2. Try to INCR the string value (should fail)
        let incr_cmd = ParsedCommand::new("INCR", vec!["string_key".to_string()]);
        let result = registry.execute(&incr_cmd, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));

        // 3. Test unknown command
        let unknown_cmd = ParsedCommand::new("UNKNOWN", vec![]);
        let result = registry.execute(&unknown_cmd, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn test_concurrent_command_execution() {
        let store = Arc::new(MemoryStore::new());
        let registry = Arc::new({
            let mut reg = CommandRegistry::new();
            reg.register(Box::new(SetCommand));
            reg.register(Box::new(GetCommand));
            reg.register(Box::new(IncrCommand));
            reg
        });

        let mut handles = Vec::new();

        // Spawn multiple tasks executing commands concurrently
        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            let registry_clone = Arc::clone(&registry);
            
            let handle = tokio::spawn(async move {
                // Each task performs a sequence of operations
                let key = format!("concurrent_key_{}", i);
                
                // SET operation
                let set_cmd = ParsedCommand::new("SET", vec![key.clone(), format!("value_{}", i)]);
                let result = registry_clone.execute(&set_cmd, &store_clone).await;
                assert!(matches!(result, CommandResult::Ok(_)));

                // GET operation
                let get_cmd = ParsedCommand::new("GET", vec![key.clone()]);
                let result = registry_clone.execute(&get_cmd, &store_clone).await;
                assert!(matches!(result, CommandResult::Ok(ResponseValue::BulkString(Some(_)))));

                // INCR operation on shared counter
                let incr_cmd = ParsedCommand::new("INCR", vec!["shared_counter".to_string()]);
                let result = registry_clone.execute(&incr_cmd, &store_clone).await;
                assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(_))));

                i
            });
            
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify final state
        let counter_value = store.get("shared_counter").unwrap().unwrap();
        assert_eq!(counter_value.value.to_integer().unwrap(), 10);
    }
}