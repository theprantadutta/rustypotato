//! Hash command implementations (HSET, HGET, HDEL, HGETALL, HEXISTS)

use crate::commands::{Command, CommandArity, CommandResult, ResponseValue};
use crate::storage::MemoryStore;
use async_trait::async_trait;

/// HSET command implementation
/// Sets field in the hash stored at key to value
pub struct HsetCommand;

#[async_trait]
impl Command for HsetCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if args.len() != 3 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'HSET' command".to_string(),
            );
        }

        let key = &args[0];
        let field = &args[1];
        let value = &args[2];

        match store.hset(key, field, value).await {
            Ok(is_new_field) => {
                // Return 1 if new field, 0 if field was updated
                CommandResult::Ok(ResponseValue::Integer(if is_new_field { 1 } else { 0 }))
            }
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "HSET"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(4) // HSET key field value
    }
}

/// HGET command implementation
/// Gets the value of a hash field
pub struct HgetCommand;

#[async_trait]
impl Command for HgetCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if args.len() != 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'HGET' command".to_string(),
            );
        }

        let key = &args[0];
        let field = &args[1];

        match store.hget(key, field).await {
            Ok(Some(value)) => CommandResult::Ok(ResponseValue::BulkString(Some(value))),
            Ok(None) => CommandResult::Ok(ResponseValue::BulkString(None)), // Field doesn't exist
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "HGET"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(3) // HGET key field
    }
}

/// HDEL command implementation
/// Removes the specified fields from the hash stored at key
pub struct HdelCommand;

#[async_trait]
impl Command for HdelCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if args.len() < 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'HDEL' command".to_string(),
            );
        }

        let key = &args[0];
        let fields = &args[1..];
        let mut deleted_count = 0i64;

        for field in fields {
            match store.hdel(key, field).await {
                Ok(true) => deleted_count += 1,
                Ok(false) => {} // Field didn't exist, continue
                Err(e) => return CommandResult::Error(e.to_client_error()),
            }
        }

        CommandResult::Ok(ResponseValue::Integer(deleted_count))
    }

    fn name(&self) -> &'static str {
        "HDEL"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::AtLeast(3) // HDEL key field [field ...]
    }
}

/// HGETALL command implementation
/// Gets all fields and values in a hash
pub struct HgetallCommand;

#[async_trait]
impl Command for HgetallCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'HGETALL' command".to_string(),
            );
        }

        let key = &args[0];

        match store.hgetall(key).await {
            Ok(fields) => {
                // Convert to array of alternating field-value pairs
                let mut result = Vec::new();
                for (field, value) in fields {
                    result.push(ResponseValue::BulkString(Some(field)));
                    result.push(ResponseValue::BulkString(Some(value)));
                }
                CommandResult::Ok(ResponseValue::Array(result))
            }
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "HGETALL"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2) // HGETALL key
    }
}

/// HEXISTS command implementation
/// Determines if a hash field exists
pub struct HexistsCommand;

