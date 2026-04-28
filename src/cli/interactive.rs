//! Interactive REPL mode for CLI

use crate::cli::client::CliClient;
use crate::error::{Result, RustyPotatoError};
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
            eprintln!("Failed to connect to server: {e}");
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
                            let formatted = CliClient::format_response(&response);
                            println!("{formatted}");
                        }
                        Err(e) => {
                            eprintln!("Error: {e}");
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to read input: {}", e);
                    eprintln!("Failed to read input: {e}");
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
