//! CLI command definitions using clap

use clap::{Parser, Subcommand};

/// RustyPotato CLI client
#[derive(Parser)]
#[command(name = "rustypotato-cli")]
#[command(about = "A CLI client for RustyPotato key-value store")]
#[command(version)]
pub struct Cli {
    /// Server address to connect to
    #[arg(short, long, default_value = "127.0.0.1:6379")]
    pub address: String,

    /// Command to execute
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Available CLI commands
#[derive(Subcommand)]
pub enum Commands {
    /// Set a key-value pair
    Set { key: String, value: String },
    /// Get a value by key
    Get { key: String },
    /// Delete a key
    Del { key: String },
    /// Check if a key exists
    Exists { key: String },
    /// Set expiration time for a key
    Expire { key: String, seconds: u64 },
    /// Get time to live for a key
    Ttl { key: String },
    /// Start interactive REPL mode
    Interactive,
}
