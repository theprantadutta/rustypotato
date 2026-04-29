//! Server-level commands that don't touch storage state directly:
//! PING, ECHO, DBSIZE, TYPE, FLUSHDB, INFO and (in subsequent
//! stages) COMMAND, KEYS, QUIT.

use crate::commands::registry::Args;
use crate::commands::{arg_str, Command, CommandArity, CommandResult, ResponseValue};
use crate::storage::MemoryStore;
use async_trait::async_trait;
use std::sync::OnceLock;
use std::time::Instant;

/// Process start time, captured the first time INFO runs. Used as a
/// best-effort proxy for server uptime in the absence of an explicit
/// "server started at" hook into `RustyPotatoServer`. The drift
/// against true server startup is the gap between bind and the first
/// INFO probe — usually well under a second on real deployments.
static UPTIME_ANCHOR: OnceLock<Instant> = OnceLock::new();

fn uptime_anchor() -> Instant {
    *UPTIME_ANCHOR.get_or_init(Instant::now)
}

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
    async fn execute(&self, args: Args<'_>, _store: &MemoryStore) -> CommandResult {
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
    async fn execute(&self, args: Args<'_>, _store: &MemoryStore) -> CommandResult {
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
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
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
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'TYPE' command".to_string(),
            );
        }
        let key = match arg_str(args, 0) {
            Ok(k) => k,
            Err(e) => return CommandResult::Error(e),
        };
        let kind = match store.get(key) {
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
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        // Real Redis accepts a single optional `ASYNC` / `SYNC` token;
        // we don't async-flush so we just reject unknown args.
        match args.len() {
            0 => {
                store.clear();
                CommandResult::Ok(ResponseValue::SimpleString("OK".to_string()))
            }
            1 if args[0].eq_ignore_ascii_case(b"SYNC")
                || args[0].eq_ignore_ascii_case(b"ASYNC") =>
            {
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

/// `INFO [section]` — return server-side diagnostic info as a single
/// bulk string in Redis' `# Section\nkey:value\n...` format.
///
/// Stage 7 implementation: the `server` and `keyspace` sections are
/// supported; everything else is omitted. `INFO all` and `INFO
/// default` return both. `INFO <unknown>` returns an empty bulk
/// string (matches Redis behavior, surprisingly).
pub struct InfoCommand;

#[async_trait]
impl Command for InfoCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        // Trigger uptime anchor on first use.
        let uptime_seconds = uptime_anchor().elapsed().as_secs();

        let want_section = match args.len() {
            0 => "default".to_string(),
            1 => match arg_str(args, 0) {
                Ok(s) => s.to_string(),
                Err(e) => return CommandResult::Error(e),
            },
            _ => {
                return CommandResult::Error(
                    "ERR wrong number of arguments for 'INFO' command".to_string(),
                );
            }
        };

        let want_server = matches!(
            &*want_section.to_ascii_lowercase(),
            "server" | "default" | "all" | "everything"
        );
        let want_keyspace = matches!(
            &*want_section.to_ascii_lowercase(),
            "keyspace" | "default" | "all" | "everything"
        );

        let mut out = String::new();
        if want_server {
            out.push_str("# Server\r\n");
            out.push_str(&format!(
                "rustypotato_version:{}\r\n",
                env!("CARGO_PKG_VERSION")
            ));
            out.push_str(&format!("os:{}\r\n", std::env::consts::OS));
            out.push_str(&format!("arch:{}\r\n", std::env::consts::ARCH));
            out.push_str(&format!("process_id:{}\r\n", std::process::id()));
            out.push_str(&format!("uptime_in_seconds:{uptime_seconds}\r\n"));
            out.push_str(&format!("tcp_port:{}\r\n", 0)); // server doesn't surface its port to commands; clients already know it
            out.push_str("\r\n");
        }
        if want_keyspace {
            out.push_str("# Keyspace\r\n");
            let keys = store.len();
            if keys > 0 {
                out.push_str(&format!("db0:keys={keys},expires=0,avg_ttl=0\r\n"));
            }
            out.push_str("\r\n");
        }

        CommandResult::Ok(ResponseValue::bulk(out))
    }

    fn name(&self) -> &'static str {
        "INFO"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Range(1, 2)
    }

    fn is_mutation(&self) -> bool {
        false
    }
}

