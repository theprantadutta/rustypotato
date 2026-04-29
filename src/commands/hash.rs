//! Hash command implementations (HSET, HGET, HDEL, HGETALL, HEXISTS)

use crate::commands::registry::Args;
use crate::commands::{arg_str, Command, CommandArity, CommandResult, ResponseValue};
use crate::storage::MemoryStore;
use async_trait::async_trait;

/// HSET command implementation.
///
/// Variadic per Redis 4+: `HSET key field value [field value ...]`.
/// Returns the count of newly created fields (existing fields that
/// were updated do not increment the count). Real clients (e.g.
/// `redis-py.hset(mapping=...)`) emit the multi-pair form, so the
/// non-variadic form is a real-world incompatibility, not a curiosity.
pub struct HsetCommand;

#[async_trait]
impl Command for HsetCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        // Need key + at least one (field, value) pair, and pairs must
        // be balanced.
        if args.len() < 3 || !(args.len() - 1).is_multiple_of(2) {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'HSET' command".to_string(),
            );
        }

        let key = match arg_str(args, 0) {
            Ok(k) => k.to_string(),
            Err(e) => return CommandResult::Error(e),
        };

        let mut new_fields = 0i64;
        let mut i = 1;
        while i < args.len() {
            let field = match arg_str(args, i) {
                Ok(f) => f,
                Err(e) => return CommandResult::Error(e),
            };
            let value = match arg_str(args, i + 1) {
                Ok(v) => v,
                Err(e) => return CommandResult::Error(e),
            };
            match store.hset(key.as_str(), field, value).await {
                Ok(is_new_field) => {
                    if is_new_field {
                        new_fields += 1;
                    }
                }
                Err(e) => return CommandResult::Error(e.to_client_error()),
            }
            i += 2;
        }

        CommandResult::Ok(ResponseValue::Integer(new_fields))
    }

    fn name(&self) -> &'static str {
        "HSET"
    }

    fn arity(&self) -> CommandArity {
        // HSET key field value [field value ...] — at least 4 args
        // (cmd + key + 1 pair); pair-count balance is checked in
        // `execute` since `CommandArity` can't express "must be even".
        CommandArity::AtLeast(4)
    }
}

/// HGET command implementation
/// Gets the value of a hash field
pub struct HgetCommand;

#[async_trait]
impl Command for HgetCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() != 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'HGET' command".to_string(),
            );
        }

        let key = match arg_str(args, 0) {
            Ok(k) => k,
            Err(e) => return CommandResult::Error(e),
        };
        let field = match arg_str(args, 1) {
            Ok(f) => f,
            Err(e) => return CommandResult::Error(e),
        };

        match store.hget(key, field).await {
            Ok(Some(value)) => CommandResult::Ok(ResponseValue::bulk(value)),
            Ok(None) => CommandResult::Ok(ResponseValue::nil_bulk()),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "HGET"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(3) // HGET key field
    }

    fn is_mutation(&self) -> bool {
        false
    }
}

/// HDEL command implementation
/// Removes the specified fields from the hash stored at key
pub struct HdelCommand;

