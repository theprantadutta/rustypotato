//! Interactive REPL mode for the CLI client.
//!
//! Wraps `rustyline` for line editing, persistent history (up/down
//! arrows), and tab-completion of registered command names. Because
//! rustyline is synchronous and the CLI client runs on the tokio
//! runtime, every blocking `readline` call lives on
//! `tokio::task::spawn_blocking` so it doesn't stall the runtime.

use crate::cli::client::CliClient;
use crate::error::{Result, RustyPotatoError};
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::FileHistory;
use rustyline::validate::Validator;
use rustyline::{Context, Editor, Helper};
use std::path::PathBuf;
use tracing::{debug, error, warn};

/// All commands the CLI knows about. Used for tab completion.
/// Kept in sorted order for deterministic completion suggestions.
const KNOWN_COMMANDS: &[&str] = &[
    "BGREWRITEAOF",
    "COMMAND",
    "DBSIZE",
    "DECR",
    "DEL",
    "ECHO",
    "EXISTS",
    "EXPIRE",
    "EXPIREAT",
    "FLUSHDB",
    "GET",
    "HDEL",
    "HEXISTS",
    "HGET",
    "HGETALL",
    "HKEYS",
    "HLEN",
    "HMGET",
    "HSET",
    "HVALS",
    "INCR",
    "INFO",
    "KEYS",
    "MGET",
    "MSET",
    "PEXPIRE",
    "PEXPIREAT",
    "PING",
    "PTTL",
    "QUIT",
    "SET",
    "TTL",
    "TYPE",
];

/// Helper that wires rustyline up with command-name tab completion.
struct ReplHelper;

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> std::result::Result<(usize, Vec<Pair>), ReadlineError> {
        // Only complete the FIRST whitespace-delimited token (the
        // command name). Once the user has typed a space, key/field
        // arguments aren't worth speculating on.
        let prefix_end = line[..pos].find(char::is_whitespace).unwrap_or(pos);
        if prefix_end != pos {
            // Cursor is past the first token — no completions.
            return Ok((pos, vec![]));
        }
        let prefix = &line[..pos];
        let upper = prefix.to_ascii_uppercase();
        let candidates = KNOWN_COMMANDS
            .iter()
            .filter(|cmd| cmd.starts_with(&upper))
            .map(|cmd| Pair {
                display: (*cmd).to_string(),
                replacement: (*cmd).to_string(),
            })
            .collect();
        Ok((0, candidates))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;
}

impl Highlighter for ReplHelper {}
impl Validator for ReplHelper {}
impl Helper for ReplHelper {}

/// Interactive REPL for CLI client.
pub struct InteractiveMode {
    client: CliClient,
    history_path: PathBuf,
}

