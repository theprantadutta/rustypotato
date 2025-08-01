//! Command registry and execution framework

use crate::storage::MemoryStore;
use async_trait::async_trait;
use std::collections::HashMap;
use uuid::Uuid;

/// Trait for command implementations
#[async_trait]
pub trait Command: Send + Sync {
    /// Execute the command with given arguments
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult;
    
    /// Get the command name
    fn name(&self) -> &'static str;
    
    /// Get the command arity specification
    fn arity(&self) -> super::CommandArity;
}

/// Command execution result
#[derive(Debug, Clone, PartialEq)]
pub enum CommandResult {
    Ok(super::ResponseValue),
    Error(String),
}

/// Parsed command representation
#[derive(Debug, Clone)]
pub struct ParsedCommand {
    pub name: String,
    pub args: Vec<String>,
    pub client_id: Uuid,
}

impl ParsedCommand {
    /// Create a new parsed command
    pub fn new(name: String, args: Vec<String>, client_id: Uuid) -> Self {
        Self { name, args, client_id }
    }
    
    /// Parse a command string into a ParsedCommand
    /// Supports Redis-style command parsing: "SET key value"
    pub fn parse(input: &str, client_id: Uuid) -> Result<Self, String> {
        let parts: Vec<&str> = input.trim().split_whitespace().collect();
        
        if parts.is_empty() {
            return Err("Empty command".to_string());
        }
        
        let name = parts[0].to_string();
        let args = parts[1..].iter().map(|s| s.to_string()).collect();
        
        Ok(Self::new(name, args, client_id))
    }
    
    /// Get the total number of arguments (including command name)
    pub fn total_args(&self) -> usize {
        self.args.len() + 1
    }
    
    /// Check if the command has the expected number of arguments
    pub fn validate_arity(&self, arity: &super::CommandArity) -> Result<(), String> {
        let total = self.total_args();
        
        match arity {
            super::CommandArity::Fixed(expected) => {
                if total != *expected {
                    return Err(format!(
                        "wrong number of arguments for '{}' command: expected {}, got {}",
                        self.name, expected, total
                    ));
                }
            }
            super::CommandArity::Range(min, max) => {
                if total < *min || total > *max {
                    return Err(format!(
                        "wrong number of arguments for '{}' command: expected {}-{}, got {}",
                        self.name, min, max, total
                    ));
                }
            }
            super::CommandArity::AtLeast(min) => {
                if total < *min {
                    return Err(format!(
                        "wrong number of arguments for '{}' command: expected at least {}, got {}",
                        self.name, min, total
                    ));
                }
            }
        }
        
        Ok(())
    }
}

/// Command registry for lookup and dispatch
pub struct CommandRegistry {
    commands: HashMap<String, Box<dyn Command>>,
}

