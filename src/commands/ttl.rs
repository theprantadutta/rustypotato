//! TTL command implementations
//! (EXPIRE, TTL, PTTL, PEXPIRE, EXPIREAT, PEXPIREAT)

use crate::commands::registry::Args;
use crate::commands::{arg_str, Command, CommandArity, CommandResult, ResponseValue};
use crate::storage::{ExpireFlag, MemoryStore};
use async_trait::async_trait;
use bytes::Bytes;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// EXPIRE command implementation
///
/// Supports the full Redis 7 syntax: `EXPIRE key seconds [NX | XX | GT | LT]`.
/// Flags are mutually exclusive — combining them yields a syntax error.
pub struct ExpireCommand;

fn parse_expire_flag(args: &[Bytes]) -> std::result::Result<ExpireFlag, String> {
    if args.is_empty() {
        return Ok(ExpireFlag::None);
    }
    if args.len() > 1 {
        return Err("ERR syntax error".to_string());
    }
    let token = std::str::from_utf8(&args[0]).map_err(|_| "ERR syntax error".to_string())?;
    match token.to_ascii_uppercase().as_str() {
        "NX" => Ok(ExpireFlag::Nx),
        "XX" => Ok(ExpireFlag::Xx),
        "GT" => Ok(ExpireFlag::Gt),
        "LT" => Ok(ExpireFlag::Lt),
        _ => Err("ERR syntax error".to_string()),
    }
}

#[async_trait]
impl Command for ExpireCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() < 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'EXPIRE' command".to_string(),
            );
        }

        let key = match arg_str(args, 0) {
            Ok(k) => k,
            Err(e) => return CommandResult::Error(e),
        };
        let ttl_str = match arg_str(args, 1) {
            Ok(s) => s,
            Err(_) => {
                return CommandResult::Error(
                    "ERR value is not an integer or out of range".to_string(),
                )
            }
        };

        // Parse TTL seconds
        let ttl_seconds = match ttl_str.parse::<u64>() {
            Ok(seconds) => seconds,
            Err(_) => {
                return CommandResult::Error(
                    "ERR value is not an integer or out of range".to_string(),
                );
            }
        };

        let flag = match parse_expire_flag(&args[2..]) {
            Ok(f) => f,
            Err(e) => return CommandResult::Error(e),
        };

        match store.expire_with_options(key, ttl_seconds, flag).await {
            Ok(true) => CommandResult::Ok(ResponseValue::Integer(1)),
            Ok(false) => CommandResult::Ok(ResponseValue::Integer(0)),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "EXPIRE"
    }

    fn arity(&self) -> CommandArity {
        // EXPIRE key seconds [NX|XX|GT|LT]
        CommandArity::Range(3, 4)
    }
}

/// `PEXPIRE key milliseconds [NX | XX | GT | LT]` — like EXPIRE but
/// the deadline is specified in milliseconds.
pub struct PexpireCommand;

#[async_trait]
impl Command for PexpireCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() < 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'PEXPIRE' command".to_string(),
            );
        }

        let key = match arg_str(args, 0) {
            Ok(k) => k,
            Err(e) => return CommandResult::Error(e),
        };
        let ms_str = match arg_str(args, 1) {
            Ok(s) => s,
            Err(_) => {
                return CommandResult::Error(
                    "ERR value is not an integer or out of range".to_string(),
                )
            }
        };
        let millis = match ms_str.parse::<u64>() {
            Ok(n) => n,
            Err(_) => {
                return CommandResult::Error(
                    "ERR value is not an integer or out of range".to_string(),
                )
            }
        };
        let flag = match parse_expire_flag(&args[2..]) {
            Ok(f) => f,
            Err(e) => return CommandResult::Error(e),
        };

        let deadline = Instant::now() + Duration::from_millis(millis);
        match store.expire_at_with_options(key, deadline, flag).await {
            Ok(true) => CommandResult::Ok(ResponseValue::Integer(1)),
            Ok(false) => CommandResult::Ok(ResponseValue::Integer(0)),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "PEXPIRE"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Range(3, 4)
    }
}