#[async_trait]
impl Command for HexistsCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if args.len() != 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'HEXISTS' command".to_string(),
            );
        }

        let key = &args[0];
        let field = &args[1];

        match store.hexists(key, field).await {
            Ok(exists) => CommandResult::Ok(ResponseValue::Integer(if exists { 1 } else { 0 })),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "HEXISTS"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(3) // HEXISTS key field
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStore;
    use std::collections::HashMap;

    // Helper function to create a test store
    fn create_test_store() -> MemoryStore {
        MemoryStore::new()
    }

    // HSET command tests
    #[tokio::test]
    async fn test_hset_command_new_field() {
        let store = create_test_store();
        let cmd = HsetCommand;
        let args = vec!["myhash".to_string(), "field1".to_string(), "value1".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 1); // New field created
            }
            _ => panic!("Expected integer response from HSET command"),
        }

        // Verify the field was actually stored
        let value = store.hget("myhash", "field1").await.unwrap();
        assert_eq!(value, Some("value1".to_string()));
    }

    #[tokio::test]
    async fn test_hset_command_update_field() {
        let store = create_test_store();
        let cmd = HsetCommand;

        // Set initial value
        let args1 = vec!["myhash".to_string(), "field1".to_string(), "value1".to_string()];
        let result1 = cmd.execute(&args1, &store).await;
        assert!(matches!(result1, CommandResult::Ok(ResponseValue::Integer(1))));

        // Update the same field
        let args2 = vec!["myhash".to_string(), "field1".to_string(), "value2".to_string()];
        let result2 = cmd.execute(&args2, &store).await;

        match result2 {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 0); // Field was updated, not created
            }
            _ => panic!("Expected integer response from HSET command"),
        }

        // Verify the field was updated
        let value = store.hget("myhash", "field1").await.unwrap();
        assert_eq!(value, Some("value2".to_string()));
    }

    #[tokio::test]
    async fn test_hset_command_wrong_args() {
        let store = create_test_store();
        let cmd = HsetCommand;

        // Too few arguments
        let args = vec!["myhash".to_string(), "field1".to_string()];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));

        // Too many arguments
        let args = vec!["myhash".to_string(), "field1".to_string(), "value1".to_string(), "extra".to_string()];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[test]
    fn test_hset_command_properties() {
        let cmd = HsetCommand;
        assert_eq!(cmd.name(), "HSET");
        assert_eq!(cmd.arity(), CommandArity::Fixed(4));
    }

    // HGET command tests
    #[tokio::test]
    async fn test_hget_command_existing_field() {
        let store = create_test_store();
        
        // Set up test data
        store.hset("myhash", "field1", "value1").await.unwrap();

        let cmd = HgetCommand;
        let args = vec!["myhash".to_string(), "field1".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(Some(value))) => {
                assert_eq!(value, "value1");
            }
            _ => panic!("Expected bulk string response from HGET command"),
        }
    }

    #[tokio::test]
    async fn test_hget_command_nonexistent_field() {
        let store = create_test_store();
        
        // Set up test data with different field
        store.hset("myhash", "field1", "value1").await.unwrap();

        let cmd = HgetCommand;
        let args = vec!["myhash".to_string(), "field2".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(None)) => {
                // Expected: field doesn't exist
            }
            _ => panic!("Expected nil response for nonexistent field"),
        }
    }

    #[tokio::test]
    async fn test_hget_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = HgetCommand;
        let args = vec!["nonexistent".to_string(), "field1".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(None)) => {
                // Expected: key doesn't exist
            }
            _ => panic!("Expected nil response for nonexistent key"),
        }
    }

    #[tokio::test]
    async fn test_hget_command_wrong_args() {
        let store = create_test_store();
        let cmd = HgetCommand;

        // Too few arguments
        let args = vec!["myhash".to_string()];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));

        // Too many arguments
        let args = vec!["myhash".to_string(), "field1".to_string(), "extra".to_string()];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[test]
    fn test_hget_command_properties() {
        let cmd = HgetCommand;
        assert_eq!(cmd.name(), "HGET");
        assert_eq!(cmd.arity(), CommandArity::Fixed(3));
    }

    // HDEL command tests
    #[tokio::test]
    async fn test_hdel_command_single_field() {
        let store = create_test_store();
        
        // Set up test data
        store.hset("myhash", "field1", "value1").await.unwrap();
        store.hset("myhash", "field2", "value2").await.unwrap();

        let cmd = HdelCommand;
        let args = vec!["myhash".to_string(), "field1".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 1);
            }
            _ => panic!("Expected integer response from HDEL command"),
        }

        // Verify field was deleted
        let value = store.hget("myhash", "field1").await.unwrap();
        assert_eq!(value, None);

        // Verify other field still exists
        let value = store.hget("myhash", "field2").await.unwrap();
        assert_eq!(value, Some("value2".to_string()));
    }

    #[tokio::test]
    async fn test_hdel_command_multiple_fields() {
        let store = create_test_store();
        
        // Set up test data
        store.hset("myhash", "field1", "value1").await.unwrap();
        store.hset("myhash", "field2", "value2").await.unwrap();
        store.hset("myhash", "field3", "value3").await.unwrap();

        let cmd = HdelCommand;
        let args = vec!["myhash".to_string(), "field1".to_string(), "field2".to_string(), "nonexistent".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 2); // field1 and field2 deleted, nonexistent didn't exist
            }
            _ => panic!("Expected integer response from HDEL command"),
        }

        // Verify fields were deleted
        assert_eq!(store.hget("myhash", "field1").await.unwrap(), None);
        assert_eq!(store.hget("myhash", "field2").await.unwrap(), None);
        
        // Verify field3 still exists
        assert_eq!(store.hget("myhash", "field3").await.unwrap(), Some("value3".to_string()));
    }

    #[tokio::test]
    async fn test_hdel_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = HdelCommand;
        let args = vec!["nonexistent".to_string(), "field1".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 0);
            }
            _ => panic!("Expected integer response from HDEL command"),
        }
    }

    #[tokio::test]
    async fn test_hdel_command_wrong_args() {
        let store = create_test_store();
        let cmd = HdelCommand;

        // Too few arguments
        let args = vec!["myhash".to_string()];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[test]
    fn test_hdel_command_properties() {
        let cmd = HdelCommand;
        assert_eq!(cmd.name(), "HDEL");
        assert_eq!(cmd.arity(), CommandArity::AtLeast(3));
    }

    // HGETALL command tests
    #[tokio::test]
    async fn test_hgetall_command_with_fields() {
        let store = create_test_store();
        
        // Set up test data
        store.hset("myhash", "field1", "value1").await.unwrap();
        store.hset("myhash", "field2", "value2").await.unwrap();

        let cmd = HgetallCommand;
        let args = vec!["myhash".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(values)) => {
                assert_eq!(values.len(), 4); // 2 fields * 2 (field + value)
                
                // Convert to HashMap for easier testing (order might vary)
                let mut fields = HashMap::new();
                for chunk in values.chunks(2) {
                    if let (ResponseValue::BulkString(Some(field)), ResponseValue::BulkString(Some(value))) = (&chunk[0], &chunk[1]) {
                        fields.insert(field.clone(), value.clone());
                    }
                }
                
                assert_eq!(fields.get("field1"), Some(&"value1".to_string()));
                assert_eq!(fields.get("field2"), Some(&"value2".to_string()));
            }
            _ => panic!("Expected array response from HGETALL command"),
        }
    }

    #[tokio::test]
    async fn test_hgetall_command_empty_hash() {
        let store = create_test_store();
        
        // Create empty hash
        store.hset("myhash", "field1", "value1").await.unwrap();
        store.hdel("myhash", "field1").await.unwrap();

        let cmd = HgetallCommand;
        let args = vec!["myhash".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(values)) => {
                assert_eq!(values.len(), 0);
            }
            _ => panic!("Expected empty array response from HGETALL command"),
        }
    }

    #[tokio::test]
    async fn test_hgetall_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = HgetallCommand;
        let args = vec!["nonexistent".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(values)) => {
                assert_eq!(values.len(), 0);
            }
            _ => panic!("Expected empty array response for nonexistent key"),
        }
    }

    #[tokio::test]
    async fn test_hgetall_command_wrong_args() {
        let store = create_test_store();
        let cmd = HgetallCommand;

        // Too few arguments
        let args = vec![];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));

        // Too many arguments
        let args = vec!["myhash".to_string(), "extra".to_string()];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[test]
    fn test_hgetall_command_properties() {
        let cmd = HgetallCommand;
        assert_eq!(cmd.name(), "HGETALL");
        assert_eq!(cmd.arity(), CommandArity::Fixed(2));
    }

    // HEXISTS command tests
    #[tokio::test]
    async fn test_hexists_command_existing_field() {
        let store = create_test_store();
        
        // Set up test data
        store.hset("myhash", "field1", "value1").await.unwrap();

        let cmd = HexistsCommand;
        let args = vec!["myhash".to_string(), "field1".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(exists)) => {
                assert_eq!(exists, 1);
            }
            _ => panic!("Expected integer response from HEXISTS command"),
        }
    }

    #[tokio::test]
    async fn test_hexists_command_nonexistent_field() {
        let store = create_test_store();
        
        // Set up test data with different field
        store.hset("myhash", "field1", "value1").await.unwrap();

        let cmd = HexistsCommand;
        let args = vec!["myhash".to_string(), "field2".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(exists)) => {
                assert_eq!(exists, 0);
            }
            _ => panic!("Expected integer response from HEXISTS command"),
        }
    }

    #[tokio::test]
    async fn test_hexists_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = HexistsCommand;
        let args = vec!["nonexistent".to_string(), "field1".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(exists)) => {
                assert_eq!(exists, 0);
            }
            _ => panic!("Expected integer response from HEXISTS command"),
        }
    }

    #[tokio::test]
    async fn test_hexists_command_wrong_args() {
        let store = create_test_store();
        let cmd = HexistsCommand;

        // Too few arguments
        let args = vec!["myhash".to_string()];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));

        // Too many arguments
        let args = vec!["myhash".to_string(), "field1".to_string(), "extra".to_string()];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[test]
    fn test_hexists_command_properties() {
        let cmd = HexistsCommand;
        assert_eq!(cmd.name(), "HEXISTS");
        assert_eq!(cmd.arity(), CommandArity::Fixed(3));
    }

    // Integration tests
    #[tokio::test]
    async fn test_hash_operations_integration() {
        let store = create_test_store();

        // Test complete workflow
        let hset_cmd = HsetCommand;
        let hget_cmd = HgetCommand;
        let hexists_cmd = HexistsCommand;
        let hgetall_cmd = HgetallCommand;
        let hdel_cmd = HdelCommand;

        // Set multiple fields
        let result = hset_cmd.execute(&["myhash".to_string(), "name".to_string(), "John".to_string()], &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        let result = hset_cmd.execute(&["myhash".to_string(), "age".to_string(), "30".to_string()], &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        // Get individual fields
        let result = hget_cmd.execute(&["myhash".to_string(), "name".to_string()], &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::BulkString(Some(ref v))) if v == "John"));

        // Check field existence
        let result = hexists_cmd.execute(&["myhash".to_string(), "name".to_string()], &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        let result = hexists_cmd.execute(&["myhash".to_string(), "nonexistent".to_string()], &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(0))));

        // Get all fields
        let result = hgetall_cmd.execute(&["myhash".to_string()], &store).await;
        if let CommandResult::Ok(ResponseValue::Array(values)) = result {
            assert_eq!(values.len(), 4); // 2 fields * 2
        } else {
            panic!("Expected array response from HGETALL");
        }

        // Delete a field
        let result = hdel_cmd.execute(&["myhash".to_string(), "age".to_string()], &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        // Verify field was deleted
        let result = hget_cmd.execute(&["myhash".to_string(), "age".to_string()], &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::BulkString(None))));

        // Verify other field still exists
        let result = hget_cmd.execute(&["myhash".to_string(), "name".to_string()], &store).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::BulkString(Some(ref v))) if v == "John"));
    }

    #[tokio::test]
    async fn test_hash_operations_with_type_mismatch() {
        let store = create_test_store();

        // Set a string value first
        store.set("mykey", "string_value").await.unwrap();

        // Try to use hash operations on string key
        let hset_cmd = HsetCommand;
        let result = hset_cmd.execute(&["mykey".to_string(), "field1".to_string(), "value1".to_string()], &store).await;
        assert!(matches!(result, CommandResult::Error(_)));

        let hget_cmd = HgetCommand;
        let result = hget_cmd.execute(&["mykey".to_string(), "field1".to_string()], &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }
}