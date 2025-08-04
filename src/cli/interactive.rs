//! Interactive REPL mode for CLI

use crate::cli::client::CliClient;
use crate::error::{RustyPotatoError, Result};
use std::io::{self, Write};
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, error, warn};

/// Interactive REPL for CLI client
pub struct InteractiveMode {
    client: CliClient,
    history: Vec<String>,
    max_history: usize,
}

impl InteractiveMode {
    /// Create a new interactive mode instance
    pub fn new(address: String) -> Self {
        Self {
            client: CliClient::with_address(address),
            history: Vec::new(),
            max_history: 1000,
        }
    }

    /// Start the interactive REPL
    pub async fn start(&mut self) -> Result<()> {
        println!("RustyPotato CLI - Interactive Mode");
        println!("Type 'help' for available commands, 'quit' or 'exit' to leave");
        println!();

        // Connect to server
        if let Err(e) = self.client.connect().await {
            eprintln!("Failed to connect to server: {}", e);
            return Err(e);
        }

        println!("Connected to RustyPotato server");
        println!();

        // Main REPL loop
        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        loop {
            // Print prompt
            print!("rustypotato> ");
            io::stdout().flush().unwrap();

            // Read line
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    // EOF (Ctrl+D)
                    println!();
                    break;
                }
                Ok(_) => {
                    let input = line.trim();
                    
                    if input.is_empty() {
                        continue;
                    }

                    // Handle special commands
                    match input.to_lowercase().as_str() {
                        "quit" | "exit" => {
                            println!("Goodbye!");
                            break;
                        }
                        "help" => {
                            self.show_help();
                            continue;
                        }
                        "history" => {
                            self.show_history();
                            continue;
                        }
                        "clear" => {
                            // Clear screen (works on most terminals)
                            print!("\x1B[2J\x1B[1;1H");
                            io::stdout().flush().unwrap();
                            continue;
                        }
                        _ => {}
                    }

                    // Add to history
                    self.add_to_history(input.to_string());

                    // Parse and execute command
                    match self.parse_and_execute(input).await {
                        Ok(response) => {
                            let formatted = self.client.format_response(&response);
                            println!("{}", formatted);
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to read input: {}", e);
                    eprintln!("Failed to read input: {}", e);
                    break;
                }
            }
        }

        // Disconnect from server
        if let Err(e) = self.client.disconnect().await {
            warn!("Error during disconnect: {}", e);
        }

        Ok(())
    }

    /// Parse command line and execute
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

    /// Add command to history
    fn add_to_history(&mut self, command: String) {
        // Don't add duplicate consecutive commands
        if let Some(last) = self.history.last() {
            if last == &command {
                return;
            }
        }

        self.history.push(command);

        // Limit history size
        if self.history.len() > self.max_history {
            self.history.remove(0);
        }
    }

    /// Show command history
    fn show_history(&self) {
        if self.history.is_empty() {
            println!("No command history");
            return;
        }

        println!("Command History:");
        for (i, cmd) in self.history.iter().enumerate() {
            println!("{:3}: {}", i + 1, cmd);
        }
    }

    /// Show help information
    fn show_help(&self) {
        println!("Available Commands:");
        println!();
        println!("String Operations:");
        println!("  SET key value          - Set a key to a value");
        println!("  GET key                - Get the value of a key");
        println!("  DEL key                - Delete a key");
        println!("  EXISTS key             - Check if a key exists");
        println!();
        println!("TTL Operations:");
        println!("  EXPIRE key seconds     - Set expiration time for a key");
        println!("  TTL key                - Get time to live for a key");
        println!();
        println!("Atomic Operations:");
        println!("  INCR key               - Increment the integer value of a key");
        println!("  DECR key               - Decrement the integer value of a key");
        println!();
        println!("Interactive Commands:");
        println!("  help                   - Show this help message");
        println!("  history                - Show command history");
        println!("  clear                  - Clear the screen");
        println!("  quit, exit             - Exit the interactive mode");
        println!();
        println!("Examples:");
        println!("  SET mykey \"hello world\"");
        println!("  GET mykey");
        println!("  EXPIRE mykey 60");
        println!("  INCR counter");
        println!();
    }
}

/// Simple command line parser for interactive mode
pub struct CommandParser;

impl CommandParser {
    /// Parse a command line into command and arguments
    /// Handles quoted strings and escaping
    pub fn parse(input: &str) -> Result<Vec<String>> {
        let mut parts = Vec::new();
        let mut current_part = String::new();
        let mut in_quotes = false;
        let mut escape_next = false;
        let mut chars = input.chars().peekable();
        let mut has_quotes = false;

        while let Some(ch) = chars.next() {
            if escape_next {
                // Handle escape sequences
                match ch {
                    'n' => current_part.push('\n'),
                    't' => current_part.push('\t'),
                    'r' => current_part.push('\r'),
                    '\\' => current_part.push('\\'),
                    '"' => current_part.push('"'),
                    _ => {
                        // For unknown escape sequences, just include the character
                        current_part.push(ch);
                    }
                }
                escape_next = false;
                continue;
            }

            match ch {
                '\\' => {
                    escape_next = true;
                }
                '"' => {
                    in_quotes = !in_quotes;
                    has_quotes = true;
                }
                ' ' | '\t' if !in_quotes => {
                    if !current_part.is_empty() || has_quotes {
                        parts.push(current_part.clone());
                        current_part.clear();
                        has_quotes = false;
                    }
                    // Skip multiple whitespace
                    while let Some(&next_ch) = chars.peek() {
                        if next_ch == ' ' || next_ch == '\t' {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
                _ => {
                    current_part.push(ch);
                }
            }
        }

        if in_quotes {
            return Err(RustyPotatoError::ProtocolError {
                message: "Unterminated quoted string".to_string(),
                command: Some(input.to_string()),
                source: None,
            });
        }

        if !current_part.is_empty() || has_quotes {
            parts.push(current_part);
        }

        Ok(parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_parser_simple() {
        let result = CommandParser::parse("SET key value").unwrap();
        assert_eq!(result, vec!["SET", "key", "value"]);
    }

    #[test]
    fn test_command_parser_quoted() {
        let result = CommandParser::parse("SET key \"hello world\"").unwrap();
        assert_eq!(result, vec!["SET", "key", "hello world"]);
    }

    #[test]
    fn test_command_parser_escaped() {
        let result = CommandParser::parse("SET key \"hello \\\"world\\\"\"").unwrap();
        assert_eq!(result, vec!["SET", "key", "hello \"world\""]);
    }

    #[test]
    fn test_command_parser_multiple_spaces() {
        let result = CommandParser::parse("SET    key     value").unwrap();
        assert_eq!(result, vec!["SET", "key", "value"]);
    }

    #[test]
    fn test_command_parser_empty() {
        let result = CommandParser::parse("").unwrap();
        assert_eq!(result, Vec::<String>::new());
    }

    #[test]
    fn test_command_parser_unterminated_quote() {
        let result = CommandParser::parse("SET key \"unterminated");
        assert!(result.is_err());
    }

    #[test]
    fn test_command_parser_tabs() {
        let result = CommandParser::parse("SET\tkey\tvalue").unwrap();
        assert_eq!(result, vec!["SET", "key", "value"]);
    }

    #[test]
    fn test_command_parser_escaped_backslash() {
        let result = CommandParser::parse("SET key \"path\\\\to\\\\file\"").unwrap();
        assert_eq!(result, vec!["SET", "key", "path\\to\\file"]);
    }
}