/// Resolve a wall-clock target (seconds or milliseconds since the
/// Unix epoch) into a monotonic `Instant`. Targets in the past
/// produce an instant 1ms in the past, which immediately reads as
/// expired (matches Redis: `EXPIREAT key 0` deletes immediately).
fn resolve_unix_target(target_ms: u64) -> std::result::Result<Instant, String> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "ERR system clock error".to_string())?
        .as_millis() as u64;
    if target_ms <= now_ms {
        return Ok(Instant::now() - Duration::from_millis(1));
    }
    let delta = Duration::from_millis(target_ms - now_ms);
    Ok(Instant::now() + delta)
}

/// `EXPIREAT key unix-time-seconds [NX | XX | GT | LT]` — set TTL
/// using an absolute Unix timestamp in seconds.
pub struct ExpireatCommand;

#[async_trait]
impl Command for ExpireatCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() < 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'EXPIREAT' command".to_string(),
            );
        }
        let key = match arg_str(args, 0) {
            Ok(k) => k,
            Err(e) => return CommandResult::Error(e),
        };
        let secs_str = match arg_str(args, 1) {
            Ok(s) => s,
            Err(_) => {
                return CommandResult::Error(
                    "ERR value is not an integer or out of range".to_string(),
                )
            }
        };
        let secs = match secs_str.parse::<u64>() {
            Ok(n) => n,
            Err(_) => {
                return CommandResult::Error(
                    "ERR value is not an integer or out of range".to_string(),
                )
            }
        };
        let flag = match parse_expire_flag(&args[2..]) {
            Ok(f) => f,
            Err(e) => return CommandResult::Error(e),
        };
        let target_ms = secs.saturating_mul(1000);
        let deadline = match resolve_unix_target(target_ms) {
            Ok(d) => d,
            Err(e) => return CommandResult::Error(e),
        };
        match store.expire_at_with_options(key, deadline, flag).await {
            Ok(true) => CommandResult::Ok(ResponseValue::Integer(1)),
            Ok(false) => CommandResult::Ok(ResponseValue::Integer(0)),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "EXPIREAT"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Range(3, 4)
    }
}

/// `PEXPIREAT key unix-time-milliseconds [NX | XX | GT | LT]` — set
/// TTL using an absolute Unix timestamp in milliseconds.
pub struct PexpireatCommand;

#[async_trait]
impl Command for PexpireatCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() < 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'PEXPIREAT' command".to_string(),
            );
        }
        let key = match arg_str(args, 0) {
            Ok(k) => k,
            Err(e) => return CommandResult::Error(e),
        };
        let ms_str = match arg_str(args, 1) {
            Ok(s) => s,
            Err(_) => {
                return CommandResult::Error(
                    "ERR value is not an integer or out of range".to_string(),
                )
            }
        };
        let target_ms = match ms_str.parse::<u64>() {
            Ok(n) => n,
            Err(_) => {
                return CommandResult::Error(
                    "ERR value is not an integer or out of range".to_string(),
                )
            }
        };
        let flag = match parse_expire_flag(&args[2..]) {
            Ok(f) => f,
            Err(e) => return CommandResult::Error(e),
        };
        let deadline = match resolve_unix_target(target_ms) {
            Ok(d) => d,
            Err(e) => return CommandResult::Error(e),
        };
        match store.expire_at_with_options(key, deadline, flag).await {
            Ok(true) => CommandResult::Ok(ResponseValue::Integer(1)),
            Ok(false) => CommandResult::Ok(ResponseValue::Integer(0)),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "PEXPIREAT"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Range(3, 4)
    }
}

/// TTL command implementation
/// Returns the remaining time to live of a key that has a timeout.
pub struct TtlCommand;

#[async_trait]
impl Command for TtlCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'TTL' command".to_string(),
            );
        }

        let key = match arg_str(args, 0) {
            Ok(k) => k,
            Err(e) => return CommandResult::Error(e),
        };

        match store.ttl(key) {
            Ok(ttl) => CommandResult::Ok(ResponseValue::Integer(ttl)),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "TTL"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2) // TTL key
    }

    fn is_mutation(&self) -> bool {
        false
    }
}

/// `PTTL key` — like TTL but returns the remaining time in
/// milliseconds. Same special return values: -2 (no key), -1 (no
/// expiration).
pub struct PttlCommand;

