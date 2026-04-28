//! Structured logging infrastructure for RustyPotato
//!
//! This module provides comprehensive logging setup with tracing subscriber,
//! structured output formats, log rotation integration, and performance monitoring.

use crate::config::{Config, LogFormat};
use crate::error::{Result, RustyPotatoError};
use crate::monitoring::LogRotationManager;
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info, warn, Level};
use tracing_subscriber::{
    fmt::{self, format::FmtSpan, time::ChronoUtc},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter, Registry,
};

/// Logging system manager that handles structured logging setup and configuration
#[derive(Debug)]
pub struct LoggingSystem {
    config: Config,
    log_rotation: Option<Arc<LogRotationManager>>,
}

impl LoggingSystem {
    /// Create a new logging system with the given configuration
    pub fn new(config: Config) -> Self {
        Self {
            config,
            log_rotation: None,
        }
    }

    /// Initialize the logging system with tracing subscriber
    pub async fn initialize(&mut self) -> Result<()> {
        info!(
            "Initializing logging system with level: {}",
            self.config.logging.level
        );

        // Parse log level
        let log_level = self.parse_log_level(&self.config.logging.level)?;

        // Create environment filter
        let env_filter = EnvFilter::builder()
            .with_default_directive(log_level.into())
            .from_env_lossy()
            .add_directive("rustypotato=trace".parse().unwrap())
            .add_directive("tokio=info".parse().unwrap())
            .add_directive("hyper=info".parse().unwrap());

        // Set up log rotation if file logging is enabled
        if let Some(ref log_file_path) = self.config.logging.file_path {
            let rotation_config = crate::monitoring::LogRotationConfig {
                log_file_path: log_file_path.clone(),
                rotation_policy: crate::monitoring::RotationPolicy::SizeOrDaily {
                    size_bytes: 100 * 1024 * 1024, // 100MB
                    hour: 0,                       // Midnight
                },
                max_files: 7,
                compress: true,
                rotation_dir: None,
            };

            let log_rotation_manager = Arc::new(LogRotationManager::new(rotation_config));
            log_rotation_manager.start().await?;
            self.log_rotation = Some(log_rotation_manager);
        }

        // Create the appropriate formatter based on configuration
        match (&self.config.logging.format, &self.config.logging.file_path) {
            (LogFormat::Json, Some(file_path)) => {
                self.setup_json_file_logging(env_filter, file_path).await?;
            }
            (LogFormat::Json, None) => {
                self.setup_json_console_logging(env_filter).await?;
            }
            (LogFormat::Pretty, Some(file_path)) => {
                self.setup_pretty_file_logging(env_filter, file_path)
                    .await?;
            }
            (LogFormat::Pretty, None) => {
                self.setup_pretty_console_logging(env_filter).await?;
            }
            (LogFormat::Compact, Some(file_path)) => {
                self.setup_compact_file_logging(env_filter, file_path)
                    .await?;
            }
            (LogFormat::Compact, None) => {
                self.setup_compact_console_logging(env_filter).await?;
            }
        }

        info!("Logging system initialized successfully");
        Ok(())
    }

    /// Get the log rotation manager if available
    pub fn log_rotation_manager(&self) -> Option<Arc<LogRotationManager>> {
        self.log_rotation.clone()
    }

    /// Parse log level string to tracing Level
    fn parse_log_level(&self, level_str: &str) -> Result<Level> {
        match level_str.to_lowercase().as_str() {
            "trace" => Ok(Level::TRACE),
            "debug" => Ok(Level::DEBUG),
            "info" => Ok(Level::INFO),
            "warn" => Ok(Level::WARN),
            "error" => Ok(Level::ERROR),
            _ => Err(RustyPotatoError::ConfigError {
                message: format!("Invalid log level: {level_str}"),
                config_key: Some("logging.level".to_string()),
                source: None,
            }),
        }
    }