/// `KEYS pattern` — return all keys matching a glob pattern.
///
/// Supported metacharacters (subset of Redis' glob, sufficient for the
/// common `*`, `prefix:*`, `user:?:profile` patterns):
///   `*`         — match any (possibly empty) sequence of characters
///   `?`         — match exactly one character
///   `[abc]`     — match one of the listed characters
///   `[^abc]`    — match any character NOT in the list
///   `[a-z]`     — match a character in the range
///   `\<c>`      — escape the next character (treat `*?[\` as literal)
///
/// O(N) where N = total keys. Documented as such — same caveat Redis
/// gives. Suitable for admin / dev workflows; production code should
/// prefer SCAN once we add it.
pub struct KeysCommand;

#[async_trait]
impl Command for KeysCommand {
    async fn execute(&self, args: Args<'_>, store: &MemoryStore) -> CommandResult {
        if args.len() != 1 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'KEYS' command".to_string(),
            );
        }
        let pattern = match arg_str(args, 0) {
            Ok(p) => p.to_string(),
            Err(e) => return CommandResult::Error(e),
        };
        let matches: Vec<ResponseValue> = store
            .keys()
            .into_iter()
            .filter(|k| glob_match(&pattern, k))
            .map(ResponseValue::bulk)
            .collect();
        CommandResult::Ok(ResponseValue::Array(matches))
    }

    fn name(&self) -> &'static str {
        "KEYS"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(2) // KEYS pattern
    }

    fn is_mutation(&self) -> bool {
        false
    }
}

/// Match `text` against a Redis-style glob `pattern`. Recursive
/// implementation; the recursion only descends on `*`, so worst case
/// is O(len(pattern) * len(text)) for adversarial patterns — fine for
/// the keyspaces this server is sized for.
fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), text.as_bytes())
}

