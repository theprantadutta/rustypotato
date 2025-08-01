//! RustyPotato CLI client binary
//! 
//! Command-line interface for interacting with RustyPotato servers

use clap::Parser;
use rusty_potato::cli::{Cli, Commands, CliClient};
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging for CLI
    tracing_subscriber::fmt()
        .with_env_filter("warn") // Less verbose for CLI
        .init();

    let cli = Cli::parse();

    info!("Connecting to RustyPotato server at {}", cli.address);

    // Create CLI client
    let _client = CliClient::new();

    match cli.command {
        Some(Commands::Set { key, value }) => {
            println!("SET {} {}", key, value);
            println!("Note: CLI functionality will be implemented in task 15");
        }
        Some(Commands::Get { key }) => {
            println!("GET {}", key);
            println!("Note: CLI functionality will be implemented in task 15");
        }
        Some(Commands::Del { key }) => {
            println!("DEL {}", key);
            println!("Note: CLI functionality will be implemented in task 15");
        }
        Some(Commands::Exists { key }) => {
            println!("EXISTS {}", key);
            println!("Note: CLI functionality will be implemented in task 15");
        }
        Some(Commands::Expire { key, seconds }) => {
            println!("EXPIRE {} {}", key, seconds);
            println!("Note: CLI functionality will be implemented in task 15");
        }
        Some(Commands::Ttl { key }) => {
            println!("TTL {}", key);
            println!("Note: CLI functionality will be implemented in task 15");
        }
        Some(Commands::Interactive) => {
            println!("Starting interactive mode...");
            println!("Note: Interactive mode will be implemented in task 15");
        }
        None => {
            println!("No command specified. Use --help for usage information.");
        }
    }

    Ok(())
}