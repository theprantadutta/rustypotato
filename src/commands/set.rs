//! Set command implementations (SADD, SREM, SMEMBERS, SCARD, SISMEMBER, SPOP, SRANDMEMBER)

use crate::commands::{Command, CommandArity, CommandResult, ResponseValue};
use crate::storage::MemoryStore;
use async_trait::async_trait;
use uuid::Uuid;

/// SADD command implementation
/// Add the specified members to the set stored at key
pub struct SaddCommand;

#[async_trait]
impl Command for SaddCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore, _client_id: Uuid) -> CommandResult {
        if args.len() < 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'SADD' command".to_string(),
            );
        }

        let key = &args[0];
        let members = &args[1..];

        match store.sadd(key.clone(), members).await {
            Ok(added_count) => CommandResult::Ok(ResponseValue::Integer(added_count)),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "SADD"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::AtLeast(3) // SADD key member [member ...]
    }
}

/// SREM command implementation
/// Remove the specified members from the set stored at key
pub struct SremCommand;

#[async_trait]
impl Command for SremCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore, _client_id: Uuid) -> CommandResult {
        if args.len() < 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'SREM' command".to_string(),
            );
        }

        let key = &args[0];
        let members = &args[1..];

        match store.srem(key.clone(), members).await {
            Ok(removed_count) => CommandResult::Ok(ResponseValue::Integer(removed_count)),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "SREM"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::AtLeast(3) // SREM key member [member ...]
    }
}

/// SMEMBERS command implementation
/// Returns all the members of the set value stored at key
pub struct SmembersCommand;

#[async_trait]
impl Command for SmembersCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore, _client_id: Uuid) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'SMEMBERS' command".to_string(),
            );
        }

        let key = &args[0];

        match store.smembers(key) {
            Ok(members) => {
                let array = members
                    .into_iter()
                    .map(|m| ResponseValue::BulkString(Some(m)))
                    .collect();
                CommandResult::Ok(ResponseValue::Array(array))
            }
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "SMEMBERS"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2) // SMEMBERS key
    }
}

/// SCARD command implementation
/// Returns the set cardinality (number of elements) of the set stored at key
pub struct ScardCommand;

#[async_trait]
impl Command for ScardCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore, _client_id: Uuid) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'SCARD' command".to_string(),
            );
        }

        let key = &args[0];

        match store.scard(key) {
            Ok(count) => CommandResult::Ok(ResponseValue::Integer(count)),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "SCARD"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2) // SCARD key
    }
}

/// SISMEMBER command implementation
/// Returns if member is a member of the set stored at key
pub struct SismemberCommand;

#[async_trait]
impl Command for SismemberCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore, _client_id: Uuid) -> CommandResult {
        if args.len() != 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'SISMEMBER' command".to_string(),
            );
        }

        let key = &args[0];
        let member = &args[1];

        match store.sismember(key, member) {
            Ok(is_member) => CommandResult::Ok(ResponseValue::Integer(if is_member { 1 } else { 0 })),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "SISMEMBER"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(3) // SISMEMBER key member
    }
}

/// SPOP command implementation
/// Removes and returns one or more random members from the set value stored at key
pub struct SpopCommand;

#[async_trait]
impl Command for SpopCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore, _client_id: Uuid) -> CommandResult {
        if args.is_empty() || args.len() > 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'SPOP' command".to_string(),
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

        match store.spop(key, count).await {
            Ok(Some(members)) => {
                if count.is_some() {
                    // Return array when count is specified
                    let array = members
                        .into_iter()
                        .map(|m| ResponseValue::BulkString(Some(m)))
                        .collect();
                    CommandResult::Ok(ResponseValue::Array(array))
                } else {
                    // Return single value when no count
                    CommandResult::Ok(ResponseValue::BulkString(members.into_iter().next()))
                }
            }
            Ok(None) => {
                if count.is_some() {
                    // Return empty array when count specified but set doesn't exist
                    CommandResult::Ok(ResponseValue::Array(vec![]))
                } else {
                    CommandResult::Ok(ResponseValue::BulkString(None))
                }
            }
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "SPOP"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Range(2, 3) // SPOP key [count]
    }
}

/// SRANDMEMBER command implementation
/// Returns one or more random members from the set value stored at key
/// With positive count: returns distinct members
/// With negative count: allows duplicates
pub struct SrandmemberCommand;

