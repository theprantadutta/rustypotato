//! Error types and handling for RustyPotato
//!
//! This module defines all error types used throughout the system,
//! provides conversion utilities for client responses, implements
//! structured logging, and includes error recovery mechanisms.

use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, error, info, instrument, warn, Span};

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
                format!("ERR unknown command '{}'", command)
            }
            RustyPotatoError::WrongArity {
                command,
                expected,
                actual,
            } => {
                format!(
                    "ERR wrong number of arguments for '{}' command (expected {}, got {})",
                    command, expected, actual
                )
            }
            RustyPotatoError::NotAnInteger { value } => {
                format!("ERR value is not an integer or out of range: {}", value)
            }
            RustyPotatoError::KeyNotFound { key } => {
                format!("ERR no such key: {}", key)
            }
            RustyPotatoError::ProtocolError { message, .. } => {
                format!("ERR protocol error: {}", message)
            }
            RustyPotatoError::AuthenticationError { message, .. } => {
                format!("ERR authentication failed: {}", message)
            }
            RustyPotatoError::AuthorizationError { message, .. } => {
                format!("ERR authorization failed: {}", message)
            }
            RustyPotatoError::TimeoutError { message, .. } => {
                format!("ERR timeout: {}", message)
            }
            RustyPotatoError::ResourceExhaustion { message, .. } => {
                format!("ERR resource exhausted: {}", message)
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

/// Error recovery strategies for different error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryStrategy {
    /// No recovery possible, fail immediately
    None,
    /// Retry the operation with exponential backoff
    Retry,
    /// Use a fallback mechanism
    Fallback,
    /// Gracefully degrade functionality
    Degrade,
    /// Circuit breaker pattern
    CircuitBreaker,
}

/// Recovery configuration for different error scenarios
#[derive(Debug, Clone)]
pub struct RecoveryConfig {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_multiplier: f64,
    pub circuit_breaker_threshold: u32,
    pub circuit_breaker_timeout: Duration,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            circuit_breaker_threshold: 5,
            circuit_breaker_timeout: Duration::from_secs(60),
        }
    }
}

/// Error recovery manager that implements various recovery strategies
#[derive(Debug)]
pub struct ErrorRecoveryManager {
    config: RecoveryConfig,
    circuit_breaker_state: std::sync::Arc<std::sync::RwLock<CircuitBreakerState>>,
}