impl CommandRegistry {
    /// Create a new command registry
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }
    
    /// Register a command
    pub fn register(&mut self, command: Box<dyn Command>) {
        let name = command.name().to_uppercase();
        self.commands.insert(name, command);
    }
    
    /// Execute a parsed command with validation
    pub async fn execute(&self, cmd: &ParsedCommand, store: &MemoryStore) -> CommandResult {
        let command_name = cmd.name.to_uppercase();
        
        match self.commands.get(&command_name) {
            Some(command) => {
                // Validate command arity before execution
                if let Err(err) = cmd.validate_arity(&command.arity()) {
                    return CommandResult::Error(format!("ERR {}", err));
                }
                
                // Execute the command
                command.execute(&cmd.args, store).await
            }
            None => CommandResult::Error(format!("ERR unknown command '{}'", cmd.name)),
        }
    }
    
    /// Check if a command exists
    pub fn has_command(&self, name: &str) -> bool {
        self.commands.contains_key(&name.to_uppercase())
    }
    
    /// Get the list of registered command names
    pub fn command_names(&self) -> Vec<String> {
        self.commands.keys().cloned().collect()
    }
    
    /// Get the number of registered commands
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }
    
    /// Get command information for a specific command
    pub fn get_command_info(&self, name: &str) -> Option<(&str, super::CommandArity)> {
        self.commands.get(&name.to_uppercase())
            .map(|cmd| (cmd.name(), cmd.arity()))
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::CommandArity;
    use crate::storage::MemoryStore;
    use uuid::Uuid;

    // Mock command for testing
    struct MockCommand {
        name: &'static str,
        arity: CommandArity,
        should_error: bool,
    }

    #[async_trait]
    impl Command for MockCommand {
        async fn execute(&self, args: &[String], _store: &MemoryStore) -> CommandResult {
            if self.should_error {
                CommandResult::Error("Mock error".to_string())
            } else {
                CommandResult::Ok(crate::commands::ResponseValue::SimpleString(
                    format!("Mock response with {} args", args.len())
                ))
            }
        }

        fn name(&self) -> &'static str {
            self.name
        }

        fn arity(&self) -> CommandArity {
            self.arity.clone()
        }
    }

    #[test]
    fn test_parsed_command_creation() {
        let client_id = Uuid::new_v4();
        let cmd = ParsedCommand::new(
            "SET".to_string(),
            vec!["key".to_string(), "value".to_string()],
            client_id,
        );

        assert_eq!(cmd.name, "SET");
        assert_eq!(cmd.args, vec!["key", "value"]);
        assert_eq!(cmd.client_id, client_id);
        assert_eq!(cmd.total_args(), 3); // SET + key + value
    }

    #[test]
    fn test_parsed_command_parse_valid() {
        let client_id = Uuid::new_v4();
        let result = ParsedCommand::parse("SET key value", client_id);

        assert!(result.is_ok());
        let cmd = result.unwrap();
        assert_eq!(cmd.name, "SET");
        assert_eq!(cmd.args, vec!["key", "value"]);
        assert_eq!(cmd.client_id, client_id);
    }

    #[test]
    fn test_parsed_command_parse_empty() {
        let client_id = Uuid::new_v4();
        let result = ParsedCommand::parse("", client_id);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Empty command");
    }

    #[test]
    fn test_parsed_command_parse_whitespace() {
        let client_id = Uuid::new_v4();
        let result = ParsedCommand::parse("   ", client_id);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Empty command");
    }

    #[test]
    fn test_parsed_command_parse_single_command() {
        let client_id = Uuid::new_v4();
        let result = ParsedCommand::parse("PING", client_id);

        assert!(result.is_ok());
        let cmd = result.unwrap();
        assert_eq!(cmd.name, "PING");
        assert_eq!(cmd.args.len(), 0);
        assert_eq!(cmd.total_args(), 1);
    }

    #[test]
    fn test_parsed_command_parse_multiple_spaces() {
        let client_id = Uuid::new_v4();
        let result = ParsedCommand::parse("SET    key    value", client_id);

        assert!(result.is_ok());
        let cmd = result.unwrap();
        assert_eq!(cmd.name, "SET");
        assert_eq!(cmd.args, vec!["key", "value"]);
    }

    #[test]
    fn test_validate_arity_fixed_valid() {
        let client_id = Uuid::new_v4();
        let cmd = ParsedCommand::new(
            "SET".to_string(),
            vec!["key".to_string(), "value".to_string()],
            client_id,
        );

        let result = cmd.validate_arity(&CommandArity::Fixed(3));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_arity_fixed_invalid() {
        let client_id = Uuid::new_v4();
        let cmd = ParsedCommand::new(
            "SET".to_string(),
            vec!["key".to_string()],
            client_id,
        );

        let result = cmd.validate_arity(&CommandArity::Fixed(3));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("wrong number of arguments"));
    }

    #[test]
    fn test_validate_arity_range_valid() {
        let client_id = Uuid::new_v4();
        let cmd = ParsedCommand::new(
            "DEL".to_string(),
            vec!["key1".to_string(), "key2".to_string()],
            client_id,
        );

        let result = cmd.validate_arity(&CommandArity::Range(2, 5));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_arity_range_too_few() {
        let client_id = Uuid::new_v4();
        let cmd = ParsedCommand::new("DEL".to_string(), vec![], client_id);

        let result = cmd.validate_arity(&CommandArity::Range(2, 5));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected 2-5"));
    }

    #[test]
    fn test_validate_arity_range_too_many() {
        let client_id = Uuid::new_v4();
        let cmd = ParsedCommand::new(
            "DEL".to_string(),
            vec!["k1".to_string(), "k2".to_string(), "k3".to_string(), "k4".to_string(), "k5".to_string()],
            client_id,
        );

        let result = cmd.validate_arity(&CommandArity::Range(2, 5));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected 2-5"));
    }

    #[test]
    fn test_validate_arity_at_least_valid() {
        let client_id = Uuid::new_v4();
        let cmd = ParsedCommand::new(
            "DEL".to_string(),
            vec!["key1".to_string(), "key2".to_string(), "key3".to_string()],
            client_id,
        );

        let result = cmd.validate_arity(&CommandArity::AtLeast(2));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_arity_at_least_invalid() {
        let client_id = Uuid::new_v4();
        let cmd = ParsedCommand::new("DEL".to_string(), vec![], client_id);

        let result = cmd.validate_arity(&CommandArity::AtLeast(2));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected at least 2"));
    }

    #[test]
    fn test_command_registry_new() {
        let registry = CommandRegistry::new();
        assert_eq!(registry.command_count(), 0);
        assert!(registry.command_names().is_empty());
    }

    #[test]
    fn test_command_registry_register() {
        let mut registry = CommandRegistry::new();
        let mock_cmd = Box::new(MockCommand {
            name: "TEST",
            arity: CommandArity::Fixed(1),
            should_error: false,
        });

        registry.register(mock_cmd);

        assert_eq!(registry.command_count(), 1);
        assert!(registry.has_command("TEST"));
        assert!(registry.has_command("test")); // Case insensitive
        assert!(!registry.has_command("NONEXISTENT"));
    }

    #[test]
    fn test_command_registry_command_names() {
        let mut registry = CommandRegistry::new();
        
        registry.register(Box::new(MockCommand {
            name: "SET",
            arity: CommandArity::Fixed(3),
            should_error: false,
        }));
        
        registry.register(Box::new(MockCommand {
            name: "GET",
            arity: CommandArity::Fixed(2),
            should_error: false,
        }));

        let names = registry.command_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"SET".to_string()));
        assert!(names.contains(&"GET".to_string()));
    }

    #[test]
    fn test_command_registry_get_command_info() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(MockCommand {
            name: "TEST",
            arity: CommandArity::Fixed(2),
            should_error: false,
        }));

        let info = registry.get_command_info("TEST");
        assert!(info.is_some());
        let (name, arity) = info.unwrap();
        assert_eq!(name, "TEST");
        assert_eq!(arity, CommandArity::Fixed(2));

        let info = registry.get_command_info("NONEXISTENT");
        assert!(info.is_none());
    }

    #[tokio::test]
    async fn test_command_registry_execute_success() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(MockCommand {
            name: "TEST",
            arity: CommandArity::Fixed(2),
            should_error: false,
        }));

        let store = MemoryStore::new();
        let client_id = Uuid::new_v4();
        let cmd = ParsedCommand::new("TEST".to_string(), vec!["arg1".to_string()], client_id);

        let result = registry.execute(&cmd, &store).await;
        match result {
            CommandResult::Ok(crate::commands::ResponseValue::SimpleString(msg)) => {
                assert_eq!(msg, "Mock response with 1 args");
            }
            _ => panic!("Expected successful command execution"),
        }
    }

    #[tokio::test]
    async fn test_command_registry_execute_unknown_command() {
        let registry = CommandRegistry::new();
        let store = MemoryStore::new();
        let client_id = Uuid::new_v4();
        let cmd = ParsedCommand::new("UNKNOWN".to_string(), vec![], client_id);

        let result = registry.execute(&cmd, &store).await;
        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("unknown command 'UNKNOWN'"));
            }
            _ => panic!("Expected error for unknown command"),
        }
    }

    #[tokio::test]
    async fn test_command_registry_execute_arity_error() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(MockCommand {
            name: "TEST",
            arity: CommandArity::Fixed(3),
            should_error: false,
        }));

        let store = MemoryStore::new();
        let client_id = Uuid::new_v4();
        let cmd = ParsedCommand::new("TEST".to_string(), vec!["arg1".to_string()], client_id);

        let result = registry.execute(&cmd, &store).await;
        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("wrong number of arguments"));
            }
            _ => panic!("Expected arity error"),
        }
    }

    #[tokio::test]
    async fn test_command_registry_execute_command_error() {
        let mut registry = CommandRegistry::new();
        registry.register(Box::new(MockCommand {
            name: "ERROR_CMD",
            arity: CommandArity::Fixed(1),
            should_error: true,
        }));

        let store = MemoryStore::new();
        let client_id = Uuid::new_v4();
        let cmd = ParsedCommand::new("ERROR_CMD".to_string(), vec![], client_id);

        let result = registry.execute(&cmd, &store).await;
        match result {
            CommandResult::Error(msg) => {
                assert_eq!(msg, "Mock error");
            }
            _ => panic!("Expected command error"),
        }
    }

    #[test]
    fn test_command_result_equality() {
        let result1 = CommandResult::Ok(crate::commands::ResponseValue::SimpleString("test".to_string()));
        let result2 = CommandResult::Ok(crate::commands::ResponseValue::SimpleString("test".to_string()));
        let result3 = CommandResult::Error("error".to_string());
        let result4 = CommandResult::Error("error".to_string());

        assert_eq!(result1, result2);
        assert_eq!(result3, result4);
        assert_ne!(result1, result3);
    }

    #[test]
    fn test_response_value_types() {
        let simple = crate::commands::ResponseValue::SimpleString("OK".to_string());
        let bulk = crate::commands::ResponseValue::BulkString(Some("value".to_string()));
        let bulk_nil = crate::commands::ResponseValue::BulkString(None);
        let integer = crate::commands::ResponseValue::Integer(42);
        let array = crate::commands::ResponseValue::Array(vec![simple.clone()]);
        let nil = crate::commands::ResponseValue::Nil;

        // Test that all variants can be created and are different
        assert_ne!(simple, bulk);
        assert_ne!(bulk, bulk_nil);
        assert_ne!(integer, array);
        assert_ne!(array, nil);
    }
}