#[async_trait]
impl Command for HdelCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() < 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'HDEL' command".to_string(),
            );
        }

        let key = match arg_str(args, 0) {
            Ok(k) => k,
            Err(e) => return CommandResult::Error(e),
        };
        let mut deleted_count = 0i64;

        for i in 1..args.len() {
            let field = match arg_str(args, i) {
                Ok(f) => f,
                Err(e) => return CommandResult::Error(e),
            };
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
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'HGETALL' command".to_string(),
            );
        }

        let key = match arg_str(args, 0) {
            Ok(k) => k,
            Err(e) => return CommandResult::Error(e),
        };

        match store.hgetall(key).await {
            Ok(fields) => {
                // Convert to array of alternating field-value pairs
                let mut result = Vec::new();
                for (field, value) in fields {
                    result.push(ResponseValue::bulk(field));
                    result.push(ResponseValue::bulk(value));
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

    fn is_mutation(&self) -> bool {
        false
    }
}

/// `HMGET key field [field ...]` — return an array containing the
/// values for each field, or nil for missing fields. If the key
/// itself is missing, returns an array of nils of the same length as
/// the field list (matches Redis).
pub struct HmgetCommand;

#[async_trait]
impl Command for HmgetCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() < 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'HMGET' command".to_string(),
            );
        }

        let key = match arg_str(args, 0) {
            Ok(k) => k,
            Err(e) => return CommandResult::Error(e),
        };

        let mut out: Vec<ResponseValue> = Vec::with_capacity(args.len() - 1);
        for i in 1..args.len() {
            let field = match arg_str(args, i) {
                Ok(f) => f,
                Err(e) => return CommandResult::Error(e),
            };
            match store.hget(key, field).await {
                Ok(Some(v)) => out.push(ResponseValue::bulk(v)),
                Ok(None) => out.push(ResponseValue::nil_bulk()),
                Err(e) => return CommandResult::Error(e.to_client_error()),
            }
        }
        CommandResult::Ok(ResponseValue::Array(out))
    }

    fn name(&self) -> &'static str {
        "HMGET"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::AtLeast(3) // HMGET key field [field ...]
    }

    fn is_mutation(&self) -> bool {
        false
    }
}

/// `HKEYS key` — array of all field names in the hash.
pub struct HkeysCommand;

#[async_trait]
impl Command for HkeysCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'HKEYS' command".to_string(),
            );
        }
        let key = match arg_str(args, 0) {
            Ok(k) => k,
            Err(e) => return CommandResult::Error(e),
        };
        match store.hgetall(key).await {
            Ok(fields) => {
                let out: Vec<ResponseValue> = fields
                    .into_iter()
                    .map(|(field, _)| ResponseValue::bulk(field))
                    .collect();
                CommandResult::Ok(ResponseValue::Array(out))
            }
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "HKEYS"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2)
    }

    fn is_mutation(&self) -> bool {
        false
    }
}

/// `HVALS key` — array of all values in the hash.
pub struct HvalsCommand;

#[async_trait]
impl Command for HvalsCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'HVALS' command".to_string(),
            );
        }
        let key = match arg_str(args, 0) {
            Ok(k) => k,
            Err(e) => return CommandResult::Error(e),
        };
        match store.hgetall(key).await {
            Ok(fields) => {
                let out: Vec<ResponseValue> = fields
                    .into_iter()
                    .map(|(_, value)| ResponseValue::bulk(value))
                    .collect();
                CommandResult::Ok(ResponseValue::Array(out))
            }
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "HVALS"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2)
    }

    fn is_mutation(&self) -> bool {
        false
    }
}

/// `HLEN key` — integer count of fields in the hash, 0 if missing.
pub struct HlenCommand;

#[async_trait]
impl Command for HlenCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'HLEN' command".to_string(),
            );
        }
        let key = match arg_str(args, 0) {
            Ok(k) => k,
            Err(e) => return CommandResult::Error(e),
        };
        match store.hgetall(key).await {
            Ok(fields) => CommandResult::Ok(ResponseValue::Integer(fields.len() as i64)),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "HLEN"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2)
    }

    fn is_mutation(&self) -> bool {
        false
    }
}

/// HEXISTS command implementation
/// Determines if a hash field exists
pub struct HexistsCommand;

