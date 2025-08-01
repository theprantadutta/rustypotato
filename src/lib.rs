//! RustyPotato - A high-performance Redis-compatible key-value store
//! 
//! RustyPotato is built in Rust with a focus on performance, memory safety,
//! and concurrency. It provides Redis-like functionality with additional
//! unique features.

// Core modules
pub mod config;
pub mod error;

// Feature modules
pub mod commands;
pub mod storage;
pub mod network;
pub mod cli;

// Public API exports
pub use config::Config;
pub use error::{RustyPotatoError, Result};

// Re-export commonly used types
pub use commands::{CommandResult, ResponseValue};
pub use storage::{MemoryStore, StoredValue, ValueType};

/// RustyPotato server instance
pub struct RustyPotatoServer {
    config: Config,
    storage: storage::MemoryStore,
    // Additional fields will be added as components are implemented
}

impl RustyPotatoServer {
    /// Create a new RustyPotato server with the given configuration
    pub fn new(config: Config) -> Result<Self> {
        let storage = storage::MemoryStore::new();
        
        Ok(Self {
            config,
            storage,
        })
    }
    
    /// Get the server configuration
    pub fn config(&self) -> &Config {
        &self.config
    }
    
    /// Get a reference to the storage layer
    pub fn storage(&self) -> &storage::MemoryStore {
        &self.storage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let config = Config::default();
        let server = RustyPotatoServer::new(config);
        assert!(server.is_ok());
    }
    
    #[test]
    fn test_server_config_access() {
        let config = Config::default();
        let server = RustyPotatoServer::new(config).unwrap();
        assert_eq!(server.config().server.port, 6379);
    }
}
