//! Error types and handling for RustyPotato
//!
//! This module defines all error types used throughout the system,
//! provides conversion utilities for client responses, and implements
//! structured logging.

use thiserror::Error;
use tracing::{debug, error, instrument, warn, Span};

/// Error severity levels for logging and recovery decisions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    /// Critical errors that require immediate attention and may cause service degradation
    Critical,
    /// High severity errors that affect functionality but don't cause service failure
    High,
    /// Medium severity errors that are recoverable and don't significantly impact service
    Medium,
    /// Low severity errors that are expected in normal operation (e.g., client errors)
    Low,
}

/// Error categories for better error handling and recovery
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Client-side errors (invalid commands, wrong arguments, etc.)
    Client,
    /// Network-related errors (connection issues, timeouts, etc.)
    Network,
    /// Storage-related errors (persistence, memory issues, etc.)
    Storage,
    /// Configuration-related errors
    Configuration,
    /// Internal system errors
    System,
    /// Protocol-related errors
    Protocol,
}

/// Main error type for RustyPotato operations with enhanced metadata
#[derive(Debug, Error)]
pub enum RustyPotatoError {
    #[error("Invalid command: {command}")]
    InvalidCommand {
        command: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error(
        "Wrong number of arguments for command '{command}': expected {expected}, got {actual}"
    )]
    WrongArity {
        command: String,
        expected: String,
        actual: usize,
    },

    #[error("Value is not an integer or out of range: {value}")]
    NotAnInteger { value: String },

    /// INCR/DECR/INCRBY/DECRBY overflow — distinct from `NotAnInteger`
    /// because Redis clients pattern-match the exact error message
    /// (`ERR increment or decrement would overflow`) to surface this
    /// case to users. Conflating it with the generic non-integer error
    /// breaks client-side handling.
    #[error("increment or decrement would overflow")]
    IntegerOverflow,

    #[error("Key not found: {key}")]
    KeyNotFound { key: String },

    #[error("Persistence error: {message}")]
    PersistenceError {
        message: String,
        #[source]
        source: Option<std::io::Error>,
        recoverable: bool,
    },