    /// Set up JSON format logging to console
    async fn setup_json_console_logging(&self, env_filter: EnvFilter) -> Result<()> {
        let subscriber = Registry::default().with(env_filter).with(
            fmt::layer()
                .json()
                .with_current_span(false)
                .with_span_list(true)
                .with_timer(ChronoUtc::rfc_3339())
                .with_target(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_file(true)
                .with_line_number(true),
        );

        if let Err(e) = subscriber.try_init() {
            warn!(
                "Failed to initialize tracing subscriber (may already be set): {}",
                e
            );
        }
        Ok(())
    }

    /// Set up JSON format logging to file
    async fn setup_json_file_logging(&self, env_filter: EnvFilter, file_path: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                RustyPotatoError::InternalError {
                    message: format!("Failed to create log directory: {e}"),
                    component: Some("logging".to_string()),
                    source: Some(Box::new(e)),
                }
            })?;
        }

        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)
            .map_err(|e| RustyPotatoError::InternalError {
                message: format!("Failed to open log file: {e}"),
                component: Some("logging".to_string()),
                source: Some(Box::new(e)),
            })?;

        let subscriber = Registry::default().with(env_filter).with(
            fmt::layer()
                .json()
                .with_writer(Arc::new(file))
                .with_current_span(false)
                .with_span_list(true)
                .with_timer(ChronoUtc::rfc_3339())
                .with_target(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_file(true)
                .with_line_number(true),
        );

        if let Err(e) = subscriber.try_init() {
            warn!(
                "Failed to initialize tracing subscriber (may already be set): {}",
                e
            );
        }
        Ok(())
    }

    /// Set up pretty format logging to console
    async fn setup_pretty_console_logging(&self, env_filter: EnvFilter) -> Result<()> {
        let subscriber = Registry::default().with(env_filter).with(
            fmt::layer()
                .pretty()
                .with_timer(ChronoUtc::rfc_3339())
                .with_target(true)
                .with_thread_ids(false)
                .with_thread_names(false)
                .with_file(false)
                .with_line_number(false)
                .with_span_events(FmtSpan::CLOSE),
        );

        if let Err(e) = subscriber.try_init() {
            warn!(
                "Failed to initialize tracing subscriber (may already be set): {}",
                e
            );
        }
        Ok(())
    }

    /// Set up pretty format logging to file
    async fn setup_pretty_file_logging(
        &self,
        env_filter: EnvFilter,
        file_path: &Path,
    ) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                RustyPotatoError::InternalError {
                    message: format!("Failed to create log directory: {e}"),
                    component: Some("logging".to_string()),
                    source: Some(Box::new(e)),
                }
            })?;
        }

        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)
            .map_err(|e| RustyPotatoError::InternalError {
                message: format!("Failed to open log file: {e}"),
                component: Some("logging".to_string()),
                source: Some(Box::new(e)),
            })?;

        let subscriber = Registry::default().with(env_filter).with(
            fmt::layer()
                .with_writer(Arc::new(file))
                .with_timer(ChronoUtc::rfc_3339())
                .with_target(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_file(true)
                .with_line_number(true)
                .with_span_events(FmtSpan::CLOSE),
        );

        if let Err(e) = subscriber.try_init() {
            warn!(
                "Failed to initialize tracing subscriber (may already be set): {}",
                e
            );
        }
        Ok(())
    }

    /// Set up compact format logging to console
    async fn setup_compact_console_logging(&self, env_filter: EnvFilter) -> Result<()> {
        let subscriber = Registry::default().with(env_filter).with(
            fmt::layer()
                .compact()
                .with_timer(ChronoUtc::rfc_3339())
                .with_target(false)
                .with_thread_ids(false)
                .with_thread_names(false)
                .with_file(false)
                .with_line_number(false),
        );

        if let Err(e) = subscriber.try_init() {
            warn!(
                "Failed to initialize tracing subscriber (may already be set): {}",
                e
            );
        }
        Ok(())
    }

    /// Set up compact format logging to file
    async fn setup_compact_file_logging(
        &self,
        env_filter: EnvFilter,
        file_path: &Path,
    ) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                RustyPotatoError::InternalError {
                    message: format!("Failed to create log directory: {e}"),
                    component: Some("logging".to_string()),
                    source: Some(Box::new(e)),
                }
            })?;
        }

        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)
            .map_err(|e| RustyPotatoError::InternalError {
                message: format!("Failed to open log file: {e}"),
                component: Some("logging".to_string()),
                source: Some(Box::new(e)),
            })?;

        let subscriber = Registry::default().with(env_filter).with(
            fmt::layer()
                .compact()
                .with_writer(Arc::new(file))
                .with_timer(ChronoUtc::rfc_3339())
                .with_target(true)
                .with_thread_ids(false)
                .with_thread_names(false)
                .with_file(false)
                .with_line_number(false),
        );

        if let Err(e) = subscriber.try_init() {
            warn!(
                "Failed to initialize tracing subscriber (may already be set): {}",
                e
            );
        }
        Ok(())
    }

    /// Flush any pending log messages
    pub async fn flush(&self) -> Result<()> {
        // For file-based logging, we might want to flush the file
        // This is a placeholder for more advanced flushing logic
        if let Some(log_rotation) = &self.log_rotation {
            // The log rotation manager handles flushing
            let _status = log_rotation.get_status().await;
        }
        Ok(())
    }

    /// Shutdown the logging system gracefully
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down logging system");

        // Flush any pending logs
        self.flush().await?;

        // If we have log rotation, we might want to do a final rotation
        if let Some(log_rotation) = &self.log_rotation {
            // Optionally trigger a final rotation on shutdown
            // log_rotation.rotate_now().await?;
            let _status = log_rotation.get_status().await;
        }

        info!("Logging system shutdown complete");
        Ok(())
    }
}

