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
}
