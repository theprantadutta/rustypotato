//! Server-level commands that don't touch storage state directly:
//! PING, ECHO, and (in subsequent stages) COMMAND, INFO, DBSIZE,
//! TYPE, FLUSHDB, QUIT, KEYS.

use crate::commands::{Command, CommandArity, CommandResult, ResponseValue};
use crate::storage::MemoryStore;
use async_trait::async_trait;

/// `PING [message]` — health probe.
/// - 0 args: returns simple string `+PONG\r\n`.
/// - 1 arg: echoes the message as a bulk string.
///
/// Most clients (including `redis-cli`'s connection handshake) probe
/// with PING before sending any other command. Without it the
/// server looks broken even when it's healthy.
pub struct PingCommand;

#[async_trait]
impl Command for PingCommand {
    async fn execute(&self, args: &[String], _store: &MemoryStore) -> CommandResult {
        match args.len() {
            0 => CommandResult::Ok(ResponseValue::SimpleString("PONG".to_string())),
            1 => CommandResult::Ok(ResponseValue::bulk(args[0].clone())),
            _ => {
                CommandResult::Error("ERR wrong number of arguments for 'PING' command".to_string())
            }
        }
    }

    fn name(&self) -> &'static str {
        "PING"
    }

    fn arity(&self) -> CommandArity {
        // 1 (cmd) + optional message
        CommandArity::Range(1, 2)
    }

    fn is_mutation(&self) -> bool {
        false
    }
}

/// `ECHO message` — returns the message verbatim.
///
/// Used by clients to verify connectivity and round-trip behaviour.
/// Trivial implementation; included now because adding it later would
/// be the only reason to do another commit pass over the registry.
pub struct EchoCommand;

#[async_trait]
impl Command for EchoCommand {
    async fn execute(&self, args: &[String], _store: &MemoryStore) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'ECHO' command".to_string(),
            );
        }
        CommandResult::Ok(ResponseValue::bulk(args[0].clone()))
    }

    fn name(&self) -> &'static str {
        "ECHO"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2) // ECHO message
    }

    fn is_mutation(&self) -> bool {
        false
    }
}

/// `DBSIZE` — return the number of keys currently in the store.
///
/// Includes keys past their TTL — the count is computed cheaply from
/// `MemoryStore::len()` rather than walking the index to filter
/// expired entries. Real Redis behaves similarly: DBSIZE is "good
/// enough" but the active-expiration sweep eventually corrects drift.
pub struct DbsizeCommand;

#[async_trait]
impl Command for DbsizeCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if !args.is_empty() {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'DBSIZE' command".to_string(),
            );
        }
        CommandResult::Ok(ResponseValue::Integer(store.len() as i64))
    }

    fn name(&self) -> &'static str {
        "DBSIZE"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(1) // DBSIZE
    }

    fn is_mutation(&self) -> bool {
        false
    }
}

/// `TYPE key` — return the type of the value stored at `key`.
///
/// `+string` / `+integer` / `+hash` / `+list` / `+none` (Redis itself
/// returns `+none` when the key doesn't exist). The variant names
/// come straight from `ValueType::type_name`.
pub struct TypeCommand;

#[async_trait]
impl Command for TypeCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'TYPE' command".to_string(),
            );
        }
        let kind = match store.get(&args[0]) {
            Ok(Some(stored)) => stored.value.type_name().to_string(),
            Ok(None) => "none".to_string(),
            Err(e) => return CommandResult::Error(e.to_client_error()),
        };
        CommandResult::Ok(ResponseValue::SimpleString(kind))
    }

    fn name(&self) -> &'static str {
        "TYPE"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2) // TYPE key
    }

    fn is_mutation(&self) -> bool {
        false
    }
}

/// `FLUSHDB` — drop every key in the store.
///
/// Mutating, so it IS recorded to the AOF (replay reproduces the
/// flush deterministically by clearing again).
pub struct FlushdbCommand;

#[async_trait]
impl Command for FlushdbCommand {
    async fn execute(&self, args: &[String], store: &MemoryStore) -> CommandResult {
        // Real Redis accepts a single optional `ASYNC` / `SYNC` token;
        // we don't async-flush so we just reject unknown args.
        match args.len() {
            0 => {
                store.clear();
                CommandResult::Ok(ResponseValue::SimpleString("OK".to_string()))
            }
            1 if args[0].eq_ignore_ascii_case("SYNC") || args[0].eq_ignore_ascii_case("ASYNC") => {
                store.clear();
                CommandResult::Ok(ResponseValue::SimpleString("OK".to_string()))
            }
            _ => CommandResult::Error("ERR syntax error".to_string()),
        }
    }