impl InteractiveMode {
    /// Create a new interactive mode instance. History is loaded
    /// from / saved to `~/.rustypotato_history`.
    pub fn new(address: String) -> Self {
        let history_path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".rustypotato_history");
        Self {
            client: CliClient::with_address(address),
            history_path,
        }
    }

    /// Start the interactive REPL.
    pub async fn start(&mut self) -> Result<()> {
        println!("RustyPotato CLI - Interactive Mode");
        println!("Type 'help' for available commands, 'quit' or 'exit' to leave");
        println!();

        // Connect to server
        if let Err(e) = self.client.connect().await {
            eprintln!("Failed to connect to server: {e}");
            return Err(e);
        }

        println!("Connected to RustyPotato server");
        println!();

        // Spin up rustyline. Loading history is best-effort; a missing
        // file on first run is normal.
        let mut editor: Editor<ReplHelper, FileHistory> =
            Editor::new().map_err(|e| RustyPotatoError::InternalError {
                message: format!("Failed to initialise rustyline editor: {e}"),
                component: Some("cli::interactive".to_string()),
                source: None,
            })?;
        editor.set_helper(Some(ReplHelper));
        let _ = editor.load_history(&self.history_path);

        // Each iteration moves the editor across an `await` boundary
        // via `spawn_blocking`; the editor returns at the end of the
        // turn so it can be reused next iteration.
        let mut editor = Some(editor);
        loop {
            let mut ed = editor.take().unwrap();
            let line_result =
                tokio::task::spawn_blocking(move || (ed.readline("rustypotato> "), ed))
                    .await
                    .map_err(|e| RustyPotatoError::InternalError {
                        message: format!("readline task panicked: {e}"),
                        component: Some("cli::interactive".to_string()),
                        source: None,
                    })?;
            let (read, ed) = line_result;
            editor = Some(ed);

            let line = match read {
                Ok(l) => l,
                Err(ReadlineError::Interrupted) => {
                    // Ctrl+C — clear the line and re-prompt.
                    continue;
                }
                Err(ReadlineError::Eof) => {
                    // Ctrl+D — graceful exit.
                    println!();
                    break;
                }
                Err(e) => {
                    error!("Failed to read input: {}", e);
                    eprintln!("Failed to read input: {e}");
                    break;
                }
            };

            let input = line.trim();
            if input.is_empty() {
                continue;
            }

            // Best-effort history append; ignore failures so the user
            // can still issue commands even if disk write fails.
            let _ = editor.as_mut().unwrap().add_history_entry(input);

            // Special meta-commands handled locally.
            match input.to_lowercase().as_str() {
                "quit" | "exit" => {
                    println!("Goodbye!");
                    break;
                }
                "help" => {
                    self.show_help();
                    continue;
                }
                "clear" => {
                    print!("\x1B[2J\x1B[1;1H");
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                    continue;
                }
                _ => {}
            }

            match self.parse_and_execute(input).await {
                Ok(response) => {
                    let formatted = CliClient::format_response(&response);
                    println!("{formatted}");
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                }
            }
        }

        // Save history; warn but don't fail the session if disk write
        // doesn't work.
        if let Some(ed) = editor.as_mut() {
            if let Err(e) = ed.save_history(&self.history_path) {
                warn!("Failed to save history: {}", e);
            }
        }

        // Disconnect from server
        if let Err(e) = self.client.disconnect().await {
            warn!("Error during disconnect: {}", e);
        }

        Ok(())
    }

    /// Parse command line and execute.
    async fn parse_and_execute(&mut self, input: &str) -> Result<crate::commands::ResponseValue> {
        let parts: Vec<&str> = input.split_whitespace().collect();

        if parts.is_empty() {
            return Err(RustyPotatoError::InvalidCommand {
                command: "".to_string(),
                source: None,
            });
        }

        let command = parts[0].to_uppercase();
        let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

        debug!("Executing command: {} with args: {:?}", command, args);

        self.client.execute_command(&command, &args).await
    }

    /// Show help information.
    fn show_help(&self) {
        println!("Available Commands:");
        println!();
        println!("String Operations:");
        println!("  SET key value [EX|PX|EXAT|PXAT n] [NX|XX] [KEEPTTL] [GET]");
        println!("  GET key                - Get the value of a key");
        println!("  DEL key [key ...]      - Delete keys");
        println!("  EXISTS key [key ...]   - Count existing keys");
        println!("  MGET key [key ...]     - Get multiple values");
        println!("  MSET key val [key val ...] - Set multiple key/value pairs");
        println!();
        println!("TTL Operations:");
        println!("  EXPIRE key seconds [NX|XX|GT|LT]");
        println!("  PEXPIRE key milliseconds [NX|XX|GT|LT]");
        println!("  EXPIREAT key unix-seconds [NX|XX|GT|LT]");
        println!("  PEXPIREAT key unix-milliseconds [NX|XX|GT|LT]");
        println!("  TTL key                - Get TTL in seconds");
        println!("  PTTL key               - Get TTL in milliseconds");
        println!();
        println!("Atomic Operations:");
        println!("  INCR key               - Increment integer value");
        println!("  DECR key               - Decrement integer value");
        println!();
        println!("Hash Operations:");
        println!("  HSET key field value [field value ...]");
        println!("  HGET key field");
        println!("  HMGET key field [field ...]");
        println!("  HDEL key field [field ...]");
        println!("  HGETALL key | HKEYS key | HVALS key | HLEN key");
        println!("  HEXISTS key field");
        println!();
        println!("Server / Admin:");
        println!("  PING [message] | ECHO message");
        println!("  DBSIZE | TYPE key | FLUSHDB | KEYS pattern");
        println!("  INFO [section] | COMMAND [COUNT|DOCS|LIST]");
        println!("  BGREWRITEAOF           - Compact the AOF");
        println!();
        println!("Interactive Commands:");
        println!("  help                   - Show this help message");
        println!("  clear                  - Clear the screen");
        println!("  quit, exit             - Exit the interactive mode");
        println!();
        println!("Tab-completion is available for command names.");
        println!("Up/down arrows recall previous commands.");
        println!();
    }
}
