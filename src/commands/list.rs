//! List command implementations (LPUSH, RPUSH, LPOP, RPOP, LLEN, LRANGE)

use crate::commands::{Command, CommandArity, CommandResult, ResponseValue};
use crate::storage::MemoryStore;
use async_trait::async_trait;
use uuid::Uuid;

/// LPUSH command implementation
/// Insert all the specified values at the head of the list stored at key
pub struct LpushCommand;

#[async_trait]
impl Command for LpushCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore, _client_id: Uuid) -> CommandResult {
        if args.len() < 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'LPUSH' command".to_string(),
            );
        }

        let key = &args[0];
        let values = &args[1..];

        match store.lpush(key.clone(), values).await {
            Ok(new_len) => CommandResult::Ok(ResponseValue::Integer(new_len)),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "LPUSH"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::AtLeast(3) // LPUSH key value [value ...]
    }
}

/// RPUSH command implementation
/// Insert all the specified values at the tail of the list stored at key
pub struct RpushCommand;

#[async_trait]
impl Command for RpushCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore, _client_id: Uuid) -> CommandResult {
        if args.len() < 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'RPUSH' command".to_string(),
            );
        }

        let key = &args[0];
        let values = &args[1..];

        match store.rpush(key.clone(), values).await {
            Ok(new_len) => CommandResult::Ok(ResponseValue::Integer(new_len)),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "RPUSH"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::AtLeast(3) // RPUSH key value [value ...]
    }
}

/// LPOP command implementation
/// Removes and returns the first element(s) of the list stored at key
pub struct LpopCommand;

#[async_trait]
impl Command for LpopCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore, _client_id: Uuid) -> CommandResult {
        if args.is_empty() || args.len() > 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'LPOP' command".to_string(),
            );
        }

        let key = &args[0];
        let count = if args.len() == 2 {
            match args[1].parse::<usize>() {
                Ok(c) => Some(c),
                Err(_) => {
                    return CommandResult::Error(
                        "ERR value is not an integer or out of range".to_string(),
                    )
                }
            }
        } else {
            None
        };

        match store.lpop(key, count).await {
            Ok(Some(values)) => {
                if count.is_some() {
                    // Return array when count is specified
                    let array = values
                        .into_iter()
                        .map(|v| ResponseValue::BulkString(Some(v)))
                        .collect();
                    CommandResult::Ok(ResponseValue::Array(array))
                } else {
                    // Return single value (or nil) when no count
                    CommandResult::Ok(ResponseValue::BulkString(values.into_iter().next()))
                }
            }
            Ok(None) => {
                if count.is_some() {
                    // Return empty array when count specified but list doesn't exist
                    CommandResult::Ok(ResponseValue::Array(vec![]))
                } else {
                    CommandResult::Ok(ResponseValue::BulkString(None))
                }
            }
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "LPOP"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Range(2, 3) // LPOP key [count]
    }
}

/// RPOP command implementation
/// Removes and returns the last element(s) of the list stored at key
pub struct RpopCommand;

#[async_trait]
impl Command for RpopCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore, _client_id: Uuid) -> CommandResult {
        if args.is_empty() || args.len() > 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'RPOP' command".to_string(),
            );
        }

        let key = &args[0];
        let count = if args.len() == 2 {
            match args[1].parse::<usize>() {
                Ok(c) => Some(c),
                Err(_) => {
                    return CommandResult::Error(
                        "ERR value is not an integer or out of range".to_string(),
                    )
                }
            }
        } else {
            None
        };

        match store.rpop(key, count).await {
            Ok(Some(values)) => {
                if count.is_some() {
                    // Return array when count is specified
                    let array = values
                        .into_iter()
                        .map(|v| ResponseValue::BulkString(Some(v)))
                        .collect();
                    CommandResult::Ok(ResponseValue::Array(array))
                } else {
                    // Return single value (or nil) when no count
                    CommandResult::Ok(ResponseValue::BulkString(values.into_iter().next()))
                }
            }
            Ok(None) => {
                if count.is_some() {
                    // Return empty array when count specified but list doesn't exist
                    CommandResult::Ok(ResponseValue::Array(vec![]))
                } else {
                    CommandResult::Ok(ResponseValue::BulkString(None))
                }
            }
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "RPOP"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Range(2, 3) // RPOP key [count]
    }
}

