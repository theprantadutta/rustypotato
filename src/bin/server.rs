//! RustyPotato server binary
//! 
//! Main entry point for the RustyPotato server with full command engine and network integration

use rustypotato::{Config, RustyPotatoServer};
use tracing::{info, error, warn};
use tracing_subscriber;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging with structured output
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .with_thread_ids(true)
        .init();

    info!("Starting RustyPotato server...");

    // Load configuration from file and environment variables
    let config = match Config::load() {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            return Err(e.into());
        }
    };
    
    info!("Server configuration: bind_address={}, port={}, max_connections={}", 
          config.server.bind_address, config.server.port, config.server.max_connections);

    // Create server instance with full integration
    let mut server = match RustyPotatoServer::new(config) {
        Ok(server) => server,
        Err(e) => {
            error!("Failed to create server: {}", e);
            return Err(e.into());
        }
    };

    // Display server statistics
    let stats = server.stats().await;
    info!("RustyPotato server created successfully with {} registered commands: {:?}", 
          stats.registered_commands, stats.command_names);

    // Set up graceful shutdown handling
    let shutdown_signal = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C signal handler");
        warn!("Received CTRL+C, initiating graceful shutdown...");
    };

    // Start the server with integrated command engine and network layer
    tokio::select! {
        result = server.start() => {
            match result {
                Ok(()) => {
                    info!("Server stopped normally");
                }
                Err(e) => {
                    error!("Server error: {}", e);
                    return Err(e.into());
                }
            }
        }
        _ = shutdown_signal => {
            info!("Shutdown signal received, stopping server...");
            if let Err(e) = server.shutdown().await {
                error!("Error during shutdown: {}", e);
            }
        }
    }

    info!("RustyPotato server shutdown complete");
    Ok(())
}