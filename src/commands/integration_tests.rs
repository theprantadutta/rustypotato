//! Integration tests for command execution through the registry

#[cfg(test)]
mod tests {
    use crate::commands::{CommandRegistry, SetCommand, GetCommand, DelCommand, ExistsCommand, ParsedCommand};
    use crate::storage::MemoryStore;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_basic_commands_integration() {
        // Set up the command registry with all basic commands
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(SetCommand));
        registry.register(Box::new(GetCommand));
        registry.register(Box::new(DelCommand));
        registry.register(Box::new(ExistsCommand));
        
        let store = MemoryStore::new();
        let client_id = Uuid::new_v4();
        
        // Test SET command
        let set_cmd = ParsedCommand::parse("SET mykey myvalue", client_id).unwrap();
        let result = registry.execute(&set_cmd, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::SimpleString(ref s)) if s == "OK"));
        
        // Test GET command
        let get_cmd = ParsedCommand::parse("GET mykey", client_id).unwrap();
        let result = registry.execute(&get_cmd, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::BulkString(Some(ref s))) if s == "myvalue"));
        
        // Test EXISTS command
        let exists_cmd = ParsedCommand::parse("EXISTS mykey", client_id).unwrap();
        let result = registry.execute(&exists_cmd, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::Integer(1))));
        
        // Test DEL command
        let del_cmd = ParsedCommand::parse("DEL mykey", client_id).unwrap();
        let result = registry.execute(&del_cmd, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::Integer(1))));
        
        // Verify key is deleted
        let get_cmd2 = ParsedCommand::parse("GET mykey", client_id).unwrap();
        let result = registry.execute(&get_cmd2, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::BulkString(None))));
        
        let exists_cmd2 = ParsedCommand::parse("EXISTS mykey", client_id).unwrap();
        let result = registry.execute(&exists_cmd2, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::Integer(0))));
    }
    
    #[tokio::test]
    async fn test_multiple_keys_workflow() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(SetCommand));
        registry.register(Box::new(GetCommand));
        registry.register(Box::new(DelCommand));
        registry.register(Box::new(ExistsCommand));
        
        let store = MemoryStore::new();
        let client_id = Uuid::new_v4();
        
        // Set multiple keys
        for i in 1..=5 {
            let cmd = ParsedCommand::parse(&format!("SET key{} value{}", i, i), client_id).unwrap();
            let result = registry.execute(&cmd, &store).await;
            assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::SimpleString(ref s)) if s == "OK"));
        }
        
        // Check all keys exist
        let exists_cmd = ParsedCommand::parse("EXISTS key1 key2 key3 key4 key5", client_id).unwrap();
        let result = registry.execute(&exists_cmd, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::Integer(5))));
        
        // Delete some keys
        let del_cmd = ParsedCommand::parse("DEL key1 key3 key5", client_id).unwrap();
        let result = registry.execute(&del_cmd, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::Integer(3))));
        
        // Check remaining keys
        let exists_cmd2 = ParsedCommand::parse("EXISTS key1 key2 key3 key4 key5", client_id).unwrap();
        let result = registry.execute(&exists_cmd2, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::Integer(2)))); // key2 and key4
        
        // Verify specific keys
        let get_key2 = ParsedCommand::parse("GET key2", client_id).unwrap();
        let result = registry.execute(&get_key2, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::BulkString(Some(ref s))) if s == "value2"));
        
        let get_key4 = ParsedCommand::parse("GET key4", client_id).unwrap();
        let result = registry.execute(&get_key4, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::BulkString(Some(ref s))) if s == "value4"));
    }
    
    #[tokio::test]
    async fn test_case_insensitive_commands() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(SetCommand));
        registry.register(Box::new(GetCommand));
        
        let store = MemoryStore::new();
        let client_id = Uuid::new_v4();
        
        // Test lowercase commands
        let set_cmd = ParsedCommand::parse("set testkey testvalue", client_id).unwrap();
        let result = registry.execute(&set_cmd, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::SimpleString(ref s)) if s == "OK"));
        
        // Test uppercase commands
        let get_cmd = ParsedCommand::parse("GET testkey", client_id).unwrap();
        let result = registry.execute(&get_cmd, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::BulkString(Some(ref s))) if s == "testvalue"));
        
        // Test mixed case commands
        let get_cmd2 = ParsedCommand::parse("GeT testkey", client_id).unwrap();
        let result = registry.execute(&get_cmd2, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::BulkString(Some(ref s))) if s == "testvalue"));
    }
    
    #[tokio::test]
    async fn test_error_handling_integration() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(SetCommand));
        registry.register(Box::new(GetCommand));
        
        let store = MemoryStore::new();
        let client_id = Uuid::new_v4();
        
        // Test unknown command
        let unknown_cmd = ParsedCommand::parse("UNKNOWN key", client_id).unwrap();
        let result = registry.execute(&unknown_cmd, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Error(ref msg) if msg.contains("unknown command")));
        
        // Test wrong arity
        let wrong_arity_cmd = ParsedCommand::parse("SET onlykey", client_id).unwrap();
        let result = registry.execute(&wrong_arity_cmd, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Error(ref msg) if msg.contains("wrong number of arguments")));
        
        // Test GET on non-existent key (should return nil, not error)
        let get_missing_cmd = ParsedCommand::parse("GET nonexistent", client_id).unwrap();
        let result = registry.execute(&get_missing_cmd, &store).await;
        assert!(matches!(result, crate::commands::CommandResult::Ok(crate::commands::ResponseValue::BulkString(None))));
    }
}