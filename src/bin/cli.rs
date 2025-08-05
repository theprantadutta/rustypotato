//! RustyPotato CLI client binary
//!
//! Command-line interface for interacting with RustyPotato servers

use clap::Parser;
use rustypotato::cli::interactive::InteractiveMode;
use rustypotato::cli::{Cli, CliClient, Commands};
use tracing::error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging for CLI
    tracing_subscriber::fmt()
        .with_env_filter("warn") // Less verbose for CLI
        .init();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Set { key, value }) => {
            execute_single_command(&cli.address, "SET", &[key, value]).await?;
        }
        Some(Commands::Get { key }) => {
            execute_single_command(&cli.address, "GET", &[key]).await?;
        }
        Some(Commands::Del { key }) => {
            execute_single_command(&cli.address, "DEL", &[key]).await?;
        }
        Some(Commands::Exists { key }) => {
            execute_single_command(&cli.address, "EXISTS", &[key]).await?;
        }
        Some(Commands::Expire { key, seconds }) => {
            execute_single_command(&cli.address, "EXPIRE", &[key, seconds.to_string()]).await?;
        }
        Some(Commands::Ttl { key }) => {
            execute_single_command(&cli.address, "TTL", &[key]).await?;
        }
        Some(Commands::Interactive) => {
            let mut interactive = InteractiveMode::new(cli.address);
            if let Err(e) = interactive.start().await {
                error!("Interactive mode failed: {}", e);
                eprintln!("Interactive mode failed: {e}");
                std::process::exit(1);
            }
        }
        None => {
            println!("No command specified. Use --help for usage information.");
            println!("Use 'rustypotato-cli interactive' to start interactive mode.");
        }
    }

    Ok(())
}

/// Execute a single command and display the result
async fn execute_single_command(
    address: &str,
    command: &str,
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = CliClient::with_address(address.to_string());

    // Connect to server
    if let Err(e) = client.connect().await {
        eprintln!("Failed to connect to server at {address}: {e}");
        std::process::exit(1);
    }

    // Execute command
    match client.execute_command(command, args).await {
        Ok(response) => {
            let formatted = CliClient::format_response(&response);
            println!("{formatted}");
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }

    // Disconnect
    if let Err(e) = client.disconnect().await {
        error!("Error during disconnect: {}", e);
    }

    Ok(())
}