#[async_trait]
impl Command for PttlCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'PTTL' command".to_string(),
            );
        }
        let key = match arg_str(args, 0) {
            Ok(k) => k,
            Err(e) => return CommandResult::Error(e),
        };
        match store.pttl(key) {
            Ok(ms) => CommandResult::Ok(ResponseValue::Integer(ms)),
            Err(e) => CommandResult::Error(e.to_client_error()),
        }
    }

    fn name(&self) -> &'static str {
        "PTTL"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2)
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
    use std::time::Duration;

    // Helper function to create a test store
    fn create_test_store() -> MemoryStore {
        MemoryStore::new()
    }

    // EXPIRE command tests
    #[tokio::test]
    async fn test_expire_command_existing_key() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();

        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"test_key"),
            bytes::Bytes::from_static(b"60"),
        ];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(1)) => {
                // Expected: key exists and expiration was set
            }
            _ => panic!("Expected integer 1 for successful EXPIRE on existing key"),
        }

        // Verify TTL was set
        let ttl = store.ttl("test_key").unwrap();
        assert!(
            ttl > 0 && ttl <= 60,
            "Expected TTL between 1 and 60, got {ttl}"
        );
    }

    #[tokio::test]
    async fn test_expire_command_nonexistent_key() {
        let store = create_test_store();

        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"nonexistent_key"),
            bytes::Bytes::from_static(b"60"),
        ];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(0)) => {
                // Expected: key doesn't exist
            }
            _ => panic!("Expected integer 0 for EXPIRE on nonexistent key"),
        }
    }

    #[tokio::test]
    async fn test_expire_command_zero_ttl() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();

        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"test_key"),
            bytes::Bytes::from_static(b"0"),
        ];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(1)) => {
                // Expected: key exists and expiration was set (immediate expiration)
            }
            _ => panic!("Expected integer 1 for EXPIRE with 0 TTL"),
        }

        // Key should be expired immediately or very soon
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(
            !store.exists("test_key").unwrap(),
            "Key should be expired with 0 TTL"
        );
    }

    #[tokio::test]
    async fn test_expire_command_large_ttl() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();

        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"test_key"),
            bytes::Bytes::from_static(b"2147483647"),
        ]; // Large but valid u64

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(1)) => {
                // Expected: key exists and expiration was set
            }
            _ => panic!("Expected integer 1 for EXPIRE with large TTL"),
        }

        let ttl = store.ttl("test_key").unwrap();
        assert!(ttl > 2147483640, "Expected large TTL, got {ttl}");
    }

    #[tokio::test]
    async fn test_expire_command_invalid_ttl_non_numeric() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();

        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"test_key"),
            bytes::Bytes::from_static(b"not_a_number"),
        ];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("value is not an integer"));
            }
            _ => panic!("Expected error for non-numeric TTL"),
        }
    }

    #[tokio::test]
    async fn test_expire_command_invalid_ttl_negative() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();

        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"test_key"),
            bytes::Bytes::from_static(b"-1"),
        ];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("value is not an integer"));
            }
            _ => panic!("Expected error for negative TTL"),
        }
    }

    #[tokio::test]
    async fn test_expire_command_wrong_args_too_few() {
        let store = create_test_store();
        let cmd = ExpireCommand;
        let args = vec![bytes::Bytes::from_static(b"only_key")];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("wrong number of arguments"));
            }
            _ => panic!("Expected error for wrong number of arguments"),
        }
    }

    #[tokio::test]
    async fn test_expire_command_unknown_flag() {
        // With NX/XX/GT/LT flag support a third arg that isn't a
        // recognized flag now produces `syntax error` instead of
        // `wrong number of arguments`.
        let store = create_test_store();
        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"key"),
            bytes::Bytes::from_static(b"60"),
            bytes::Bytes::from_static(b"extra"),
        ];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("syntax error"), "got {msg}");
            }
            _ => panic!("Expected syntax error for unrecognized flag"),
        }
    }

    #[tokio::test]
    async fn test_expire_command_overwrite_existing_expiration() {
        let store = create_test_store();
        store
            .set_with_ttl("test_key", "test_value", 120)
            .await
            .unwrap();

        // Verify initial TTL
        let initial_ttl = store.ttl("test_key").unwrap();
        assert!(
            initial_ttl > 100,
            "Expected initial TTL > 100, got {initial_ttl}"
        );

        // Set new expiration
        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"test_key"),
            bytes::Bytes::from_static(b"30"),
        ];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(1)) => {
                // Expected: key exists and expiration was updated
            }
            _ => panic!("Expected integer 1 for EXPIRE overwriting existing expiration"),
        }

        // Verify TTL was updated
        let new_ttl = store.ttl("test_key").unwrap();
        assert!(
            new_ttl <= 30 && new_ttl > 0,
            "Expected new TTL <= 30, got {new_ttl}"
        );
    }

    #[test]
    fn test_expire_command_properties() {
        let cmd = ExpireCommand;
        assert_eq!(cmd.name(), "EXPIRE");
        // Stage 8: arity widened for the optional NX/XX/GT/LT flag.
        assert_eq!(cmd.arity(), CommandArity::Range(3, 4));
    }

    // EXPIRE flag tests (Stage 8: NX/XX/GT/LT)

    #[tokio::test]
    async fn test_expire_nx_no_existing_ttl_applies() {
        let store = create_test_store();
        store.set("k", "v").await.unwrap();
        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"60"),
            bytes::Bytes::from_static(b"NX"),
        ];
        assert_eq!(
            cmd.execute(&args, &store).await,
            CommandResult::Ok(ResponseValue::Integer(1))
        );
        assert!(store.ttl("k").unwrap() > 0);
    }

    #[tokio::test]
    async fn test_expire_nx_existing_ttl_rejected() {
        let store = create_test_store();
        store.set_with_ttl("k", "v", 100).await.unwrap();
        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"60"),
            bytes::Bytes::from_static(b"NX"),
        ];
        assert_eq!(
            cmd.execute(&args, &store).await,
            CommandResult::Ok(ResponseValue::Integer(0))
        );
        // Original TTL preserved.
        assert!(store.ttl("k").unwrap() > 60);
    }

    #[tokio::test]
    async fn test_expire_xx_existing_ttl_applies() {
        let store = create_test_store();
        store.set_with_ttl("k", "v", 100).await.unwrap();
        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"30"),
            bytes::Bytes::from_static(b"XX"),
        ];
        assert_eq!(
            cmd.execute(&args, &store).await,
            CommandResult::Ok(ResponseValue::Integer(1))
        );
        let ttl = store.ttl("k").unwrap();
        assert!(ttl <= 30 && ttl > 0, "ttl {ttl}");
    }

    #[tokio::test]
    async fn test_expire_xx_no_existing_ttl_rejected() {
        let store = create_test_store();
        store.set("k", "v").await.unwrap();
        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"30"),
            bytes::Bytes::from_static(b"XX"),
        ];
        assert_eq!(
            cmd.execute(&args, &store).await,
            CommandResult::Ok(ResponseValue::Integer(0))
        );
        // No TTL was applied.
        assert_eq!(store.ttl("k").unwrap(), -1);
    }

    #[tokio::test]
    async fn test_expire_gt_only_applies_if_greater() {
        let store = create_test_store();
        store.set_with_ttl("k", "v", 30).await.unwrap();

        let cmd = ExpireCommand;
        // 10 < 30 → rejected
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"10"),
            bytes::Bytes::from_static(b"GT"),
        ];
        assert_eq!(
            cmd.execute(&args, &store).await,
            CommandResult::Ok(ResponseValue::Integer(0))
        );
        let ttl = store.ttl("k").unwrap();
        assert!(ttl > 20, "ttl should be unchanged, got {ttl}");

        // 200 > 30 → applies
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"200"),
            bytes::Bytes::from_static(b"GT"),
        ];
        assert_eq!(
            cmd.execute(&args, &store).await,
            CommandResult::Ok(ResponseValue::Integer(1))
        );
        assert!(store.ttl("k").unwrap() > 100);
    }

    #[tokio::test]
    async fn test_expire_gt_no_existing_ttl_rejected() {
        // GT with no current TTL: current is treated as +inf, so any
        // finite new TTL is shorter and the operation is rejected.
        let store = create_test_store();
        store.set("k", "v").await.unwrap();
        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"60"),
            bytes::Bytes::from_static(b"GT"),
        ];
        assert_eq!(
            cmd.execute(&args, &store).await,
            CommandResult::Ok(ResponseValue::Integer(0))
        );
        assert_eq!(store.ttl("k").unwrap(), -1);
    }

    #[tokio::test]
    async fn test_expire_lt_only_applies_if_less() {
        let store = create_test_store();
        store.set_with_ttl("k", "v", 30).await.unwrap();

        let cmd = ExpireCommand;
        // 100 > 30 → rejected
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"100"),
            bytes::Bytes::from_static(b"LT"),
        ];
        assert_eq!(
            cmd.execute(&args, &store).await,
            CommandResult::Ok(ResponseValue::Integer(0))
        );
        let ttl = store.ttl("k").unwrap();
        assert!(ttl <= 30, "ttl should be unchanged, got {ttl}");

        // 5 < 30 → applies
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"5"),
            bytes::Bytes::from_static(b"LT"),
        ];
        assert_eq!(
            cmd.execute(&args, &store).await,
            CommandResult::Ok(ResponseValue::Integer(1))
        );
        let ttl = store.ttl("k").unwrap();
        assert!(ttl <= 5 && ttl > 0, "ttl {ttl}");
    }

    #[tokio::test]
    async fn test_expire_lt_no_existing_ttl_applies() {
        // LT with no current TTL: current is +inf, so any finite TTL
        // is less and the operation applies.
        let store = create_test_store();
        store.set("k", "v").await.unwrap();
        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"60"),
            bytes::Bytes::from_static(b"LT"),
        ];
        assert_eq!(
            cmd.execute(&args, &store).await,
            CommandResult::Ok(ResponseValue::Integer(1))
        );
        let ttl = store.ttl("k").unwrap();
        assert!(ttl > 0 && ttl <= 60, "ttl {ttl}");
    }

    #[tokio::test]
    async fn test_expire_unknown_flag_rejected() {
        let store = create_test_store();
        store.set("k", "v").await.unwrap();
        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"60"),
            bytes::Bytes::from_static(b"FOO"),
        ];
        match cmd.execute(&args, &store).await {
            CommandResult::Error(msg) => assert!(msg.contains("syntax error"), "{msg}"),
            _ => panic!("expected syntax error"),
        }
    }

    #[tokio::test]
    async fn test_expire_flag_lowercase_accepted() {
        let store = create_test_store();
        store.set_with_ttl("k", "v", 100).await.unwrap();
        let cmd = ExpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"30"),
            bytes::Bytes::from_static(b"xx"),
        ];
        assert_eq!(
            cmd.execute(&args, &store).await,
            CommandResult::Ok(ResponseValue::Integer(1))
        );
    }

    // TTL command tests
    #[tokio::test]
    async fn test_ttl_command_key_with_expiration() {
        let store = create_test_store();
        store
            .set_with_ttl("test_key", "test_value", 60)
            .await
            .unwrap();

        let cmd = TtlCommand;
        let args = vec![bytes::Bytes::from_static(b"test_key")];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(ttl)) => {
                assert!(
                    ttl > 0 && ttl <= 60,
                    "Expected TTL between 1 and 60, got {ttl}"
                );
            }
            _ => panic!("Expected integer TTL for key with expiration"),
        }
    }

    #[tokio::test]
    async fn test_ttl_command_key_without_expiration() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();

        let cmd = TtlCommand;
        let args = vec![bytes::Bytes::from_static(b"test_key")];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(-1)) => {
                // Expected: -1 for key without expiration
            }
            _ => panic!("Expected integer -1 for key without expiration"),
        }
    }

    #[tokio::test]
    async fn test_ttl_command_nonexistent_key() {
        let store = create_test_store();

        let cmd = TtlCommand;
        let args = vec![bytes::Bytes::from_static(b"nonexistent_key")];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(-2)) => {
                // Expected: -2 for nonexistent key
            }
            _ => panic!("Expected integer -2 for nonexistent key"),
        }
    }

    #[tokio::test]
    async fn test_ttl_command_expired_key() {
        let store = create_test_store();

        // Set key with very short TTL
        let expires_at = std::time::Instant::now() + Duration::from_millis(1);
        store
            .set_with_expiration("expired_key", "value", expires_at)
            .await
            .unwrap();

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(10)).await;

        let cmd = TtlCommand;
        let args = vec![bytes::Bytes::from_static(b"expired_key")];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(-2)) => {
                // Expected: -2 for expired key (treated as nonexistent)
            }
            _ => panic!("Expected integer -2 for expired key"),
        }
    }

    #[tokio::test]
    async fn test_ttl_command_wrong_args_too_few() {
        let store = create_test_store();
        let cmd = TtlCommand;
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
    async fn test_ttl_command_wrong_args_too_many() {
        let store = create_test_store();
        let cmd = TtlCommand;
        let args = vec![
            bytes::Bytes::from_static(b"key1"),
            bytes::Bytes::from_static(b"key2"),
        ];

        let result = cmd.execute(&args, &store).await;

        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("wrong number of arguments"));
            }
            _ => panic!("Expected error for wrong number of arguments"),
        }
    }

    #[test]
    fn test_ttl_command_properties() {
        let cmd = TtlCommand;
        assert_eq!(cmd.name(), "TTL");
        assert_eq!(cmd.arity(), CommandArity::Fixed(2));
    }

    // Integration tests between EXPIRE and TTL commands
    #[tokio::test]
    async fn test_expire_ttl_integration() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();

        // Initially no expiration
        let ttl_cmd = TtlCommand;
        let ttl_args = vec![bytes::Bytes::from_static(b"test_key")];
        let result = ttl_cmd.execute(&ttl_args, &store).await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(-1)));

        // Set expiration
        let expire_cmd = ExpireCommand;
        let expire_args = vec![
            bytes::Bytes::from_static(b"test_key"),
            bytes::Bytes::from_static(b"30"),
        ];
        let result = expire_cmd.execute(&expire_args, &store).await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(1)));

        // Check TTL is now positive
        let result = ttl_cmd.execute(&ttl_args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Integer(ttl)) => {
                assert!(
                    ttl > 0 && ttl <= 30,
                    "Expected TTL between 1 and 30, got {ttl}"
                );
            }
            _ => panic!("Expected positive TTL after EXPIRE"),
        }
    }

    #[tokio::test]
    async fn test_expire_ttl_timing_accuracy() {
        let store = create_test_store();
        store.set("test_key", "test_value").await.unwrap();

        // Set 5 second expiration
        let expire_cmd = ExpireCommand;
        let expire_args = vec![
            bytes::Bytes::from_static(b"test_key"),
            bytes::Bytes::from_static(b"5"),
        ];
        expire_cmd.execute(&expire_args, &store).await;

        // Check TTL immediately
        let ttl_cmd = TtlCommand;
        let ttl_args = vec![bytes::Bytes::from_static(b"test_key")];
        let result = ttl_cmd.execute(&ttl_args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(ttl)) => {
                assert!(
                    (4..=5).contains(&ttl),
                    "Expected TTL between 4 and 5, got {ttl}"
                );
            }
            _ => panic!("Expected TTL between 4 and 5 seconds"),
        }

        // Wait 1 second and check again
        tokio::time::sleep(Duration::from_secs(1)).await;
        let result = ttl_cmd.execute(&ttl_args, &store).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(ttl)) => {
                assert!(
                    (3..=4).contains(&ttl),
                    "Expected TTL between 3 and 4 after 1 second, got {ttl}"
                );
            }
            _ => panic!("Expected TTL between 3 and 4 seconds after waiting"),
        }
    }

    #[tokio::test]
    async fn test_expire_ttl_with_key_updates() {
        let store = create_test_store();
        store.set("test_key", "original_value").await.unwrap();

        // Set expiration
        let expire_cmd = ExpireCommand;
        let expire_args = vec![
            bytes::Bytes::from_static(b"test_key"),
            bytes::Bytes::from_static(b"60"),
        ];
        expire_cmd.execute(&expire_args, &store).await;

        // Verify TTL is set
        let ttl_cmd = TtlCommand;
        let ttl_args = vec![bytes::Bytes::from_static(b"test_key")];
        let result = ttl_cmd.execute(&ttl_args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Integer(ttl)) => {
                assert!(ttl > 0, "Expected positive TTL, got {ttl}");
            }
            _ => panic!("Expected positive TTL"),
        }

        // Update the key value (should preserve expiration in our implementation)
        store.set("test_key", "new_value").await.unwrap();

        // Check if TTL is preserved or reset (depends on implementation)
        // In our current implementation, SET removes expiration
        let result = ttl_cmd.execute(&ttl_args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Integer(-1)) => {
                // Expected: SET removes expiration in our implementation
            }
            _ => panic!("Expected TTL to be reset after SET operation"),
        }
    }

    #[tokio::test]
    async fn test_expire_ttl_edge_cases() {
        let store = create_test_store();

        // Test with empty key name
        store.set("", "empty_key_value").await.unwrap();

        let expire_cmd = ExpireCommand;
        let expire_args = vec![
            bytes::Bytes::from_static(b""),
            bytes::Bytes::from_static(b"60"),
        ];
        let result = expire_cmd.execute(&expire_args, &store).await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(1)));

        let ttl_cmd = TtlCommand;
        let ttl_args = vec![bytes::Bytes::from_static(b"")];
        let result = ttl_cmd.execute(&ttl_args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Integer(ttl)) => {
                assert!(ttl > 0, "Expected positive TTL for empty key, got {ttl}");
            }
            _ => panic!("Expected positive TTL for empty key"),
        }
    }

    #[tokio::test]
    async fn test_expire_ttl_unicode_keys() {
        let store = create_test_store();
        let unicode_key_str = "🔑测试";
        store.set(unicode_key_str, "unicode_value").await.unwrap();
        let unicode_key = bytes::Bytes::from_static("🔑测试".as_bytes());

        // Set expiration on unicode key
        let expire_cmd = ExpireCommand;
        let expire_args = vec![unicode_key.clone(), bytes::Bytes::from_static(b"45")];
        let result = expire_cmd.execute(&expire_args, &store).await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(1)));

        // Check TTL for unicode key
        let ttl_cmd = TtlCommand;
        let ttl_args = vec![unicode_key.clone()];
        let result = ttl_cmd.execute(&ttl_args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Integer(ttl)) => {
                assert!(
                    ttl > 0 && ttl <= 45,
                    "Expected TTL between 1 and 45 for unicode key, got {ttl}"
                );
            }
            _ => panic!("Expected positive TTL for unicode key"),
        }
    }

    // Concurrent access tests
    #[tokio::test]
    async fn test_concurrent_expire_ttl_operations() {
        use std::sync::Arc;
        use tokio::task::JoinSet;

        let store = Arc::new(create_test_store());

        // Pre-populate with keys
        for i in 0..10 {
            let key = format!("key{i}");
            store.set(&key, format!("value{i}")).await.unwrap();
        }

        let mut join_set = JoinSet::new();

        // Spawn tasks that set expiration concurrently
        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            join_set.spawn(async move {
                let key = bytes::Bytes::from(format!("key{i}"));
                let ttl = 60 + i; // Different TTL for each key

                let expire_cmd = ExpireCommand;
                let expire_args = vec![key.clone(), bytes::Bytes::from(ttl.to_string())];
                let result = expire_cmd.execute(&expire_args, &store_clone).await;
                assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(1)));

                // Check TTL
                let ttl_cmd = TtlCommand;
                let ttl_args = vec![key.clone()];
                let result = ttl_cmd.execute(&ttl_args, &store_clone).await;
                let key_display = String::from_utf8_lossy(&key).into_owned();
                match result {
                    CommandResult::Ok(ResponseValue::Integer(actual_ttl)) => {
                        assert!(
                            actual_ttl > 0,
                            "Expected positive TTL for key {key_display}, got {actual_ttl}"
                        );
                    }
                    _ => panic!("Expected positive TTL for key {key_display}"),
                }
            });
        }

        // Wait for all tasks to complete
        while let Some(result) = join_set.join_next().await {
            result.unwrap(); // Panic if any task failed
        }

        // Verify all keys have expiration set
        for i in 0..10 {
            let key = format!("key{i}");
            let ttl = store.ttl(&key).unwrap();
            assert!(ttl > 0, "Expected positive TTL for key {key}, got {ttl}");
        }
    }

    // PTTL / PEXPIRE / EXPIREAT / PEXPIREAT tests (Stage 8 partial 4)

    #[tokio::test]
    async fn test_pttl_returns_milliseconds() {
        let store = create_test_store();
        store.set_with_ttl("k", "v", 60).await.unwrap();

        let cmd = PttlCommand;
        let args = vec![bytes::Bytes::from_static(b"k")];
        let result = cmd.execute(&args, &store).await;
        match result {
            CommandResult::Ok(ResponseValue::Integer(ms)) => {
                // 60s = 60_000ms; allow ±1s slack for test latency.
                assert!(
                    (59_000..=60_000).contains(&ms),
                    "expected ~60_000ms, got {ms}"
                );
            }
            other => panic!("expected integer ms, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_pttl_negative_for_no_ttl_and_missing_key() {
        let store = create_test_store();
        store.set("k", "v").await.unwrap();

        let cmd = PttlCommand;
        // Key without expiration → -1
        let result = cmd
            .execute(&[bytes::Bytes::from_static(b"k")], &store)
            .await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(-1)));

        // Missing key → -2
        let result = cmd
            .execute(&[bytes::Bytes::from_static(b"missing")], &store)
            .await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(-2)));
    }

    #[tokio::test]
    async fn test_pexpire_applies_ttl_in_milliseconds() {
        let store = create_test_store();
        store.set("k", "v").await.unwrap();

        let cmd = PexpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"30000"),
        ];
        let result = cmd.execute(&args, &store).await;
        assert_eq!(result, CommandResult::Ok(ResponseValue::Integer(1)));

        let pttl = store.pttl("k").unwrap();
        assert!(
            (29_000..=30_000).contains(&pttl),
            "expected pttl ~30_000ms, got {pttl}"
        );
    }

    #[tokio::test]
    async fn test_pexpire_with_xx_flag_rejects_keys_without_ttl() {
        let store = create_test_store();
        store.set("k", "v").await.unwrap();

        let cmd = PexpireCommand;
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"5000"),
            bytes::Bytes::from_static(b"XX"),
        ];
        assert_eq!(
            cmd.execute(&args, &store).await,
            CommandResult::Ok(ResponseValue::Integer(0))
        );
        assert_eq!(store.ttl("k").unwrap(), -1);
    }

    #[tokio::test]
    async fn test_expireat_with_far_future_unix_seconds() {
        let store = create_test_store();
        store.set("k", "v").await.unwrap();

        // 100 seconds from now in Unix-seconds.
        let target = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 100;

        let cmd = ExpireatCommand;
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from(target.to_string()),
        ];
        assert_eq!(
            cmd.execute(&args, &store).await,
            CommandResult::Ok(ResponseValue::Integer(1))
        );
        let ttl = store.ttl("k").unwrap();
        assert!(ttl > 90 && ttl <= 100, "ttl should be ~100s, got {ttl}");
    }

    #[tokio::test]
    async fn test_expireat_past_timestamp_deletes_immediately() {
        let store = create_test_store();
        store.set("k", "v").await.unwrap();

        let cmd = ExpireatCommand;
        // 0 = the epoch — definitely in the past.
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"0"),
        ];
        assert_eq!(
            cmd.execute(&args, &store).await,
            CommandResult::Ok(ResponseValue::Integer(1))
        );
        // Key reads as missing because the deadline is in the past.
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(!store.exists("k").unwrap());
    }

    #[tokio::test]
    async fn test_pexpireat_with_far_future_unix_millis() {
        let store = create_test_store();
        store.set("k", "v").await.unwrap();

        let target_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 50_000; // +50s in ms

        let cmd = PexpireatCommand;
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from(target_ms.to_string()),
        ];
        assert_eq!(
            cmd.execute(&args, &store).await,
            CommandResult::Ok(ResponseValue::Integer(1))
        );
        let pttl = store.pttl("k").unwrap();
        assert!(
            pttl > 40_000 && pttl <= 50_000,
            "pttl should be ~50_000ms, got {pttl}"
        );
    }

    #[tokio::test]
    async fn test_expireat_unknown_flag_rejected() {
        let store = create_test_store();
        store.set("k", "v").await.unwrap();

        let cmd = ExpireatCommand;
        let args = vec![
            bytes::Bytes::from_static(b"k"),
            bytes::Bytes::from_static(b"9999999999"),
            bytes::Bytes::from_static(b"FOO"),
        ];
        match cmd.execute(&args, &store).await {
            CommandResult::Error(msg) => assert!(msg.contains("syntax error"), "{msg}"),
            _ => panic!("expected syntax error"),
        }
    }

    #[test]
    fn test_new_ttl_commands_mutation_flags() {
        // PTTL is read-only; PEXPIRE/EXPIREAT/PEXPIREAT all mutate.
        assert!(!PttlCommand.is_mutation());
        assert!(PexpireCommand.is_mutation());
        assert!(ExpireatCommand.is_mutation());
        assert!(PexpireatCommand.is_mutation());
    }
}