#[async_trait]
impl Command for SrandmemberCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore, _client_id: Uuid) -> CommandResult {
        if args.is_empty() || args.len() > 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'SRANDMEMBER' command".to_string(),
            );
        }

        let key = &args[0];
        let count = if args.len() == 2 {
            match args[1].parse::<i64>() {
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

        match store.srandmember(key, count) {
            Ok(Some(members)) => {
                if count.is_some() {
                    // Return array when count is specified
                    let array = members
                        .into_iter()
                        .map(|m| ResponseValue::BulkString(Some(m)))
                        .collect();
                    CommandResult::Ok(ResponseValue::Array(array))
                } else {
                    // Return single value when no count
                    CommandResult::Ok(ResponseValue::BulkString(members.into_iter().next()))
                }
            }
            Ok(None) => {
                if count.is_some() {
                    // Return empty array when count specified but set doesn't exist
                    CommandResult::Ok(ResponseValue::Array(vec![]))
                } else {
                    CommandResult::Ok(ResponseValue::BulkString(None))
                }
            }
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "SRANDMEMBER"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Range(2, 3) // SRANDMEMBER key [count]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStore;
    use uuid::Uuid;

    fn create_test_store() -> MemoryStore {
        MemoryStore::new()
    }

    fn test_client_id() -> Uuid {
        Uuid::new_v4()
    }

    // SADD command tests
    #[tokio::test]
    async fn test_sadd_command_new_members() {
        let store = create_test_store();
        let cmd = SaddCommand;
        let args = vec!["myset".to_string(), "a".to_string(), "b".to_string(), "c".to_string()];

        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 3); // All 3 members are new
            }
            _ => panic!("Expected integer response from SADD command"),
        }
    }

    #[tokio::test]
    async fn test_sadd_command_duplicate_members() {
        let store = create_test_store();
        let cmd = SaddCommand;

        // Add initial members
        let args1 = vec!["myset".to_string(), "a".to_string(), "b".to_string()];
        cmd.execute(&args1, &store, test_client_id()).await;

        // Add some duplicates and one new
        let args2 = vec!["myset".to_string(), "a".to_string(), "c".to_string()];
        let result = cmd.execute(&args2, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 1); // Only "c" is new
            }
            _ => panic!("Expected integer response from SADD command"),
        }
    }

    #[tokio::test]
    async fn test_sadd_command_wrong_args() {
        let store = create_test_store();
        let cmd = SaddCommand;

        // Too few arguments
        let args = vec!["myset".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[test]
    fn test_sadd_command_properties() {
        let cmd = SaddCommand;
        assert_eq!(cmd.name(), "SADD");
        assert_eq!(cmd.arity(), CommandArity::AtLeast(3));
    }

    // SREM command tests
    #[tokio::test]
    async fn test_srem_command_existing_members() {
        let store = create_test_store();

        // Set up test data
        store.sadd("myset".to_string(), &["a".to_string(), "b".to_string(), "c".to_string()]).await.unwrap();

        let cmd = SremCommand;
        let args = vec!["myset".to_string(), "a".to_string(), "b".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 2);
            }
            _ => panic!("Expected integer response from SREM command"),
        }
    }

    #[tokio::test]
    async fn test_srem_command_nonexistent_members() {
        let store = create_test_store();

        store.sadd("myset".to_string(), &["a".to_string()]).await.unwrap();

        let cmd = SremCommand;
        let args = vec!["myset".to_string(), "x".to_string(), "y".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 0);
            }
            _ => panic!("Expected integer response from SREM command"),
        }
    }

    #[tokio::test]
    async fn test_srem_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = SremCommand;
        let args = vec!["nonexistent".to_string(), "a".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 0);
            }
            _ => panic!("Expected integer response from SREM command"),
        }
    }

    #[test]
    fn test_srem_command_properties() {
        let cmd = SremCommand;
        assert_eq!(cmd.name(), "SREM");
        assert_eq!(cmd.arity(), CommandArity::AtLeast(3));
    }

    // SMEMBERS command tests
    #[tokio::test]
    async fn test_smembers_command() {
        let store = create_test_store();

        store.sadd("myset".to_string(), &["a".to_string(), "b".to_string(), "c".to_string()]).await.unwrap();

        let cmd = SmembersCommand;
        let args = vec!["myset".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(members)) => {
                assert_eq!(members.len(), 3);
            }
            _ => panic!("Expected array response from SMEMBERS command"),
        }
    }

    #[tokio::test]
    async fn test_smembers_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = SmembersCommand;
        let args = vec!["nonexistent".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(members)) => {
                assert_eq!(members.len(), 0);
            }
            _ => panic!("Expected empty array from SMEMBERS command"),
        }
    }

    #[test]
    fn test_smembers_command_properties() {
        let cmd = SmembersCommand;
        assert_eq!(cmd.name(), "SMEMBERS");
        assert_eq!(cmd.arity(), CommandArity::Fixed(2));
    }

    // SCARD command tests
    #[tokio::test]
    async fn test_scard_command() {
        let store = create_test_store();

        store.sadd("myset".to_string(), &["a".to_string(), "b".to_string(), "c".to_string()]).await.unwrap();

        let cmd = ScardCommand;
        let args = vec!["myset".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 3);
            }
            _ => panic!("Expected integer response from SCARD command"),
        }
    }

    #[tokio::test]
    async fn test_scard_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = ScardCommand;
        let args = vec!["nonexistent".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 0);
            }
            _ => panic!("Expected 0 from SCARD for nonexistent key"),
        }
    }

    #[test]
    fn test_scard_command_properties() {
        let cmd = ScardCommand;
        assert_eq!(cmd.name(), "SCARD");
        assert_eq!(cmd.arity(), CommandArity::Fixed(2));
    }

    // SISMEMBER command tests
    #[tokio::test]
    async fn test_sismember_command_exists() {
        let store = create_test_store();

        store.sadd("myset".to_string(), &["a".to_string(), "b".to_string()]).await.unwrap();

        let cmd = SismemberCommand;
        let args = vec!["myset".to_string(), "a".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(is_member)) => {
                assert_eq!(is_member, 1);
            }
            _ => panic!("Expected integer response from SISMEMBER command"),
        }
    }

    #[tokio::test]
    async fn test_sismember_command_not_exists() {
        let store = create_test_store();

        store.sadd("myset".to_string(), &["a".to_string()]).await.unwrap();

        let cmd = SismemberCommand;
        let args = vec!["myset".to_string(), "x".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(is_member)) => {
                assert_eq!(is_member, 0);
            }
            _ => panic!("Expected integer response from SISMEMBER command"),
        }
    }

    #[tokio::test]
    async fn test_sismember_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = SismemberCommand;
        let args = vec!["nonexistent".to_string(), "a".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(is_member)) => {
                assert_eq!(is_member, 0);
            }
            _ => panic!("Expected 0 for nonexistent key"),
        }
    }

    #[test]
    fn test_sismember_command_properties() {
        let cmd = SismemberCommand;
        assert_eq!(cmd.name(), "SISMEMBER");
        assert_eq!(cmd.arity(), CommandArity::Fixed(3));
    }

    // SPOP command tests
    #[tokio::test]
    async fn test_spop_command_single() {
        let store = create_test_store();

        store.sadd("myset".to_string(), &["a".to_string(), "b".to_string(), "c".to_string()]).await.unwrap();

        let cmd = SpopCommand;
        let args = vec!["myset".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(Some(_))) => {
                // Member was popped
                assert_eq!(store.scard("myset").unwrap(), 2);
            }
            _ => panic!("Expected bulk string response from SPOP command"),
        }
    }

    #[tokio::test]
    async fn test_spop_command_with_count() {
        let store = create_test_store();

        store.sadd("myset".to_string(), &["a".to_string(), "b".to_string(), "c".to_string()]).await.unwrap();

        let cmd = SpopCommand;
        let args = vec!["myset".to_string(), "2".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(members)) => {
                assert_eq!(members.len(), 2);
                assert_eq!(store.scard("myset").unwrap(), 1);
            }
            _ => panic!("Expected array response from SPOP with count"),
        }
    }

    #[tokio::test]
    async fn test_spop_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = SpopCommand;
        let args = vec!["nonexistent".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(None)) => {
                // Expected nil
            }
            _ => panic!("Expected nil for nonexistent key"),
        }
    }

    #[test]
    fn test_spop_command_properties() {
        let cmd = SpopCommand;
        assert_eq!(cmd.name(), "SPOP");
        assert_eq!(cmd.arity(), CommandArity::Range(2, 3));
    }

    // SRANDMEMBER command tests
    #[tokio::test]
    async fn test_srandmember_command_single() {
        let store = create_test_store();

        store.sadd("myset".to_string(), &["a".to_string(), "b".to_string(), "c".to_string()]).await.unwrap();

        let cmd = SrandmemberCommand;
        let args = vec!["myset".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(Some(_))) => {
                // Member was returned but not removed
                assert_eq!(store.scard("myset").unwrap(), 3);
            }
            _ => panic!("Expected bulk string response from SRANDMEMBER command"),
        }
    }

    #[tokio::test]
    async fn test_srandmember_command_with_positive_count() {
        let store = create_test_store();

        store.sadd("myset".to_string(), &["a".to_string(), "b".to_string(), "c".to_string()]).await.unwrap();

        let cmd = SrandmemberCommand;
        let args = vec!["myset".to_string(), "2".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(members)) => {
                assert_eq!(members.len(), 2);
                // Set should still have all 3 members
                assert_eq!(store.scard("myset").unwrap(), 3);
            }
            _ => panic!("Expected array response from SRANDMEMBER with count"),
        }
    }

    #[tokio::test]
    async fn test_srandmember_command_with_negative_count() {
        let store = create_test_store();

        store.sadd("myset".to_string(), &["a".to_string(), "b".to_string()]).await.unwrap();

        let cmd = SrandmemberCommand;
        let args = vec!["myset".to_string(), "-5".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(members)) => {
                // Negative count allows duplicates, so we can have more elements than set size
                assert_eq!(members.len(), 5);
            }
            _ => panic!("Expected array response from SRANDMEMBER with negative count"),
        }
    }

    #[tokio::test]
    async fn test_srandmember_command_nonexistent_key() {
        let store = create_test_store();
        let cmd = SrandmemberCommand;
        let args = vec!["nonexistent".to_string()];
        let result = cmd.execute(&args, &store, test_client_id()).await;

        match result {
            CommandResult::Ok(ResponseValue::BulkString(None)) => {
                // Expected nil
            }
            _ => panic!("Expected nil for nonexistent key"),
        }
    }

    #[test]
    fn test_srandmember_command_properties() {
        let cmd = SrandmemberCommand;
        assert_eq!(cmd.name(), "SRANDMEMBER");
        assert_eq!(cmd.arity(), CommandArity::Range(2, 3));
    }

    // Type mismatch tests
    #[tokio::test]
    async fn test_set_operations_with_type_mismatch() {
        let store = create_test_store();

        // Set a string value first
        store.set("mykey", "string_value").await.unwrap();

        // Try SADD on string key
        let sadd_cmd = SaddCommand;
        let result = sadd_cmd.execute(&["mykey".to_string(), "a".to_string()], &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(ref s) if s.contains("WRONGTYPE")));

        // Try SREM on string key
        let srem_cmd = SremCommand;
        let result = srem_cmd.execute(&["mykey".to_string(), "a".to_string()], &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(ref s) if s.contains("WRONGTYPE")));

        // Try SMEMBERS on string key
        let smembers_cmd = SmembersCommand;
        let result = smembers_cmd.execute(&["mykey".to_string()], &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(ref s) if s.contains("WRONGTYPE")));

        // Try SCARD on string key
        let scard_cmd = ScardCommand;
        let result = scard_cmd.execute(&["mykey".to_string()], &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(ref s) if s.contains("WRONGTYPE")));

        // Try SISMEMBER on string key
        let sismember_cmd = SismemberCommand;
        let result = sismember_cmd.execute(&["mykey".to_string(), "a".to_string()], &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(ref s) if s.contains("WRONGTYPE")));

        // Try SPOP on string key
        let spop_cmd = SpopCommand;
        let result = spop_cmd.execute(&["mykey".to_string()], &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(ref s) if s.contains("WRONGTYPE")));

        // Try SRANDMEMBER on string key
        let srandmember_cmd = SrandmemberCommand;
        let result = srandmember_cmd.execute(&["mykey".to_string()], &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Error(ref s) if s.contains("WRONGTYPE")));
    }

    // Integration test
    #[tokio::test]
    async fn test_set_operations_integration() {
        let store = create_test_store();

        let sadd_cmd = SaddCommand;
        let srem_cmd = SremCommand;
        let smembers_cmd = SmembersCommand;
        let scard_cmd = ScardCommand;
        let sismember_cmd = SismemberCommand;

        // Add members
        let result = sadd_cmd.execute(&["myset".to_string(), "a".to_string(), "b".to_string(), "c".to_string()], &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(3))));

        // Check cardinality
        let result = scard_cmd.execute(&["myset".to_string()], &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(3))));

        // Check membership
        let result = sismember_cmd.execute(&["myset".to_string(), "a".to_string()], &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        let result = sismember_cmd.execute(&["myset".to_string(), "x".to_string()], &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(0))));

        // Get all members
        let result = smembers_cmd.execute(&["myset".to_string()], &store, test_client_id()).await;
        if let CommandResult::Ok(ResponseValue::Array(members)) = result {
            assert_eq!(members.len(), 3);
        } else {
            panic!("Expected array response from SMEMBERS");
        }

        // Remove member
        let result = srem_cmd.execute(&["myset".to_string(), "b".to_string()], &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(1))));

        // Verify removal
        let result = scard_cmd.execute(&["myset".to_string()], &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(2))));

        let result = sismember_cmd.execute(&["myset".to_string(), "b".to_string()], &store, test_client_id()).await;
        assert!(matches!(result, CommandResult::Ok(ResponseValue::Integer(0))));
    }
}