    #[error("Network error: {message}")]
    NetworkError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        connection_id: Option<String>,
    },

    #[error("Protocol error: {message}")]
    ProtocolError {
        message: String,
        command: Option<String>,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Configuration error: {message}")]
    ConfigError {
        message: String,
        config_key: Option<String>,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Storage error: {message}")]
    StorageError {
        message: String,
        operation: Option<String>,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Connection error: {message}")]
    ConnectionError {
        message: String,
        connection_id: Option<String>,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Timeout error: {message}")]
    TimeoutError {
        message: String,
        operation: Option<String>,
        timeout_duration: Option<std::time::Duration>,
    },

    #[error("Authentication error: {message}")]
    AuthenticationError {
        message: String,
        user: Option<String>,
    },

    #[error("Authorization error: {message}")]
    AuthorizationError {
        message: String,
        user: Option<String>,
        operation: Option<String>,
    },

    #[error("Resource exhaustion: {message}")]
    ResourceExhaustion {
        message: String,
        resource_type: String,
        current_usage: Option<u64>,
        limit: Option<u64>,
    },

    #[error("Serialization error: {message}")]
    SerializationError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Internal error: {message}")]
    InternalError {
        message: String,
        component: Option<String>,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

/// Convenience type alias for Results
pub type Result<T> = std::result::Result<T, RustyPotatoError>;

impl RustyPotatoError {
    /// Get the error severity level for logging and recovery decisions
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            RustyPotatoError::InvalidCommand { .. }
            | RustyPotatoError::WrongArity { .. }
            | RustyPotatoError::NotAnInteger { .. }
            | RustyPotatoError::IntegerOverflow
            | RustyPotatoError::KeyNotFound { .. }
            | RustyPotatoError::AuthenticationError { .. }
            | RustyPotatoError::AuthorizationError { .. } => ErrorSeverity::Low,

            RustyPotatoError::ProtocolError { .. }
            | RustyPotatoError::TimeoutError { .. }
            | RustyPotatoError::SerializationError { .. } => ErrorSeverity::Medium,

            RustyPotatoError::NetworkError { .. }
            | RustyPotatoError::ConnectionError { .. }
            | RustyPotatoError::ConfigError { .. }
            | RustyPotatoError::StorageError { .. } => ErrorSeverity::High,

            RustyPotatoError::PersistenceError { .. }
            | RustyPotatoError::ResourceExhaustion { .. }
            | RustyPotatoError::InternalError { .. } => ErrorSeverity::Critical,
        }
    }

    /// Get the error category for routing and handling
    pub fn category(&self) -> ErrorCategory {
        match self {
            RustyPotatoError::InvalidCommand { .. }
            | RustyPotatoError::WrongArity { .. }
            | RustyPotatoError::NotAnInteger { .. }
            | RustyPotatoError::IntegerOverflow
            | RustyPotatoError::KeyNotFound { .. }
            | RustyPotatoError::AuthenticationError { .. }
            | RustyPotatoError::AuthorizationError { .. } => ErrorCategory::Client,

            RustyPotatoError::NetworkError { .. }
            | RustyPotatoError::ConnectionError { .. }
            | RustyPotatoError::TimeoutError { .. } => ErrorCategory::Network,

            RustyPotatoError::StorageError { .. } | RustyPotatoError::PersistenceError { .. } => {
                ErrorCategory::Storage
            }

            RustyPotatoError::ConfigError { .. } => ErrorCategory::Configuration,

            RustyPotatoError::ProtocolError { .. } => ErrorCategory::Protocol,

            RustyPotatoError::ResourceExhaustion { .. }
            | RustyPotatoError::SerializationError { .. }
            | RustyPotatoError::InternalError { .. } => ErrorCategory::System,
        }
    }

    /// Check if the error is recoverable
    pub fn is_recoverable(&self) -> bool {
        match self {
            RustyPotatoError::PersistenceError { recoverable, .. } => *recoverable,
            RustyPotatoError::NetworkError { .. }
            | RustyPotatoError::ConnectionError { .. }
            | RustyPotatoError::TimeoutError { .. }
            | RustyPotatoError::StorageError { .. } => true,
            RustyPotatoError::ResourceExhaustion { .. } => true,
            _ => false,
        }
    }

    /// Convert error to client-facing error message
    pub fn to_client_error(&self) -> String {
        match self {
            RustyPotatoError::InvalidCommand { command, .. } => {
                format!("ERR unknown command '{command}'")
            }
            RustyPotatoError::WrongArity {
                command,
                expected,
                actual,
            } => {
                format!(
                    "ERR wrong number of arguments for '{command}' command (expected {expected}, got {actual})"
                )
            }
            RustyPotatoError::NotAnInteger { value } => {
                format!("ERR value is not an integer or out of range: {value}")
            }
            RustyPotatoError::IntegerOverflow => {
                // Exact wording Redis uses; clients pattern-match this.
                "ERR increment or decrement would overflow".to_string()
            }
            RustyPotatoError::KeyNotFound { key } => {
                format!("ERR no such key: {key}")
            }
            RustyPotatoError::ProtocolError { message, .. } => {
                format!("ERR protocol error: {message}")
            }
            RustyPotatoError::AuthenticationError { message, .. } => {
                format!("ERR authentication failed: {message}")
            }
            RustyPotatoError::AuthorizationError { message, .. } => {
                format!("ERR authorization failed: {message}")
            }
            RustyPotatoError::TimeoutError { message, .. } => {
                format!("ERR timeout: {message}")
            }
            RustyPotatoError::ResourceExhaustion { message, .. } => {
                format!("ERR resource exhausted: {message}")
            }
            RustyPotatoError::StorageError { message, .. } => {
                // Pass through the message directly for WRONGTYPE errors
                message.clone()
            }
            _ => "ERR internal server error".to_string(),
        }
    }

    /// Check if error should be logged as warning vs error
    pub fn is_client_error(&self) -> bool {
        matches!(self.category(), ErrorCategory::Client)
    }

    /// Log the error with appropriate level and context
    #[instrument(skip(self), fields(
        error_type = %std::any::type_name::<Self>(),
        severity = ?self.severity(),
        category = ?self.category(),
        recoverable = %self.is_recoverable()
    ))]
    pub fn log(&self) {
        let span = Span::current();

        match self.severity() {
            ErrorSeverity::Critical => {
                error!(
                    error = %self,
                    category = ?self.category(),
                    recoverable = %self.is_recoverable(),
                    "Critical error occurred"
                );
            }
            ErrorSeverity::High => {
                error!(
                    error = %self,
                    category = ?self.category(),
                    recoverable = %self.is_recoverable(),
                    "High severity error occurred"
                );
            }
            ErrorSeverity::Medium => {
                warn!(
                    error = %self,
                    category = ?self.category(),
                    recoverable = %self.is_recoverable(),
                    "Medium severity error occurred"
                );
            }
            ErrorSeverity::Low => {
                debug!(
                    error = %self,
                    category = ?self.category(),
                    "Low severity error occurred"
                );
            }
        }

        // Add error-specific context
        match self {
            RustyPotatoError::NetworkError {
                connection_id: Some(conn_id),
                ..
            } => {
                span.record("connection_id", conn_id);
            }
            RustyPotatoError::ConnectionError {
                connection_id: Some(conn_id),
                ..
            } => {
                span.record("connection_id", conn_id);
            }
            RustyPotatoError::ConfigError {
                config_key: Some(key),
                ..
            } => {
                span.record("config_key", key);
            }
            RustyPotatoError::StorageError {
                operation: Some(op),
                ..
            } => {
                span.record("storage_operation", op);
            }
            RustyPotatoError::TimeoutError {
                timeout_duration: Some(duration),
                ..
            } => {
                span.record("timeout_duration_ms", duration.as_millis() as u64);
            }
            RustyPotatoError::ResourceExhaustion {
                resource_type,
                current_usage,
                limit,
                ..
            } => {
                span.record("resource_type", resource_type);
                if let Some(usage) = current_usage {
                    span.record("current_usage", usage);
                }
                if let Some(limit) = limit {
                    span.record("limit", limit);
                }
            }
            _ => {}
        }
    }
}