#[async_trait]
impl Command for HexistsCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() != 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'HEXISTS' command".to_string(),
            );
        }

        let key = match arg_str(args, 0) {
            Ok(k) => k,
            Err(e) => return CommandResult::Error(e),
        };
        let field = match arg_str(args, 1) {
            Ok(f) => f,
            Err(e) => return CommandResult::Error(e),
        };

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

    fn is_mutation(&self) -> bool {
        false
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
        let args = vec![
            bytes::Bytes::from_static(b"myhash"),
            bytes::Bytes::from_static(b"field1"),
            bytes::Bytes::from_static(b"value1"),
        ];

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
        let args1 = vec![
            bytes::Bytes::from_static(b"myhash"),
            bytes::Bytes::from_static(b"field1"),
            bytes::Bytes::from_static(b"value1"),
        ];
        let result1 = cmd.execute(&args1, &store).await;
        assert!(matches!(
            result1,
            CommandResult::Ok(ResponseValue::Integer(1))
        ));

        // Update the same field
        let args2 = vec![
            bytes::Bytes::from_static(b"myhash"),
            bytes::Bytes::from_static(b"field1"),
            bytes::Bytes::from_static(b"value2"),
        ];
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

        // Too few arguments (just the key)
        let args = vec![
            bytes::Bytes::from_static(b"myhash"),
            bytes::Bytes::from_static(b"field1"),
        ];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));

        // Unbalanced field/value count (key + 2 fields, only 1 value)
        let args = vec![
            bytes::Bytes::from_static(b"myhash"),
            bytes::Bytes::from_static(b"field1"),
            bytes::Bytes::from_static(b"value1"),
            bytes::Bytes::from_static(b"orphan_field"),
        ];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn test_hset_command_variadic_pairs() {
        // Stage 8: HSET key f1 v1 f2 v2 f3 v3 — returns count of NEW fields.
        let store = create_test_store();
        let cmd = HsetCommand;
        let args = vec![
            bytes::Bytes::from_static(b"h"),
            bytes::Bytes::from_static(b"a"),
            bytes::Bytes::from_static(b"1"),
            bytes::Bytes::from_static(b"b"),
            bytes::Bytes::from_static(b"2"),
            bytes::Bytes::from_static(b"c"),
            bytes::Bytes::from_static(b"3"),
        ];
        let result = cmd.execute(&args, &store).await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(3)));

        assert_eq!(store.hget("h", "a").await.unwrap(), Some("1".to_string()));
        assert_eq!(store.hget("h", "b").await.unwrap(), Some("2".to_string()));
        assert_eq!(store.hget("h", "c").await.unwrap(), Some("3".to_string()));

        // Re-running with one new field and two updates returns 1.
        let args = vec![
            bytes::Bytes::from_static(b"h"),
            bytes::Bytes::from_static(b"a"),
            bytes::Bytes::from_static(b"X"),
            bytes::Bytes::from_static(b"d"),
            bytes::Bytes::from_static(b"4"),
        ];
        let result = cmd.execute(&args, &store).await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(1)));
        assert_eq!(store.hget("h", "a").await.unwrap(), Some("X".to_string()));
    }

    #[test]
    fn test_hset_command_properties() {
        let cmd = HsetCommand;
        assert_eq!(cmd.name(), "HSET");
        // Stage 8: variadic — at least cmd + key + 1 (field, value) pair.
        assert_eq!(cmd.arity(), CommandArity::AtLeast(4));
    }

    // HGET command tests
    #[tokio::test]
    async fn test_hget_command_existing_field() {
        let store = create_test_store();

        // Set up test data
        store.hset("myhash", "field1", "value1").await.unwrap();

        let cmd = HgetCommand;
        let args = vec![
            bytes::Bytes::from_static(b"myhash"),
            bytes::Bytes::from_static(b"field1"),
        ];

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
        let args = vec![
            bytes::Bytes::from_static(b"myhash"),
            bytes::Bytes::from_static(b"field2"),
        ];

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
        let args = vec![
            bytes::Bytes::from_static(b"nonexistent"),
            bytes::Bytes::from_static(b"field1"),
        ];

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
        let args = vec![bytes::Bytes::from_static(b"myhash")];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));

        // Too many arguments
        let args = vec![
            bytes::Bytes::from_static(b"myhash"),
            bytes::Bytes::from_static(b"field1"),
            bytes::Bytes::from_static(b"extra"),
        ];
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
        let args = vec![
            bytes::Bytes::from_static(b"myhash"),
            bytes::Bytes::from_static(b"field1"),
        ];

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
        let args = vec![
            bytes::Bytes::from_static(b"myhash"),
            bytes::Bytes::from_static(b"field1"),
            bytes::Bytes::from_static(b"field2"),
            bytes::Bytes::from_static(b"nonexistent"),
        ];

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
        assert_eq!(
            store.hget("myhash", "field3").await.unwrap(),
            Some("value3".to_string())
        );
    }

    #[tokio::test]
    async fn test_hdel_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = HdelCommand;
        let args = vec![
            bytes::Bytes::from_static(b"nonexistent"),
            bytes::Bytes::from_static(b"field1"),
        ];

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
        let args = vec![bytes::Bytes::from_static(b"myhash")];
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
        let args = vec![bytes::Bytes::from_static(b"myhash")];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(values)) => {
                assert_eq!(values.len(), 4); // 2 fields * 2 (field + value)

                // Convert to HashMap for easier testing (order might vary)
                let mut fields = HashMap::new();
                for chunk in values.chunks(2) {
                    if let (
                        ResponseValue::BulkString(Some(field)),
                        ResponseValue::BulkString(Some(value)),
                    ) = (&chunk[0], &chunk[1])
                    {
                        fields.insert(field.clone(), value.clone());
                    }
                }

                assert_eq!(
                    fields.get(&bytes::Bytes::from("field1")),
                    Some(&bytes::Bytes::from("value1"))
                );
                assert_eq!(
                    fields.get(&bytes::Bytes::from("field2")),
                    Some(&bytes::Bytes::from("value2"))
                );
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
        let args = vec![bytes::Bytes::from_static(b"myhash")];

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
        let args = vec![bytes::Bytes::from_static(b"nonexistent")];

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
        let args = vec![
            bytes::Bytes::from_static(b"myhash"),
            bytes::Bytes::from_static(b"extra"),
        ];
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
        let args = vec![
            bytes::Bytes::from_static(b"myhash"),
            bytes::Bytes::from_static(b"field1"),
        ];

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
        let args = vec![
            bytes::Bytes::from_static(b"myhash"),
            bytes::Bytes::from_static(b"field2"),
        ];

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
        let args = vec![
            bytes::Bytes::from_static(b"nonexistent"),
            bytes::Bytes::from_static(b"field1"),
        ];

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
        let args = vec![bytes::Bytes::from_static(b"myhash")];
        let result = cmd.execute(&args, &store).await;
        assert!(matches!(result, CommandResult::Error(_)));

        // Too many arguments
        let args = vec![
            bytes::Bytes::from_static(b"myhash"),
            bytes::Bytes::from_static(b"field1"),
            bytes::Bytes::from_static(b"extra"),
        ];
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
        let result = hset_cmd
            .execute(
                &[
                    bytes::Bytes::from_static(b"myhash"),
                    bytes::Bytes::from_static(b"name"),
                    bytes::Bytes::from_static(b"John"),
                ],
                &store,
            )
            .await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::Integer(1))
        ));

        let result = hset_cmd
            .execute(
                &[
                    bytes::Bytes::from_static(b"myhash"),
                    bytes::Bytes::from_static(b"age"),
                    bytes::Bytes::from_static(b"30"),
                ],
                &store,
            )
            .await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::Integer(1))
        ));

        // Get individual fields
        let result = hget_cmd
            .execute(
                &[
                    bytes::Bytes::from_static(b"myhash"),
                    bytes::Bytes::from_static(b"name"),
                ],
                &store,
            )
            .await;
        assert!(
            matches!(result, CommandResult::Ok(ResponseValue::BulkString(Some(ref v))) if v == "John")
        );

        // Check field existence
        let result = hexists_cmd
            .execute(
                &[
                    bytes::Bytes::from_static(b"myhash"),
                    bytes::Bytes::from_static(b"name"),
                ],
                &store,
            )
            .await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::Integer(1))
        ));

        let result = hexists_cmd
            .execute(
                &[
                    bytes::Bytes::from_static(b"myhash"),
                    bytes::Bytes::from_static(b"nonexistent"),
                ],
                &store,
            )
            .await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::Integer(0))
        ));

        // Get all fields
        let result = hgetall_cmd
            .execute(&[bytes::Bytes::from_static(b"myhash")], &store)
            .await;
        if let CommandResult::Ok(ResponseValue::Array(values)) = result {
            assert_eq!(values.len(), 4); // 2 fields * 2
        } else {
            panic!("Expected array response from HGETALL");
        }

        // Delete a field
        let result = hdel_cmd
            .execute(
                &[
                    bytes::Bytes::from_static(b"myhash"),
                    bytes::Bytes::from_static(b"age"),
                ],
                &store,
            )
            .await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::Integer(1))
        ));

        // Verify field was deleted
        let result = hget_cmd
            .execute(
                &[
                    bytes::Bytes::from_static(b"myhash"),
                    bytes::Bytes::from_static(b"age"),
                ],
                &store,
            )
            .await;
        assert!(matches!(
            result,
            CommandResult::Ok(ResponseValue::BulkString(None))
        ));

        // Verify other field still exists
        let result = hget_cmd
            .execute(
                &[
                    bytes::Bytes::from_static(b"myhash"),
                    bytes::Bytes::from_static(b"name"),
                ],
                &store,
            )
            .await;
        assert!(
            matches!(result, CommandResult::Ok(ResponseValue::BulkString(Some(ref v))) if v == "John")
        );
    }

    #[tokio::test]
    async fn test_hash_operations_with_type_mismatch() {
        let store = create_test_store();

        // Set a string value first
        store.set("mykey", "string_value").await.unwrap();

        // Try to use hash operations on string key
        let hset_cmd = HsetCommand;
        let result = hset_cmd
            .execute(
                &[
                    bytes::Bytes::from_static(b"mykey"),
                    bytes::Bytes::from_static(b"field1"),
                    bytes::Bytes::from_static(b"value1"),
                ],
                &store,
            )
            .await;
        assert!(matches!(result, CommandResult::Error(_)));

        let hget_cmd = HgetCommand;
        let result = hget_cmd
            .execute(
                &[
                    bytes::Bytes::from_static(b"mykey"),
                    bytes::Bytes::from_static(b"field1"),
                ],
                &store,
            )
            .await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    // HMGET / HKEYS / HVALS / HLEN tests (Stage 8 partial 3)

    #[tokio::test]
    async fn test_hmget_returns_values_with_nil_for_missing() {
        let store = create_test_store();
        store.hset("h", "a", "1").await.unwrap();
        store.hset("h", "b", "2").await.unwrap();

        let cmd = HmgetCommand;
        let args = vec![
            bytes::Bytes::from_static(b"h"),
            bytes::Bytes::from_static(b"a"),
            bytes::Bytes::from_static(b"missing"),
            bytes::Bytes::from_static(b"b"),
        ];
        let result = cmd.execute(&args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Array(items)) => {
                assert_eq!(items.len(), 3);
                assert_eq!(items[0], ResponseValue::bulk("1"));
                assert_eq!(items[1], ResponseValue::nil_bulk());
                assert_eq!(items[2], ResponseValue::bulk("2"));
            }
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_hmget_missing_key_returns_array_of_nils() {
        let store = create_test_store();
        let cmd = HmgetCommand;
        let args = vec![
            bytes::Bytes::from_static(b"missing"),
            bytes::Bytes::from_static(b"f1"),
            bytes::Bytes::from_static(b"f2"),
        ];
        let result = cmd.execute(&args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Array(items)) => {
                assert_eq!(items.len(), 2);
                assert!(matches!(items[0], ResponseValue::BulkString(None)));
                assert!(matches!(items[1], ResponseValue::BulkString(None)));
            }
            other => panic!("expected array of nils, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_hmget_wrong_args_too_few() {
        let store = create_test_store();
        let cmd = HmgetCommand;
        // Just key, no fields
        let args = vec![bytes::Bytes::from_static(b"h")];
        let result = cmd.execute(&args, &store).await;
        match result {
            CommandResult::Error(msg) => assert!(msg.contains("wrong number of arguments")),
            _ => panic!("expected error"),
        }
    }

    #[tokio::test]
    async fn test_hkeys_returns_field_names() {
        let store = create_test_store();
        store.hset("h", "alpha", "1").await.unwrap();
        store.hset("h", "beta", "2").await.unwrap();
        store.hset("h", "gamma", "3").await.unwrap();

        let cmd = HkeysCommand;
        let args = vec![bytes::Bytes::from_static(b"h")];
        let result = cmd.execute(&args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Array(items)) => {
                assert_eq!(items.len(), 3);
                let mut names: Vec<String> = items
                    .iter()
                    .map(|v| match v {
                        ResponseValue::BulkString(Some(b)) => {
                            String::from_utf8_lossy(b).into_owned()
                        }
                        _ => panic!(),
                    })
                    .collect();
                names.sort();
                assert_eq!(names, vec!["alpha", "beta", "gamma"]);
            }
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_hkeys_missing_key_returns_empty_array() {
        let store = create_test_store();
        let cmd = HkeysCommand;
        let args = vec![bytes::Bytes::from_static(b"missing")];
        let result = cmd.execute(&args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Array(items)) => {
                assert!(items.is_empty());
            }
            other => panic!("expected empty array, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_hvals_returns_values() {
        let store = create_test_store();
        store.hset("h", "a", "alpha").await.unwrap();
        store.hset("h", "b", "beta").await.unwrap();

        let cmd = HvalsCommand;
        let args = vec![bytes::Bytes::from_static(b"h")];
        let result = cmd.execute(&args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Array(items)) => {
                assert_eq!(items.len(), 2);
                let mut values: Vec<String> = items
                    .iter()
                    .map(|v| match v {
                        ResponseValue::BulkString(Some(b)) => {
                            String::from_utf8_lossy(b).into_owned()
                        }
                        _ => panic!(),
                    })
                    .collect();
                values.sort();
                assert_eq!(values, vec!["alpha", "beta"]);
            }
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_hlen_returns_field_count() {
        let store = create_test_store();
        store.hset("h", "a", "1").await.unwrap();
        store.hset("h", "b", "2").await.unwrap();
        store.hset("h", "c", "3").await.unwrap();

        let cmd = HlenCommand;
        let args = vec![bytes::Bytes::from_static(b"h")];
        let result = cmd.execute(&args, &store).await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(3)));
    }

    #[tokio::test]
    async fn test_hlen_missing_key_returns_zero() {
        let store = create_test_store();
        let cmd = HlenCommand;
        let args = vec![bytes::Bytes::from_static(b"missing")];
        let result = cmd.execute(&args, &store).await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(0)));
    }

    #[test]
    fn test_new_hash_commands_are_read_only() {
        // Stage 8 partial 3: HMGET/HKEYS/HVALS/HLEN must NOT be logged
        // to the AOF — they're read-only. HSET (variadic) is still a
        // mutation.
        assert!(!HmgetCommand.is_mutation());
        assert!(!HkeysCommand.is_mutation());
        assert!(!HvalsCommand.is_mutation());
        assert!(!HlenCommand.is_mutation());
        assert!(HsetCommand.is_mutation());
    }
}
