//! String command implementations (SET, GET, DEL, EXISTS)

use crate::commands::{Command, CommandArity, CommandResult, ResponseValue};
use crate::storage::{MemoryStore, SetExistence, SetTtlOption};
use async_trait::async_trait;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// SET command implementation
///
/// Supports the full Redis 7 option syntax:
///
/// `SET key value [NX | XX] [EX seconds | PX milliseconds | EXAT unix-time-seconds | PXAT unix-time-milliseconds | KEEPTTL] [GET]`
pub struct SetCommand;

#[derive(Debug, Default)]
struct SetOptions {
    ttl: SetTtlChoice,
    existence: SetExistence,
    get: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum SetTtlChoice {
    #[default]
    Default, // clears any existing TTL (Redis default)
    KeepTtl,
    Seconds(u64),
    Milliseconds(u64),
    UnixSeconds(u64),
    UnixMilliseconds(u64),
}

fn parse_set_options(args: &[String]) -> std::result::Result<SetOptions, String> {
    let mut opts = SetOptions::default();
    let mut ttl_set = false;
    let mut existence_set = false;
    let mut i = 0;
    while i < args.len() {
        let token = args[i].to_ascii_uppercase();
        match token.as_str() {
            "NX" | "XX" => {
                if existence_set {
                    return Err("ERR syntax error".to_string());
                }
                opts.existence = if token == "NX" {
                    SetExistence::OnlyIfNotExists
                } else {
                    SetExistence::OnlyIfExists
                };
                existence_set = true;
                i += 1;
            }
            "KEEPTTL" => {
                if ttl_set {
                    return Err("ERR syntax error".to_string());
                }
                opts.ttl = SetTtlChoice::KeepTtl;
                ttl_set = true;
                i += 1;
            }
            "EX" | "PX" | "EXAT" | "PXAT" => {
                if ttl_set {
                    return Err("ERR syntax error".to_string());
                }
                if i + 1 >= args.len() {
                    return Err("ERR syntax error".to_string());
                }
                let n = args[i + 1]
                    .parse::<u64>()
                    .map_err(|_| "ERR value is not an integer or out of range".to_string())?;
                if matches!(token.as_str(), "EX" | "PX") && n == 0 {
                    return Err("ERR invalid expire time in 'set' command".to_string());
                }
                opts.ttl = match token.as_str() {
                    "EX" => SetTtlChoice::Seconds(n),
                    "PX" => SetTtlChoice::Milliseconds(n),
                    "EXAT" => SetTtlChoice::UnixSeconds(n),
                    "PXAT" => SetTtlChoice::UnixMilliseconds(n),
                    _ => unreachable!(),
                };
                ttl_set = true;
                i += 2;
            }
            "GET" => {
                if opts.get {
                    return Err("ERR syntax error".to_string());
                }
                opts.get = true;
                i += 1;
            }
            _ => return Err("ERR syntax error".to_string()),
        }
    }
    Ok(opts)
}

/// Convert the parsed TTL choice into a `SetTtlOption` whose `At`
/// variant is anchored to `Instant::now()`. Wall-clock options
/// (`EXAT`/`PXAT`) are translated by computing the delta against
/// `SystemTime::now()` and adding it to a freshly captured `Instant`,
/// because `StoredValue::expires_at` is monotonic.
fn resolve_ttl(choice: &SetTtlChoice) -> std::result::Result<SetTtlOption, String> {
    match choice {
        SetTtlChoice::Default => Ok(SetTtlOption::None),
        SetTtlChoice::KeepTtl => Ok(SetTtlOption::KeepTtl),
        SetTtlChoice::Seconds(n) => Ok(SetTtlOption::At(Instant::now() + Duration::from_secs(*n))),
        SetTtlChoice::Milliseconds(n) => {
            Ok(SetTtlOption::At(Instant::now() + Duration::from_millis(*n)))
        }
        SetTtlChoice::UnixSeconds(target) | SetTtlChoice::UnixMilliseconds(target) => {
            let target_ms = match choice {
                SetTtlChoice::UnixSeconds(n) => n.saturating_mul(1000),
                SetTtlChoice::UnixMilliseconds(n) => *n,
                _ => unreachable!(),
            };
            let _ = target;
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "ERR system clock error".to_string())?
                .as_millis() as u64;
            if target_ms <= now_ms {
                // Past deadline: store with a deadline already in the
                // past, which immediately reads as expired.
                return Ok(SetTtlOption::At(Instant::now() - Duration::from_millis(1)));
            }
            let delta = Duration::from_millis(target_ms - now_ms);
            Ok(SetTtlOption::At(Instant::now() + delta))
        }
    }
}