#[derive(Debug, Clone)]
struct CircuitBreakerState {
    failure_count: u32,
    last_failure_time: Option<Instant>,
    state: CircuitState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl ErrorRecoveryManager {
    /// Create a new error recovery manager with default configuration
    pub fn new() -> Self {
        Self::with_config(RecoveryConfig::default())
    }

    /// Create a new error recovery manager with custom configuration
    pub fn with_config(config: RecoveryConfig) -> Self {
        Self {
            config,
            circuit_breaker_state: std::sync::Arc::new(std::sync::RwLock::new(
                CircuitBreakerState {
                    failure_count: 0,
                    last_failure_time: None,
                    state: CircuitState::Closed,
                },
            )),
        }
    }

    /// Get the recommended recovery strategy for an error
    pub fn get_recovery_strategy(&self, error: &RustyPotatoError) -> RecoveryStrategy {
        match error {
            RustyPotatoError::NetworkError { .. }
            | RustyPotatoError::ConnectionError { .. }
            | RustyPotatoError::TimeoutError { .. } => RecoveryStrategy::Retry,

            RustyPotatoError::PersistenceError {
                recoverable: true, ..
            }
            | RustyPotatoError::StorageError { .. } => RecoveryStrategy::Retry,

            RustyPotatoError::ResourceExhaustion { .. } => RecoveryStrategy::Degrade,

            RustyPotatoError::ConfigError { .. } => RecoveryStrategy::Fallback,

            RustyPotatoError::InvalidCommand { .. }
            | RustyPotatoError::WrongArity { .. }
            | RustyPotatoError::NotAnInteger { .. }
            | RustyPotatoError::KeyNotFound { .. }
            | RustyPotatoError::AuthenticationError { .. }
            | RustyPotatoError::AuthorizationError { .. } => RecoveryStrategy::None,

            _ => RecoveryStrategy::CircuitBreaker,
        }
    }

    /// Execute an operation with retry logic
    #[instrument(skip(self, operation), fields(attempt = 1))]
    pub async fn retry_operation<T, F, Fut>(&self, mut operation: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut attempt = 1;
        let mut delay = self.config.initial_delay;

        loop {
            let span = Span::current();
            span.record("attempt", attempt);

            match operation().await {
                Ok(result) => {
                    if attempt > 1 {
                        info!(attempts = attempt, "Operation succeeded after retry");
                    }
                    return Ok(result);
                }
                Err(error) => {
                    error.log();

                    if attempt >= self.config.max_retries || !error.is_recoverable() {
                        error!(
                            attempts = attempt,
                            max_retries = self.config.max_retries,
                            recoverable = error.is_recoverable(),
                            "Operation failed after all retry attempts"
                        );
                        return Err(error);
                    }

                    warn!(
                        attempt = attempt,
                        delay_ms = delay.as_millis(),
                        "Operation failed, retrying after delay"
                    );

                    tokio::time::sleep(delay).await;

                    // Exponential backoff with jitter
                    delay = std::cmp::min(
                        Duration::from_millis(
                            (delay.as_millis() as f64 * self.config.backoff_multiplier) as u64,
                        ),
                        self.config.max_delay,
                    );

                    attempt += 1;
                }
            }
        }
    }

    /// Check if circuit breaker allows operation
    pub fn circuit_breaker_check(&self) -> Result<()> {
        let state = self.circuit_breaker_state.read().unwrap();

        match state.state {
            CircuitState::Closed => Ok(()),
            CircuitState::Open => {
                if let Some(last_failure) = state.last_failure_time {
                    if last_failure.elapsed() > self.config.circuit_breaker_timeout {
                        drop(state);
                        let mut state = self.circuit_breaker_state.write().unwrap();
                        state.state = CircuitState::HalfOpen;
                        info!("Circuit breaker transitioning to half-open state");
                        Ok(())
                    } else {
                        Err(RustyPotatoError::InternalError {
                            message: "Circuit breaker is open".to_string(),
                            component: Some("error_recovery".to_string()),
                            source: None,
                        })
                    }
                } else {
                    Ok(())
                }
            }
            CircuitState::HalfOpen => Ok(()),
        }
    }

    /// Record operation success for circuit breaker
    pub fn record_success(&self) {
        let mut state = self.circuit_breaker_state.write().unwrap();
        if state.state == CircuitState::HalfOpen {
            state.state = CircuitState::Closed;
            state.failure_count = 0;
            info!("Circuit breaker closed after successful operation");
        }
    }

    /// Record operation failure for circuit breaker
    pub fn record_failure(&self) {
        let mut state = self.circuit_breaker_state.write().unwrap();
        state.failure_count += 1;
        state.last_failure_time = Some(Instant::now());

        if state.failure_count >= self.config.circuit_breaker_threshold {
            state.state = CircuitState::Open;
            warn!(
                failure_count = state.failure_count,
                threshold = self.config.circuit_breaker_threshold,
                "Circuit breaker opened due to repeated failures"
            );
        }
    }
}

impl Default for ErrorRecoveryManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience macro for handling errors with automatic logging
#[macro_export]
macro_rules! handle_error {
    ($result:expr) => {
        match $result {
            Ok(value) => value,
            Err(error) => {
                error.log();
                return Err(error);
            }
        }
    };
    ($result:expr, $recovery:expr) => {
        match $result {
            Ok(value) => value,
            Err(error) => {
                error.log();
                return $recovery;
            }
        }
    };
}

/// Convenience macro for retrying operations
#[macro_export]
macro_rules! retry_operation {
    ($recovery_manager:expr, $operation:expr) => {
        $recovery_manager
            .retry_operation(|| async { $operation })
            .await
    };
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
            message: format!("Invalid UTF-8 sequence: {}", error),
            command: None,
            source: Some(Box::new(error)),
        }
    }
}
