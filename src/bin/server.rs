//! RustyPotato server binary
//! 
//! Main entry point for the RustyPotato server

use rusty_potato::{Config, RustyPotatoServer};
use tracing::{info, error};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Starting RustyPotato server...");

    // Load configuration (using defaults for now)
    let config = Config::default();
    
    info!("Server configuration: bind_address={}, port={}", 
          config.server.bind_address, config.server.port);

    // Create server instance
    let server = match RustyPotatoServer::new(config) {
        Ok(server) => server,
        Err(e) => {
            error!("Failed to create server: {}", e);
            return Err(e.into());
        }
    };

    info!("RustyPotato server created successfully");
    info!("Server will be fully functional after implementing network layer in task 10");

    // TODO: Start the server (will be implemented in later tasks)
    // server.start().await?;

    Ok(())
}