// Standard error conversions for common system errors
impl From<std::io::Error> for RustyPotatoError {
    fn from(error: std::io::Error) -> Self {
        RustyPotatoError::PersistenceError {
            message: error.to_string(),
            source: Some(error),
            recoverable: true,
        }
    }
}

impl From<tokio::time::error::Elapsed> for RustyPotatoError {
    fn from(_error: tokio::time::error::Elapsed) -> Self {
        RustyPotatoError::TimeoutError {
            message: "Operation timed out".to_string(),
            operation: None,
            timeout_duration: None,
        }
    }
}

impl From<serde_json::Error> for RustyPotatoError {
    fn from(error: serde_json::Error) -> Self {
        RustyPotatoError::SerializationError {
            message: error.to_string(),
            source: Some(Box::new(error)),
        }
    }
}

impl From<std::num::ParseIntError> for RustyPotatoError {
    fn from(error: std::num::ParseIntError) -> Self {
        RustyPotatoError::NotAnInteger {
            value: error.to_string(),
        }
    }
}

impl From<std::string::FromUtf8Error> for RustyPotatoError {
    fn from(error: std::string::FromUtf8Error) -> Self {
        RustyPotatoError::ProtocolError {
            message: format!("Invalid UTF-8 sequence: {error}"),
            command: None,
            source: Some(Box::new(error)),
        }
    }
}