    fn name(&self) -> &'static str {
        "FLUSHDB"
    }

    fn arity(&self) -> CommandArity {
        // FLUSHDB or FLUSHDB SYNC|ASYNC
        CommandArity::Range(1, 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStore;

    #[tokio::test]
    async fn ping_no_args_returns_pong_simple_string() {
        let store = MemoryStore::new();
        let result = PingCommand.execute(&[], &store).await;
        assert_eq!(
            result,
            CommandResult::Ok(ResponseValue::SimpleString("PONG".to_string()))
        );
    }

    #[tokio::test]
    async fn ping_with_message_echoes_bulk_string() {
        let store = MemoryStore::new();
        let result = PingCommand
            .execute(&["hello world".to_string()], &store)
            .await;
        assert_eq!(
            result,
            CommandResult::Ok(ResponseValue::bulk("hello world"))
        );
    }

    #[tokio::test]
    async fn ping_too_many_args_is_an_error() {
        let store = MemoryStore::new();
        let result = PingCommand
            .execute(&["a".to_string(), "b".to_string()], &store)
            .await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn echo_returns_message_as_bulk_string() {
        let store = MemoryStore::new();
        let result = EchoCommand.execute(&["hi there".to_string()], &store).await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::bulk("hi there")));
    }

    #[tokio::test]
    async fn echo_zero_or_two_args_errors() {
        let store = MemoryStore::new();
        assert!(matches!(
            EchoCommand.execute(&[], &store).await,
            CommandResult::Error(_)
        ));
        assert!(matches!(
            EchoCommand
                .execute(&["a".to_string(), "b".to_string()], &store)
                .await,
            CommandResult::Error(_)
        ));
    }

    #[tokio::test]
    async fn ping_and_echo_are_not_mutations() {
        assert!(!PingCommand.is_mutation());
        assert!(!EchoCommand.is_mutation());
    }

    #[tokio::test]
    async fn dbsize_returns_zero_on_empty_store() {
        let store = MemoryStore::new();
        let r = DbsizeCommand.execute(&[], &store).await;
        assert_eq!(r, CommandResult::Ok(ResponseValue::Integer(0)));
    }

    #[tokio::test]
    async fn dbsize_counts_keys_after_inserts() {
        let store = MemoryStore::new();
        store.set("a", "1").await.unwrap();
        store.set("b", "2").await.unwrap();
        store.set("c", "3").await.unwrap();
        let r = DbsizeCommand.execute(&[], &store).await;
        assert_eq!(r, CommandResult::Ok(ResponseValue::Integer(3)));
    }

    #[tokio::test]
    async fn dbsize_with_args_errors() {
        let store = MemoryStore::new();
        assert!(matches!(
            DbsizeCommand.execute(&["x".to_string()], &store).await,
            CommandResult::Error(_)
        ));
    }

    #[tokio::test]
    async fn type_returns_none_for_missing_key() {
        let store = MemoryStore::new();
        let r = TypeCommand.execute(&["nope".to_string()], &store).await;
        assert_eq!(
            r,
            CommandResult::Ok(ResponseValue::SimpleString("none".to_string()))
        );
    }

    #[tokio::test]
    async fn type_distinguishes_string_integer_hash() {
        let store = MemoryStore::new();
        store.set("s", "hi").await.unwrap();
        store.set("i", 7i64).await.unwrap();
        store.hset("h", "f", "v").await.unwrap();

        let s = TypeCommand.execute(&["s".to_string()], &store).await;
        let i = TypeCommand.execute(&["i".to_string()], &store).await;
        let h = TypeCommand.execute(&["h".to_string()], &store).await;

        assert_eq!(
            s,
            CommandResult::Ok(ResponseValue::SimpleString("string".to_string()))
        );
        assert_eq!(
            i,
            CommandResult::Ok(ResponseValue::SimpleString("integer".to_string()))
        );
        assert_eq!(
            h,
            CommandResult::Ok(ResponseValue::SimpleString("hash".to_string()))
        );
    }

    #[tokio::test]
    async fn flushdb_clears_store() {
        let store = MemoryStore::new();
        store.set("k1", "v1").await.unwrap();
        store.set("k2", "v2").await.unwrap();
        assert_eq!(store.len(), 2);

        let r = FlushdbCommand.execute(&[], &store).await;
        assert_eq!(
            r,
            CommandResult::Ok(ResponseValue::SimpleString("OK".to_string()))
        );
        assert_eq!(store.len(), 0);
    }

    #[tokio::test]
    async fn flushdb_accepts_sync_and_async_modifier() {
        let store = MemoryStore::new();
        store.set("k", "v").await.unwrap();
        let r = FlushdbCommand.execute(&["SYNC".to_string()], &store).await;
        assert!(matches!(r, CommandResult::Ok(_)));
        store.set("k", "v").await.unwrap();
        let r = FlushdbCommand.execute(&["async".to_string()], &store).await;
        assert!(matches!(r, CommandResult::Ok(_)));
    }

    #[tokio::test]
    async fn flushdb_rejects_unknown_modifier() {
        let store = MemoryStore::new();
        let r = FlushdbCommand.execute(&["NUKE".to_string()], &store).await;
        assert!(matches!(r, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn flushdb_is_a_mutation_dbsize_type_are_not() {
        assert!(FlushdbCommand.is_mutation());
        assert!(!DbsizeCommand.is_mutation());
        assert!(!TypeCommand.is_mutation());
    }
}