/// LLEN command implementation
/// Returns the length of the list stored at key
pub struct LlenCommand;

#[async_trait]
impl Command for LlenCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore, _client_id: Uuid) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'LLEN' command".to_string(),
            );
        }

        let key = &args[0];

        match store.llen(key) {
            Ok(len) => CommandResult::Ok(ResponseValue::Integer(len)),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "LLEN"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2) // LLEN key
    }
}

/// LRANGE command implementation
/// Returns the specified elements of the list stored at key
pub struct LrangeCommand;

#[async_trait]
impl Command for LrangeCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore, _client_id: Uuid) -> CommandResult {
        if args.len() != 3 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'LRANGE' command".to_string(),
            );
        }

        let key = &args[0];
        let start = match args[1].parse::<i64>() {
            Ok(s) => s,
            Err(_) => {
                return CommandResult::Error(
                    "ERR value is not an integer or out of range".to_string(),
                )
            }
        };
        let stop = match args[2].parse::<i64>() {
            Ok(s) => s,
            Err(_) => {
                return CommandResult::Error(
                    "ERR value is not an integer or out of range".to_string(),
                )
            }
        };

        match store.lrange(key, start, stop) {
            Ok(values) => {
                let array = values
                    .into_iter()
                    .map(|v| ResponseValue::BulkString(Some(v)))
                    .collect();
                CommandResult::Ok(ResponseValue::Array(array))
            }
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "LRANGE"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(4) // LRANGE key start stop
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStore;
    use uuid::Uuid;

    // Helper function to create a test store
    fn create_test_store() -> MemoryStore {
        MemoryStore::new()
    }

    fn test_client_id() -> Uuid {
        Uuid::new_v4()
    }

    // ==================== LPUSH Command Tests ====================

    #[tokio::test]
    async fn test_lpush_command_new_key() {
        let store = create_test_store();
        let cmd = LpushCommand;
        let args = vec!["mylist".to_string(), "value1".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(len)) => {
                assert_eq!(len, 1);
            }
            _ => panic!("Expected integer response from LPUSH command"),
        }

        // Verify the value was stored
        let values = store.lrange("mylist", 0, -1).unwrap();
        assert_eq!(values, vec!["value1"]);
    }

    #[tokio::test]
    async fn test_lpush_command_multiple_values() {
        let store = create_test_store();
        let cmd = LpushCommand;
        let args = vec![
            "mylist".to_string(),
            "value1".to_string(),
            "value2".to_string(),
            "value3".to_string(),
        ];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(len)) => {
                assert_eq!(len, 3);
            }
            _ => panic!("Expected integer response from LPUSH command"),
        }

        // Verify values are in correct order (last pushed is at head)
        let values = store.lrange("mylist", 0, -1).unwrap();
        assert_eq!(values, vec!["value1", "value2", "value3"]);
    }

    #[tokio::test]
    async fn test_lpush_command_existing_list() {
        let store = create_test_store();
        let cmd = LpushCommand;

        // Push initial values
        let args1 = vec!["mylist".to_string(), "value1".to_string()];
        cmd.execute(&args1, &store, test_client_id()).await;

        // Push more values
        let args2 = vec!["mylist".to_string(), "value2".to_string()];
        let result = cmd.execute(&args2, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(len)) => {
                assert_eq!(len, 2);
            }
            _ => panic!("Expected integer response from LPUSH command"),
        }

        // value2 should be at head
        let values = store.lrange("mylist", 0, -1).unwrap();
        assert_eq!(values, vec!["value2", "value1"]);
    }

    #[tokio::test]
    async fn test_lpush_command_wrong_args() {
        let store = create_test_store();
        let cmd = LpushCommand;

        // Too few arguments
        let args = vec!["mylist".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn test_lpush_command_type_error() {
        let store = create_test_store();

        // Set a string value first
        store.set("mykey", "string_value").await.unwrap();

        let cmd = LpushCommand;
        let args = vec!["mykey".to_string(), "value1".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[test]
    fn test_lpush_command_properties() {
        let cmd = LpushCommand;
        assert_eq!(cmd.name(), "LPUSH");
        assert_eq!(cmd.arity(), CommandArity::AtLeast(3));
    }

    // ==================== RPUSH Command Tests ====================

    #[tokio::test]
    async fn test_rpush_command_new_key() {
        let store = create_test_store();
        let cmd = RpushCommand;
        let args = vec!["mylist".to_string(), "value1".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(len)) => {
                assert_eq!(len, 1);
            }
            _ => panic!("Expected integer response from RPUSH command"),
        }

        let values = store.lrange("mylist", 0, -1).unwrap();
        assert_eq!(values, vec!["value1"]);
    }

    #[tokio::test]
    async fn test_rpush_command_multiple_values() {
        let store = create_test_store();
        let cmd = RpushCommand;
        let args = vec![
            "mylist".to_string(),
            "value1".to_string(),
            "value2".to_string(),
            "value3".to_string(),
        ];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(len)) => {
                assert_eq!(len, 3);
            }
            _ => panic!("Expected integer response from RPUSH command"),
        }

        // Verify values are in correct order (first pushed is at head)
        let values = store.lrange("mylist", 0, -1).unwrap();
        assert_eq!(values, vec!["value1", "value2", "value3"]);
    }

    #[tokio::test]
    async fn test_rpush_command_existing_list() {
        let store = create_test_store();
        let cmd = RpushCommand;

        // Push initial values
        let args1 = vec!["mylist".to_string(), "value1".to_string()];
        cmd.execute(&args1, &store, test_client_id()).await;

        // Push more values
        let args2 = vec!["mylist".to_string(), "value2".to_string()];
        let result = cmd.execute(&args2, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(len)) => {
                assert_eq!(len, 2);
            }
            _ => panic!("Expected integer response from RPUSH command"),
        }

        // value2 should be at tail
        let values = store.lrange("mylist", 0, -1).unwrap();
        assert_eq!(values, vec!["value1", "value2"]);
    }

    #[tokio::test]
    async fn test_rpush_command_wrong_args() {
        let store = create_test_store();
        let cmd = RpushCommand;

        let args = vec!["mylist".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[test]
    fn test_rpush_command_properties() {
        let cmd = RpushCommand;
        assert_eq!(cmd.name(), "RPUSH");
        assert_eq!(cmd.arity(), CommandArity::AtLeast(3));
    }

    // ==================== LPOP Command Tests ====================

    #[tokio::test]
    async fn test_lpop_command_single() {
        let store = create_test_store();

        // Set up test data
        store
            .rpush("mylist".to_string(), &["a".to_string(), "b".to_string(), "c".to_string()])
            .await
            .unwrap();

        let cmd = LpopCommand;
        let args = vec!["mylist".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(Some(value))) => {
                assert_eq!(value, "a");
            }
            _ => panic!("Expected bulk string response from LPOP command"),
        }

        // Verify remaining elements
        let values = store.lrange("mylist", 0, -1).unwrap();
        assert_eq!(values, vec!["b", "c"]);
    }

    #[tokio::test]
    async fn test_lpop_command_with_count() {
        let store = create_test_store();

        store
            .rpush(
                "mylist".to_string(),
                &["a".to_string(), "b".to_string(), "c".to_string(), "d".to_string()],
            )
            .await
            .unwrap();

        let cmd = LpopCommand;
        let args = vec!["mylist".to_string(), "2".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(values)) => {
                assert_eq!(values.len(), 2);
                assert!(matches!(&values[0], ResponseValue::BulkString(Some(v)) if v == "a"));
                assert!(matches!(&values[1], ResponseValue::BulkString(Some(v)) if v == "b"));
            }
            _ => panic!("Expected array response from LPOP command with count"),
        }

        // Verify remaining elements
        let values = store.lrange("mylist", 0, -1).unwrap();
        assert_eq!(values, vec!["c", "d"]);
    }

    #[tokio::test]
    async fn test_lpop_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = LpopCommand;
        let args = vec!["nonexistent".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(None)) => {
                // Expected
            }
            _ => panic!("Expected nil response for nonexistent key"),
        }
    }

    #[tokio::test]
    async fn test_lpop_command_nonexistent_key_with_count() {
        let store = create_test_store();
        let cmd = LpopCommand;
        let args = vec!["nonexistent".to_string(), "2".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(values)) => {
                assert!(values.is_empty());
            }
            _ => panic!("Expected empty array for nonexistent key with count"),
        }
    }

    #[tokio::test]
    async fn test_lpop_command_removes_empty_list() {
        let store = create_test_store();

        store.rpush("mylist".to_string(), &["a".to_string()]).await.unwrap();

        let cmd = LpopCommand;
        let args = vec!["mylist".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::BulkString(Some(ref v))) if v == "a"
        ));

        // Key should be removed when list becomes empty
        assert!(!store.exists("mylist").unwrap());
    }

    #[tokio::test]
    async fn test_lpop_command_invalid_count() {
        let store = create_test_store();
        let cmd = LpopCommand;
        let args = vec!["mylist".to_string(), "not_a_number".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn test_lpop_command_wrong_args() {
        let store = create_test_store();
        let cmd = LpopCommand;

        // No arguments
        let args: Vec<String> = vec![];
        let result = cmd.execute(&args, &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(_)));

        // Too many arguments
        let args = vec!["key".to_string(), "1".to_string(), "extra".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[test]
    fn test_lpop_command_properties() {
        let cmd = LpopCommand;
        assert_eq!(cmd.name(), "LPOP");
        assert_eq!(cmd.arity(), CommandArity::Range(2, 3));
    }

    // ==================== RPOP Command Tests ====================

    #[tokio::test]
    async fn test_rpop_command_single() {
        let store = create_test_store();

        store
            .rpush("mylist".to_string(), &["a".to_string(), "b".to_string(), "c".to_string()])
            .await
            .unwrap();

        let cmd = RpopCommand;
        let args = vec!["mylist".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(Some(value))) => {
                assert_eq!(value, "c");
            }
            _ => panic!("Expected bulk string response from RPOP command"),
        }

        let values = store.lrange("mylist", 0, -1).unwrap();
        assert_eq!(values, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn test_rpop_command_with_count() {
        let store = create_test_store();

        store
            .rpush(
                "mylist".to_string(),
                &["a".to_string(), "b".to_string(), "c".to_string(), "d".to_string()],
            )
            .await
            .unwrap();

        let cmd = RpopCommand;
        let args = vec!["mylist".to_string(), "2".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(values)) => {
                assert_eq!(values.len(), 2);
                assert!(matches!(&values[0], ResponseValue::BulkString(Some(v)) if v == "d"));
                assert!(matches!(&values[1], ResponseValue::BulkString(Some(v)) if v == "c"));
            }
            _ => panic!("Expected array response from RPOP command with count"),
        }

        let values = store.lrange("mylist", 0, -1).unwrap();
        assert_eq!(values, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn test_rpop_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = RpopCommand;
        let args = vec!["nonexistent".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(None)) => {}
            _ => panic!("Expected nil response for nonexistent key"),
        }
    }

    #[test]
    fn test_rpop_command_properties() {
        let cmd = RpopCommand;
        assert_eq!(cmd.name(), "RPOP");
        assert_eq!(cmd.arity(), CommandArity::Range(2, 3));
    }

    // ==================== LLEN Command Tests ====================

    #[tokio::test]
    async fn test_llen_command_existing_list() {
        let store = create_test_store();

        store
            .rpush("mylist".to_string(), &["a".to_string(), "b".to_string(), "c".to_string()])
            .await
            .unwrap();

        let cmd = LlenCommand;
        let args = vec!["mylist".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(len)) => {
                assert_eq!(len, 3);
            }
            _ => panic!("Expected integer response from LLEN command"),
        }
    }

    #[tokio::test]
    async fn test_llen_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = LlenCommand;
        let args = vec!["nonexistent".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(len)) => {
                assert_eq!(len, 0);
            }
            _ => panic!("Expected integer 0 for nonexistent key"),
        }
    }

    #[tokio::test]
    async fn test_llen_command_type_error() {
        let store = create_test_store();
        store.set("mykey", "string_value").await.unwrap();

        let cmd = LlenCommand;
        let args = vec!["mykey".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn test_llen_command_wrong_args() {
        let store = create_test_store();
        let cmd = LlenCommand;

        // No arguments
        let args: Vec<String> = vec![];
        let result = cmd.execute(&args, &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(_)));

        // Too many arguments
        let args = vec!["key".to_string(), "extra".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[test]
    fn test_llen_command_properties() {
        let cmd = LlenCommand;
        assert_eq!(cmd.name(), "LLEN");
        assert_eq!(cmd.arity(), CommandArity::Fixed(2));
    }

    // ==================== LRANGE Command Tests ====================

    #[tokio::test]
    async fn test_lrange_command_full_range() {
        let store = create_test_store();

        store
            .rpush(
                "mylist".to_string(),
                &["a".to_string(), "b".to_string(), "c".to_string(), "d".to_string()],
            )
            .await
            .unwrap();

        let cmd = LrangeCommand;
        let args = vec!["mylist".to_string(), "0".to_string(), "-1".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(values)) => {
                assert_eq!(values.len(), 4);
                assert!(matches!(&values[0], ResponseValue::BulkString(Some(v)) if v == "a"));
                assert!(matches!(&values[1], ResponseValue::BulkString(Some(v)) if v == "b"));
                assert!(matches!(&values[2], ResponseValue::BulkString(Some(v)) if v == "c"));
                assert!(matches!(&values[3], ResponseValue::BulkString(Some(v)) if v == "d"));
            }
            _ => panic!("Expected array response from LRANGE command"),
        }
    }

    #[tokio::test]
    async fn test_lrange_command_partial_range() {
        let store = create_test_store();

        store
            .rpush(
                "mylist".to_string(),
                &["a".to_string(), "b".to_string(), "c".to_string(), "d".to_string()],
            )
            .await
            .unwrap();

        let cmd = LrangeCommand;
        let args = vec!["mylist".to_string(), "1".to_string(), "2".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(values)) => {
                assert_eq!(values.len(), 2);
                assert!(matches!(&values[0], ResponseValue::BulkString(Some(v)) if v == "b"));
                assert!(matches!(&values[1], ResponseValue::BulkString(Some(v)) if v == "c"));
            }
            _ => panic!("Expected array response from LRANGE command"),
        }
    }

    #[tokio::test]
    async fn test_lrange_command_negative_indices() {
        let store = create_test_store();

        store
            .rpush(
                "mylist".to_string(),
                &["a".to_string(), "b".to_string(), "c".to_string(), "d".to_string()],
            )
            .await
            .unwrap();

        let cmd = LrangeCommand;
        let args = vec!["mylist".to_string(), "-3".to_string(), "-2".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(values)) => {
                assert_eq!(values.len(), 2);
                assert!(matches!(&values[0], ResponseValue::BulkString(Some(v)) if v == "b"));
                assert!(matches!(&values[1], ResponseValue::BulkString(Some(v)) if v == "c"));
            }
            _ => panic!("Expected array response from LRANGE command"),
        }
    }

    #[tokio::test]
    async fn test_lrange_command_out_of_bounds() {
        let store = create_test_store();

        store
            .rpush("mylist".to_string(), &["a".to_string(), "b".to_string()])
            .await
            .unwrap();

        let cmd = LrangeCommand;
        let args = vec!["mylist".to_string(), "0".to_string(), "100".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(values)) => {
                assert_eq!(values.len(), 2);
            }
            _ => panic!("Expected array response from LRANGE command"),
        }
    }

    #[tokio::test]
    async fn test_lrange_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = LrangeCommand;
        let args = vec!["nonexistent".to_string(), "0".to_string(), "-1".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(values)) => {
                assert!(values.is_empty());
            }
            _ => panic!("Expected empty array for nonexistent key"),
        }
    }

    #[tokio::test]
    async fn test_lrange_command_type_error() {
        let store = create_test_store();
        store.set("mykey", "string_value").await.unwrap();

        let cmd = LrangeCommand;
        let args = vec!["mykey".to_string(), "0".to_string(), "-1".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn test_lrange_command_invalid_indices() {
        let store = create_test_store();
        let cmd = LrangeCommand;

        let args = vec![
            "mylist".to_string(),
            "not_a_number".to_string(),
            "0".to_string(),
        ];
        let result = cmd.execute(&args, &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(_)));

        let args = vec![
            "mylist".to_string(),
            "0".to_string(),
            "not_a_number".to_string(),
        ];
        let result = cmd.execute(&args, &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn test_lrange_command_wrong_args() {
        let store = create_test_store();
        let cmd = LrangeCommand;

        // Too few arguments
        let args = vec!["mylist".to_string(), "0".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(_)));

        // Too many arguments
        let args = vec![
            "mylist".to_string(),
            "0".to_string(),
            "-1".to_string(),
            "extra".to_string(),
        ];
        let result = cmd.execute(&args, &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[test]
    fn test_lrange_command_properties() {
        let cmd = LrangeCommand;
        assert_eq!(cmd.name(), "LRANGE");
        assert_eq!(cmd.arity(), CommandArity::Fixed(4));
    }

    // ==================== Integration Tests ====================

    #[tokio::test]
    async fn test_list_operations_integration() {
        let store = create_test_store();

        let lpush_cmd = LpushCommand;
        let rpush_cmd = RpushCommand;
        let lpop_cmd = LpopCommand;
        let rpop_cmd = RpopCommand;
        let llen_cmd = LlenCommand;
        let lrange_cmd = LrangeCommand;

        // Build a list using LPUSH and RPUSH
        // LPUSH mylist a -> [a]
        let result = lpush_cmd
            .execute(&["mylist".to_string(), "a".to_string()], &store, test_client_id())
            .await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        // RPUSH mylist b -> [a, b]
        let result = rpush_cmd
            .execute(&["mylist".to_string(), "b".to_string()], &store, test_client_id())
            .await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(2))));

        // LPUSH mylist c -> [c, a, b]
        let result = lpush_cmd
            .execute(&["mylist".to_string(), "c".to_string()], &store, test_client_id())
            .await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(3))));

        // LLEN mylist -> 3
        let result = llen_cmd
            .execute(&["mylist".to_string()], &store, test_client_id())
            .await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(3))));

        // LRANGE mylist 0 -1 -> [c, a, b]
        let result = lrange_cmd
            .execute(
                &["mylist".to_string(), "0".to_string(), "-1".to_string()],
                &store,
                test_client_id(),
            )
            .await;
        if let CommandResult::Ok(ResponseValue::Array(values)) = result {
            assert_eq!(values.len(), 3);
        } else {
            panic!("Expected array response");
        }

        // LPOP mylist -> c, list becomes [a, b]
        let result = lpop_cmd
            .execute(&["mylist".to_string()], &store, test_client_id())
            .await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::BulkString(Some(ref v))) if v == "c"
        ));

        // RPOP mylist -> b, list becomes [a]
        let result = rpop_cmd
            .execute(&["mylist".to_string()], &store, test_client_id())
            .await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::BulkString(Some(ref v))) if v == "b"
        ));

        // LLEN mylist -> 1
        let result = llen_cmd
            .execute(&["mylist".to_string()], &store, test_client_id())
            .await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        // Pop the last element
        let result = lpop_cmd
            .execute(&["mylist".to_string()], &store, test_client_id())
            .await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::BulkString(Some(ref v))) if v == "a"
        ));

        // Key should be removed when list is empty
        assert!(!store.exists("mylist").unwrap());

        // LLEN on nonexistent key -> 0
        let result = llen_cmd
            .execute(&["mylist".to_string()], &store, test_client_id())
            .await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(0))));
    }

    #[tokio::test]
    async fn test_concurrent_list_operations() {
        use std::sync::Arc;
        use tokio::task::JoinSet;

        let store = Arc::new(MemoryStore::new());
        let mut join_set = JoinSet::new();

        // Spawn multiple tasks performing list operations concurrently
        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            join_set.spawn(async move {
                let value = format!("value{i}");

                // RPUSH
                store_clone
                    .rpush("concurrent_list".to_string(), std::slice::from_ref(&value))
                    .await
                    .unwrap();

                // LLEN
                let _len = store_clone.llen("concurrent_list").unwrap();
            });
        }

        // Wait for all tasks to complete
        while let Some(result) = join_set.join_next().await {
            result.unwrap();
        }

        // Verify final state
        let len = store.llen("concurrent_list").unwrap();
        assert_eq!(len, 10);

        let values = store.lrange("concurrent_list", 0, -1).unwrap();
        assert_eq!(values.len(), 10);
    }
}