/// Health check for logging system
pub async fn check_logging_health(config: &Config) -> Result<bool> {
    // Check if we can write to the log file if file logging is enabled
    if let Some(ref log_file_path) = config.logging.file_path {
        // Try to open the log file for writing
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file_path)
        {
            Ok(_) => Ok(true),
            Err(e) => {
                error!("Logging health check failed: {}", e);
                Ok(false)
            }
        }
    } else {
        // Console logging is always healthy if we can get here
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_parse_log_level() {
        let config = Config::default();
        let logging_system = LoggingSystem::new(config);

        assert!(matches!(
            logging_system.parse_log_level("trace"),
            Ok(Level::TRACE)
        ));
        assert!(matches!(
            logging_system.parse_log_level("debug"),
            Ok(Level::DEBUG)
        ));
        assert!(matches!(
            logging_system.parse_log_level("info"),
            Ok(Level::INFO)
        ));
        assert!(matches!(
            logging_system.parse_log_level("warn"),
            Ok(Level::WARN)
        ));
        assert!(matches!(
            logging_system.parse_log_level("error"),
            Ok(Level::ERROR)
        ));

        // Test case insensitive
        assert!(matches!(
            logging_system.parse_log_level("INFO"),
            Ok(Level::INFO)
        ));
        assert!(matches!(
            logging_system.parse_log_level("Error"),
            Ok(Level::ERROR)
        ));

        // Test invalid level
        assert!(logging_system.parse_log_level("invalid").is_err());
    }

    #[tokio::test]
    async fn test_logging_system_creation() {
        let config = Config::default();
        let logging_system = LoggingSystem::new(config);

        assert!(logging_system.log_rotation.is_none());
    }

    #[tokio::test]
    async fn test_logging_health_check_console() {
        let config = Config::default();
        let health = check_logging_health(&config).await.unwrap();
        assert!(health);
    }

    #[tokio::test]
    async fn test_logging_health_check_file() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.log");

        let mut config = Config::default();
        config.logging.file_path = Some(log_path);

        let health = check_logging_health(&config).await.unwrap();
        assert!(health);
    }

    #[tokio::test]
    async fn test_logging_health_check_invalid_file() {
        let mut config = Config::default();
        config.logging.file_path = Some(PathBuf::from("/invalid/path/test.log"));

        let health = check_logging_health(&config).await.unwrap();
        assert!(!health);
    }
}