fn glob_match_bytes(p: &[u8], t: &[u8]) -> bool {
    let (mut pi, mut ti) = (0usize, 0usize);
    // Backtrack indices for the most recent `*`.
    let (mut star_p, mut star_t): (Option<usize>, usize) = (None, 0);
    while ti < t.len() {
        if pi < p.len() {
            match p[pi] {
                b'\\' if pi + 1 < p.len() => {
                    if p[pi + 1] == t[ti] {
                        pi += 2;
                        ti += 1;
                        continue;
                    }
                }
                b'?' => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                b'*' => {
                    star_p = Some(pi);
                    star_t = ti;
                    pi += 1;
                    continue;
                }
                b'[' => {
                    if let Some((advance, ok)) = match_class(&p[pi..], t[ti]) {
                        if ok {
                            pi += advance;
                            ti += 1;
                            continue;
                        }
                    }
                }
                c if c == t[ti] => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                _ => {}
            }
        }
        // No match at this position — backtrack to the last `*` if any.
        if let Some(sp) = star_p {
            pi = sp + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    // Consume any remaining `*`s in the pattern.
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// Parse a `[...]` character class starting at `p[0]`. Returns the
/// number of bytes consumed (including the closing `]`) and whether
/// `c` matches. Returns None if the class is malformed (no closing
/// bracket).
fn match_class(p: &[u8], c: u8) -> Option<(usize, bool)> {
    debug_assert_eq!(p[0], b'[');
    let mut i = 1usize;
    let mut negate = false;
    if i < p.len() && p[i] == b'^' {
        negate = true;
        i += 1;
    }
    let mut matched = false;
    while i < p.len() && p[i] != b']' {
        // Range a-z
        if i + 2 < p.len() && p[i + 1] == b'-' && p[i + 2] != b']' {
            if p[i] <= c && c <= p[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if p[i] == c {
                matched = true;
            }
            i += 1;
        }
    }
    if i >= p.len() {
        return None;
    }
    // Consume closing ']'
    Some((i + 1, matched ^ negate))
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
            .execute(&[bytes::Bytes::from_static(b"hello world")], &store)
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
            .execute(
                &[
                    bytes::Bytes::from_static(b"a"),
                    bytes::Bytes::from_static(b"b"),
                ],
                &store,
            )
            .await;
        assert!(matches!(result, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn echo_returns_message_as_bulk_string() {
        let store = MemoryStore::new();
        let result = EchoCommand
            .execute(&[bytes::Bytes::from_static(b"hi there")], &store)
            .await;
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
                .execute(
                    &[
                        bytes::Bytes::from_static(b"a"),
                        bytes::Bytes::from_static(b"b")
                    ],
                    &store
                )
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
            DbsizeCommand
                .execute(&[bytes::Bytes::from_static(b"x")], &store)
                .await,
            CommandResult::Error(_)
        ));
    }

    #[tokio::test]
    async fn type_returns_none_for_missing_key() {
        let store = MemoryStore::new();
        let r = TypeCommand
            .execute(&[bytes::Bytes::from_static(b"nope")], &store)
            .await;
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

        let s = TypeCommand
            .execute(&[bytes::Bytes::from_static(b"s")], &store)
            .await;
        let i = TypeCommand
            .execute(&[bytes::Bytes::from_static(b"i")], &store)
            .await;
        let h = TypeCommand
            .execute(&[bytes::Bytes::from_static(b"h")], &store)
            .await;

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
        let r = FlushdbCommand
            .execute(&[bytes::Bytes::from_static(b"SYNC")], &store)
            .await;
        assert!(matches!(r, CommandResult::Ok(_)));
        store.set("k", "v").await.unwrap();
        let r = FlushdbCommand
            .execute(&[bytes::Bytes::from_static(b"async")], &store)
            .await;
        assert!(matches!(r, CommandResult::Ok(_)));
    }

    #[tokio::test]
    async fn flushdb_rejects_unknown_modifier() {
        let store = MemoryStore::new();
        let r = FlushdbCommand
            .execute(&[bytes::Bytes::from_static(b"NUKE")], &store)
            .await;
        assert!(matches!(r, CommandResult::Error(_)));
    }

    #[tokio::test]
    async fn flushdb_is_a_mutation_dbsize_type_are_not() {
        assert!(FlushdbCommand.is_mutation());
        assert!(!DbsizeCommand.is_mutation());
        assert!(!TypeCommand.is_mutation());
    }

    fn extract_bulk(result: &CommandResult) -> std::borrow::Cow<'_, str> {
        match result {
            CommandResult::Ok(ResponseValue::BulkString(Some(s))) => String::from_utf8_lossy(s),
            other => panic!("expected bulk string, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn info_default_includes_server_and_keyspace() {
        let store = MemoryStore::new();
        store.set("k1", "v").await.unwrap();
        let r = InfoCommand.execute(&[], &store).await;
        let body = extract_bulk(&r);
        assert!(body.contains("# Server"));
        assert!(body.contains("rustypotato_version:"));
        assert!(body.contains("process_id:"));
        assert!(body.contains("uptime_in_seconds:"));
        assert!(body.contains("# Keyspace"));
        assert!(body.contains("db0:keys=1"));
    }

    #[tokio::test]
    async fn info_server_section_only() {
        let store = MemoryStore::new();
        let r = InfoCommand
            .execute(&[bytes::Bytes::from_static(b"server")], &store)
            .await;
        let body = extract_bulk(&r);
        assert!(body.contains("# Server"));
        assert!(!body.contains("# Keyspace"));
    }

    #[tokio::test]
    async fn info_unknown_section_yields_empty_bulk() {
        let store = MemoryStore::new();
        let r = InfoCommand
            .execute(&[bytes::Bytes::from_static(b"nonsense")], &store)
            .await;
        assert_eq!(extract_bulk(&r), "");
    }

    #[tokio::test]
    async fn info_too_many_args_errors() {
        let store = MemoryStore::new();
        assert!(matches!(
            InfoCommand
                .execute(
                    &[
                        bytes::Bytes::from_static(b"a"),
                        bytes::Bytes::from_static(b"b")
                    ],
                    &store
                )
                .await,
            CommandResult::Error(_)
        ));
    }

    #[tokio::test]
    async fn info_is_not_a_mutation() {
        assert!(!InfoCommand.is_mutation());
    }

    #[test]
    fn glob_match_basics() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
        assert!(glob_match("foo", "foo"));
        assert!(!glob_match("foo", "bar"));
        assert!(glob_match("foo*", "foobar"));
        assert!(glob_match("*bar", "foobar"));
        assert!(glob_match("f*o*r", "foobar"));
        assert!(!glob_match("f*o*r", "foobaz"));
    }

    #[test]
    fn glob_match_question_mark() {
        assert!(glob_match("?oo", "foo"));
        assert!(!glob_match("?oo", "fooz"));
        assert!(!glob_match("?", ""));
        assert!(glob_match("user:?:profile", "user:1:profile"));
        assert!(!glob_match("user:?:profile", "user:11:profile"));
    }

    #[test]
    fn glob_match_character_class() {
        assert!(glob_match("[abc]oo", "boo"));
        assert!(!glob_match("[abc]oo", "doo"));
        assert!(glob_match("[^abc]oo", "doo"));
        assert!(!glob_match("[^abc]oo", "boo"));
        assert!(glob_match("[a-z]oo", "moo"));
        assert!(!glob_match("[a-z]oo", "Moo"));
        assert!(glob_match("[a-c1-3]", "2"));
    }

    #[test]
    fn glob_match_escapes() {
        assert!(glob_match("\\*foo", "*foo"));
        assert!(!glob_match("\\*foo", "Xfoo"));
        assert!(glob_match("\\?", "?"));
    }

    #[tokio::test]
    async fn keys_returns_array_of_matches() {
        let store = MemoryStore::new();
        store.set("user:1", "a").await.unwrap();
        store.set("user:2", "b").await.unwrap();
        store.set("session:1", "c").await.unwrap();
        let r = KeysCommand
            .execute(&[bytes::Bytes::from_static(b"user:*")], &store)
            .await;
        match r {
            CommandResult::Ok(ResponseValue::Array(items)) => {
                assert_eq!(items.len(), 2);
                let strs: Vec<String> = items
                    .into_iter()
                    .map(|v| match v {
                        ResponseValue::BulkString(Some(s)) => {
                            String::from_utf8_lossy(&s).into_owned()
                        }
                        _ => panic!(),
                    })
                    .collect();
                assert!(strs.contains(&"user:1".to_string()));
                assert!(strs.contains(&"user:2".to_string()));
            }
            _ => panic!("expected array"),
        }
    }

    #[tokio::test]
    async fn keys_returns_all_for_star() {
        let store = MemoryStore::new();
        store.set("a", "1").await.unwrap();
        store.set("b", "2").await.unwrap();
        let r = KeysCommand
            .execute(&[bytes::Bytes::from_static(b"*")], &store)
            .await;
        if let CommandResult::Ok(ResponseValue::Array(items)) = r {
            assert_eq!(items.len(), 2);
        } else {
            panic!("expected array");
        }
    }

    #[tokio::test]
    async fn keys_empty_when_no_match() {
        let store = MemoryStore::new();
        store.set("a", "1").await.unwrap();
        let r = KeysCommand
            .execute(&[bytes::Bytes::from_static(b"nope:*")], &store)
            .await;
        assert_eq!(r, CommandResult::Ok(ResponseValue::Array(vec![])));
    }

    #[tokio::test]
    async fn keys_wrong_arity_errors() {
        let store = MemoryStore::new();
        assert!(matches!(
            KeysCommand.execute(&[], &store).await,
            CommandResult::Error(_)
        ));
        assert!(matches!(
            KeysCommand
                .execute(
                    &[
                        bytes::Bytes::from_static(b"a"),
                        bytes::Bytes::from_static(b"b")
                    ],
                    &store
                )
                .await,
            CommandResult::Error(_)
        ));
    }

    #[tokio::test]
    async fn keys_is_not_a_mutation() {
        assert!(!KeysCommand.is_mutation());
    }
}