#[async_trait]
impl Command for SetCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if args.len() < 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'SET' command".to_string(),
            );
        }

        let key = &args[0];
        let value = &args[1];

        let opts = match parse_set_options(&args[2..]) {
            Ok(o) => o,
            Err(e) => return CommandResult::Error(e),
        };
        let ttl = match resolve_ttl(&opts.ttl) {
            Ok(t) => t,
            Err(e) => return CommandResult::Error(e),
        };

        match store
            .set_with_options(key.as_str(), value.as_str(), ttl, opts.existence)
            .await
        {
            Ok(outcome) => {
                if opts.get {
                    // GET: return the previous value (or nil), regardless
                    // of whether the write was applied.
                    match outcome.previous {
                        Some(sv) => match &sv.value {
                            crate::storage::ValueType::String(_)
                            | crate::storage::ValueType::Integer(_) => {
                                CommandResult::Ok(ResponseValue::bulk(sv.value.to_string()))
                            }
                            _ => CommandResult::Error(
                                "WRONGTYPE Operation against a key holding the wrong kind of value"
                                    .to_string(),
                            ),
                        },
                        None => CommandResult::Ok(ResponseValue::nil_bulk()),
                    }
                } else if outcome.applied {
                    CommandResult::Ok(ResponseValue::SimpleString("OK".to_string()))
                } else {
                    // NX/XX failed: Redis returns nil bulk.
                    CommandResult::Ok(ResponseValue::nil_bulk())
                }
            }
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "SET"
    }

    fn arity(&self) -> CommandArity {
        // SET key value [options...] — at least 3 args including command name.
        CommandArity::AtLeast(3)
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
            Ok(Some(stored_value)) => {
                CommandResult::Ok(ResponseValue::bulk(stored_value.value.to_string()))
            }
            Ok(None) => CommandResult::Ok(ResponseValue::nil_bulk()),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "GET"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2) // GET key
    }

    fn is_mutation(&self) -> bool {
        false
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

    fn is_mutation(&self) -> bool {
        false
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
    async fn test_set_command_unknown_option() {
        // With option support a third arg that isn't a recognized
        // option keyword now produces a `syntax error`, not a
        // `wrong number of arguments` error — Redis behaves this way.
        let store = create_test_store();
        let cmd = SetCommand;
        let args = vec!["key".to_string(), "value".to_string(), "extra".to_string()];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("syntax error"), "got: {msg}");
            }
            _ => panic!("Expected syntax error for unrecognized option"),
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
        // SET key value [option ...] — at least 3 args after Stage 8
        // option support; previously Fixed(3).
        assert_eq!(cmd.arity(), CommandArity::AtLeast(3));
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

    // SET option tests (Stage 8: NX/XX/EX/PX/EXAT/PXAT/KEEPTTL/GET)

    #[tokio::test]
    async fn test_set_nx_on_missing_key_writes() {
        let store = create_test_store();
        let cmd = SetCommand;
        let args = vec!["k".to_string(), "v".to_string(), "NX".to_string()];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::SimpleString(_))
        ));
        assert_eq!(store.get("k").unwrap().unwrap().value.to_string(), "v");
    }

    #[tokio::test]
    async fn test_set_nx_on_existing_key_returns_nil() {
        let store = create_test_store();
        store.set("k", "old").await.unwrap();
        let cmd = SetCommand;
        let args = vec!["k".to_string(), "new".to_string(), "NX".to_string()];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::BulkString(None))
        ));
        // Old value preserved.
        assert_eq!(store.get("k").unwrap().unwrap().value.to_string(), "old");
    }

    #[tokio::test]
    async fn test_set_xx_on_missing_key_returns_nil() {
        let store = create_test_store();
        let cmd = SetCommand;
        let args = vec!["k".to_string(), "v".to_string(), "XX".to_string()];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::BulkString(None))
        ));
        assert!(store.get("k").unwrap().is_none());
    }

    #[tokio::test]
    async fn test_set_xx_on_existing_key_writes() {
        let store = create_test_store();
        store.set("k", "old").await.unwrap();
        let cmd = SetCommand;
        let args = vec!["k".to_string(), "new".to_string(), "XX".to_string()];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::SimpleString(_))
        ));
        assert_eq!(store.get("k").unwrap().unwrap().value.to_string(), "new");
    }

    #[tokio::test]
    async fn test_set_ex_applies_ttl_in_seconds() {
        let store = create_test_store();
        let cmd = SetCommand;
        let args = vec![
            "k".to_string(),
            "v".to_string(),
            "EX".to_string(),
            "60".to_string(),
        ];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::SimpleString(_))
        ));
        let ttl = store.ttl("k").unwrap();
        assert!(ttl > 0 && ttl <= 60, "ttl {ttl}");
    }

    #[tokio::test]
    async fn test_set_px_applies_ttl_in_milliseconds() {
        let store = create_test_store();
        let cmd = SetCommand;
        let args = vec![
            "k".to_string(),
            "v".to_string(),
            "PX".to_string(),
            "60000".to_string(),
        ];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::SimpleString(_))
        ));
        let ttl = store.ttl("k").unwrap();
        assert!(ttl > 0 && ttl <= 60, "ttl {ttl}");
    }

    #[tokio::test]
    async fn test_set_ex_zero_rejected() {
        let store = create_test_store();
        let cmd = SetCommand;
        let args = vec![
            "k".to_string(),
            "v".to_string(),
            "EX".to_string(),
            "0".to_string(),
        ];
        let result = cmd.execute(&args, &store).await;
        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("invalid expire time"), "got {msg}");
            }
            _ => panic!("expected EX 0 to be rejected"),
        }
    }

    #[tokio::test]
    async fn test_set_keepttl_preserves_existing_ttl() {
        let store = create_test_store();
        store.set_with_ttl("k", "old", 120).await.unwrap();
        let initial = store.ttl("k").unwrap();
        assert!(initial > 100);

        let cmd = SetCommand;
        let args = vec!["k".to_string(), "new".to_string(), "KEEPTTL".to_string()];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::SimpleString(_))
        ));
        let after = store.ttl("k").unwrap();
        assert!(after > 100, "ttl should be preserved, got {after}");
        assert_eq!(store.get("k").unwrap().unwrap().value.to_string(), "new");
    }

    #[tokio::test]
    async fn test_set_default_clears_existing_ttl() {
        let store = create_test_store();
        store.set_with_ttl("k", "old", 120).await.unwrap();
        let cmd = SetCommand;
        let args = vec!["k".to_string(), "new".to_string()];
        let _ = cmd.execute(&args, &store).await;
        assert_eq!(store.ttl("k").unwrap(), -1);
    }

    #[tokio::test]
    async fn test_set_get_returns_previous_value() {
        let store = create_test_store();
        store.set("k", "old").await.unwrap();
        let cmd = SetCommand;
        let args = vec!["k".to_string(), "new".to_string(), "GET".to_string()];
        let result = cmd.execute(&args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::BulkString(Some(v))) => {
                assert_eq!(v, "old");
            }
            other => panic!("expected old value, got {other:?}"),
        }
        assert_eq!(store.get("k").unwrap().unwrap().value.to_string(), "new");
    }

    #[tokio::test]
    async fn test_set_get_on_missing_key_returns_nil() {
        let store = create_test_store();
        let cmd = SetCommand;
        let args = vec!["k".to_string(), "new".to_string(), "GET".to_string()];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::BulkString(None))
        ));
        assert_eq!(store.get("k").unwrap().unwrap().value.to_string(), "new");
    }

    #[tokio::test]
    async fn test_set_nx_get_returns_previous_even_when_skipped() {
        // Per Redis spec: GET reports the prior value even if NX/XX
        // caused the write to be skipped.
        let store = create_test_store();
        store.set("k", "old").await.unwrap();
        let cmd = SetCommand;
        let args = vec![
            "k".to_string(),
            "new".to_string(),
            "NX".to_string(),
            "GET".to_string(),
        ];
        let result = cmd.execute(&args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::BulkString(Some(v))) => {
                assert_eq!(v, "old");
            }
            other => panic!("expected old value, got {other:?}"),
        }
        // Value not overwritten.
        assert_eq!(store.get("k").unwrap().unwrap().value.to_string(), "old");
    }

    #[tokio::test]
    async fn test_set_conflicting_nx_xx_rejected() {
        let store = create_test_store();
        let cmd = SetCommand;
        let args = vec![
            "k".to_string(),
            "v".to_string(),
            "NX".to_string(),
            "XX".to_string(),
        ];
        let result = cmd.execute(&args, &store).await;
        match result {
            CommandResult::Error(msg) => assert!(msg.contains("syntax error"), "{msg}"),
            _ => panic!("expected syntax error"),
        }
    }

    #[tokio::test]
    async fn test_set_conflicting_ttl_options_rejected() {
        let store = create_test_store();
        let cmd = SetCommand;
        let args = vec![
            "k".to_string(),
            "v".to_string(),
            "EX".to_string(),
            "60".to_string(),
            "PX".to_string(),
            "1000".to_string(),
        ];
        let result = cmd.execute(&args, &store).await;
        match result {
            CommandResult::Error(msg) => assert!(msg.contains("syntax error"), "{msg}"),
            _ => panic!("expected syntax error"),
        }
    }

    #[tokio::test]
    async fn test_set_get_on_hash_returns_wrongtype() {
        // GET option must surface WRONGTYPE if the prior value isn't
        // a string/integer (matches Redis).
        let store = create_test_store();
        store.hset("k", "f", "v").await.unwrap();
        let cmd = SetCommand;
        let args = vec!["k".to_string(), "new".to_string(), "GET".to_string()];
        let result = cmd.execute(&args, &store).await;
        match result {
            CommandResult::Error(msg) => assert!(msg.contains("WRONGTYPE"), "{msg}"),
            _ => panic!("expected WRONGTYPE"),
        }
    }

    #[tokio::test]
    async fn test_set_options_lowercase_accepted() {
        let store = create_test_store();
        let cmd = SetCommand;
        let args = vec![
            "k".to_string(),
            "v".to_string(),
            "ex".to_string(),
            "30".to_string(),
            "nx".to_string(),
        ];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::SimpleString(_))
        ));
        assert!(store.ttl("k").unwrap() > 0);
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
