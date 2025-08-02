//! Error types and handling for RustyPotato
//! 
//! This module defines all error types used throughout the system
//! and provides conversion utilities for client responses.

use thiserror::Error;

/// Main error type for RustyPotato operations
#[derive(Debug, Error)]
pub enum RustyPotatoError {
    #[error("Invalid command: {0}")]
    InvalidCommand(String),
    
    #[error("Wrong number of arguments for command {command}: expected {expected}, got {actual}")]
    WrongArity { 
        command: String, 
        expected: String, 
        actual: usize 
    },
    
    #[error("Value is not an integer or out of range")]
    NotAnInteger,
    
    #[error("Key not found: {0}")]
    KeyNotFound(String),
    
    #[error("Persistence error: {0}")]
    PersistenceError(#[from] std::io::Error),
    
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    
    #[error("Configuration error: {0}")]
    ConfigError(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Connection error: {0}")]
    ConnectionError(String),
    
    #[error("Timeout error: {0}")]
    TimeoutError(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Convenience type alias for Results
pub type Result<T> = std::result::Result<T, RustyPotatoError>;

impl RustyPotatoError {
    /// Convert error to client-facing error message
    pub fn to_client_error(&self) -> String {
        match self {
            RustyPotatoError::InvalidCommand(cmd) => format!("ERR unknown command '{}'", cmd),
            RustyPotatoError::WrongArity { command, expected: _, actual: _ } => {
                format!("ERR wrong number of arguments for '{}' command", command)
            }
            RustyPotatoError::NotAnInteger => "ERR value is not an integer or out of range".to_string(),
            RustyPotatoError::KeyNotFound(_) => "ERR no such key".to_string(),
            RustyPotatoError::ProtocolError(msg) => format!("ERR protocol error: {}", msg),
            _ => "ERR internal server error".to_string(),
        }
    }
    
    /// Check if error should be logged as warning vs error
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            RustyPotatoError::InvalidCommand(_)
                | RustyPotatoError::WrongArity { .. }
                | RustyPotatoError::NotAnInteger
                | RustyPotatoError::KeyNotFound(_)
                | RustyPotatoError::ProtocolError(_)
        )
    }
}