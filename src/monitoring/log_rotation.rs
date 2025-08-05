//! Log rotation management for production deployments
//!
//! This module provides automatic log rotation, compression, and cleanup
//! to manage log files in production environments.

use crate::error::{Result, RustyPotatoError};
use chrono::{TimeZone, Timelike};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Log rotation manager
#[derive(Debug)]
pub struct LogRotationManager {
    config: LogRotationConfig,
    status: RwLock<LogRotationStatus>,
}

/// Configuration for log rotation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRotationConfig {
    /// Log file path to rotate
    pub log_file_path: PathBuf,
    /// Rotation policy
    pub rotation_policy: RotationPolicy,
    /// Maximum number of rotated files to keep
    pub max_files: usize,
    /// Whether to compress rotated files
    pub compress: bool,
    /// Directory to store rotated files (defaults to same as log file)
    pub rotation_dir: Option<PathBuf>,
}

/// Log rotation policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RotationPolicy {
    /// Rotate when file size exceeds the specified bytes
    Size(u64),
    /// Rotate daily at specified hour (0-23)
    Daily(u8),
    /// Rotate hourly
    Hourly,
    /// Rotate when file size exceeds bytes OR daily
    SizeOrDaily { size_bytes: u64, hour: u8 },
    /// Manual rotation only
    Manual,
}

/// Status of log rotation system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRotationStatus {
    pub last_rotation: Option<String>,
    pub next_rotation: Option<String>,
    pub current_file_size: u64,
    pub rotated_files_count: usize,
    pub total_rotated_size: u64,
    pub errors: Vec<String>,
}

impl LogRotationManager {
    /// Create a new log rotation manager
    pub fn new(config: LogRotationConfig) -> Self {
        Self {
            config,
            status: RwLock::new(LogRotationStatus {
                last_rotation: None,
                next_rotation: None,
                current_file_size: 0,
                rotated_files_count: 0,
                total_rotated_size: 0,
                errors: Vec::new(),
            }),
        }
    }

    /// Start the log rotation background task
    pub async fn start(&self) -> Result<()> {
        info!(
            "Starting log rotation manager with policy: {:?}",
            self.config.rotation_policy
        );

        // Initial status update
        self.update_status().await?;

        // Start background rotation task
        let config = self.config.clone();
        let status = Arc::new(RwLock::new(LogRotationStatus {
            last_rotation: None,
            next_rotation: None,
            current_file_size: 0,
            rotated_files_count: 0,
            total_rotated_size: 0,
            errors: Vec::new(),
        }));

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60)); // Check every minute

            loop {
                interval.tick().await;

                if let Err(e) = Self::check_and_rotate(&config, &status).await {
                    error!("Log rotation check failed: {}", e);

                    // Add error to status
                    let mut status_guard = status.write().await;
                    status_guard
                        .errors
                        .push(format!("{}: {}", chrono::Utc::now().to_rfc3339(), e));

                    // Keep only last 10 errors
                    if status_guard.errors.len() > 10 {
                        status_guard.errors.remove(0);
                    }
                }
            }
        });

        Ok(())
    }

    /// Manually trigger log rotation
    pub async fn rotate_now(&self) -> Result<()> {
        info!("Manual log rotation triggered");
        Self::perform_rotation(&self.config, &self.status).await
    }

    /// Get current rotation status
    pub async fn get_status(&self) -> LogRotationStatus {
        // Update status first to get accurate counts
        let _ = self.update_status().await;

        let status_guard = self.status.read().await;
        status_guard.clone()
    }

    /// Update status information
    async fn update_status(&self) -> Result<()> {
        let mut status = self.status.write().await;

        // Update current file size
        if let Ok(metadata) = fs::metadata(&self.config.log_file_path).await {
            status.current_file_size = metadata.len();
        }

        // Count rotated files and calculate total size
        let rotation_dir = self.get_rotation_dir();
        if let Ok(mut entries) = fs::read_dir(&rotation_dir).await {
            let mut count = 0;
            let mut total_size = 0;

            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(file_name) = entry.file_name().into_string() {
                    if self.is_rotated_file(&file_name) {
                        count += 1;
                        if let Ok(metadata) = entry.metadata().await {
                            total_size += metadata.len();
                        }
                    }
                }
            }

            status.rotated_files_count = count;
            status.total_rotated_size = total_size;
        }

        // Calculate next rotation time based on policy
        status.next_rotation = self.calculate_next_rotation().map(|t| t.to_rfc3339());

        Ok(())
    }

    /// Check if rotation is needed and perform it
    async fn check_and_rotate(
        config: &LogRotationConfig,
        status: &Arc<RwLock<LogRotationStatus>>,
    ) -> Result<()> {
        let should_rotate = match &config.rotation_policy {
            RotationPolicy::Size(max_size) => {
                if let Ok(metadata) = fs::metadata(&config.log_file_path).await {
                    metadata.len() > *max_size
                } else {
                    false
                }
            }
            RotationPolicy::Daily(hour) => Self::should_rotate_daily(*hour, status).await,
            RotationPolicy::Hourly => Self::should_rotate_hourly(status).await,
            RotationPolicy::SizeOrDaily { size_bytes, hour } => {
                let size_exceeded = if let Ok(metadata) = fs::metadata(&config.log_file_path).await
                {
                    metadata.len() > *size_bytes
                } else {
                    false
                };

                let time_exceeded = Self::should_rotate_daily(*hour, status).await;

                size_exceeded || time_exceeded
            }
            RotationPolicy::Manual => false,
        };

        if should_rotate {
            Self::perform_rotation(config, status).await?;
        }

        Ok(())
    }

    /// Check if daily rotation is needed
    async fn should_rotate_daily(hour: u8, status: &RwLock<LogRotationStatus>) -> bool {
        let now = chrono::Utc::now();
        let current_hour = now.hour() as u8;

        // Only rotate at the specified hour
        if current_hour != hour {
            return false;
        }

        let status_guard = status.read().await;

        // Check if we already rotated today
        if let Some(last_rotation_str) = &status_guard.last_rotation {
            if let Ok(last_rotation) = chrono::DateTime::parse_from_rfc3339(last_rotation_str) {
                let last_rotation_utc = last_rotation.with_timezone(&chrono::Utc);

                // If last rotation was today, don't rotate again
                if last_rotation_utc.date_naive() == now.date_naive() {
                    return false;
                }
            }
        }

        true
    }

    /// Check if hourly rotation is needed
    async fn should_rotate_hourly(status: &RwLock<LogRotationStatus>) -> bool {
        let now = chrono::Utc::now();
        let status_guard = status.read().await;

        // Check if we already rotated this hour
        if let Some(last_rotation_str) = &status_guard.last_rotation {
            if let Ok(last_rotation) = chrono::DateTime::parse_from_rfc3339(last_rotation_str) {
                let last_rotation_utc = last_rotation.with_timezone(&chrono::Utc);

                // If last rotation was this hour, don't rotate again
                if last_rotation_utc.date_naive() == now.date_naive()
                    && last_rotation_utc.hour() == now.hour()
                {
                    return false;
                }
            }
        }

        true
    }

    /// Perform the actual log rotation
    async fn perform_rotation(
        config: &LogRotationConfig,
        status: &RwLock<LogRotationStatus>,
    ) -> Result<()> {
        let log_path = &config.log_file_path;

        // Check if log file exists
        if !log_path.exists() {
            debug!(
                "Log file does not exist, skipping rotation: {}",
                log_path.display()
            );
            return Ok(());
        }

        // Generate rotated file name
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let rotation_dir = if let Some(dir) = &config.rotation_dir {
            dir.clone()
        } else {
            log_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        };

        // Ensure rotation directory exists
        fs::create_dir_all(&rotation_dir)
            .await
            .map_err(|e| RustyPotatoError::InternalError {
                message: format!("Failed to create rotation directory: {}", e),
                component: Some("log_rotation".to_string()),
                source: Some(Box::new(e)),
            })?;

        let log_file_name = log_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("rustypotato.log");

        let rotated_name = format!("{}.{}", log_file_name, timestamp);
        let rotated_path = rotation_dir.join(&rotated_name);

        // Move current log file to rotated name
        fs::rename(log_path, &rotated_path)
            .await
            .map_err(|e| RustyPotatoError::InternalError {
                message: format!("Failed to rotate log file: {}", e),
                component: Some("log_rotation".to_string()),
                source: Some(Box::new(e)),
            })?;

        info!(
            "Log file rotated: {} -> {}",
            log_path.display(),
            rotated_path.display()
        );

        // Compress if enabled
        if config.compress {
            if let Err(e) = Self::compress_file(&rotated_path).await {
                warn!("Failed to compress rotated log file: {}", e);
            }
        }

        // Clean up old files
        Self::cleanup_old_files(config, &rotation_dir).await?;

        // Update status
        {
            let mut status_guard = status.write().await;
            status_guard.last_rotation = Some(chrono::Utc::now().to_rfc3339());
            status_guard.current_file_size = 0; // New log file will be empty
        }

        Ok(())
    }

    /// Compress a rotated log file
    async fn compress_file(file_path: &Path) -> Result<()> {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::prelude::*;

        let input_data =
            fs::read(file_path)
                .await
                .map_err(|e| RustyPotatoError::InternalError {
                    message: format!("Failed to read file for compression: {}", e),
                    component: Some("log_rotation".to_string()),
                    source: Some(Box::new(e)),
                })?;

        let compressed_path = file_path.with_extension("log.gz");

        // Compress the data
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&input_data)
            .map_err(|e| RustyPotatoError::InternalError {
                message: format!("Failed to compress data: {}", e),
                component: Some("log_rotation".to_string()),
                source: Some(Box::new(e)),
            })?;

        let compressed_data = encoder
            .finish()
            .map_err(|e| RustyPotatoError::InternalError {
                message: format!("Failed to finish compression: {}", e),
                component: Some("log_rotation".to_string()),
                source: Some(Box::new(e)),
            })?;

        // Write compressed file
        fs::write(&compressed_path, compressed_data)
            .await
            .map_err(|e| RustyPotatoError::InternalError {
                message: format!("Failed to write compressed file: {}", e),
                component: Some("log_rotation".to_string()),
                source: Some(Box::new(e)),
            })?;

        // Remove original file
        fs::remove_file(file_path)
            .await
            .map_err(|e| RustyPotatoError::InternalError {
                message: format!("Failed to remove original file after compression: {}", e),
                component: Some("log_rotation".to_string()),
                source: Some(Box::new(e)),
            })?;

        debug!(
            "Compressed log file: {} -> {}",
            file_path.display(),
            compressed_path.display()
        );

        Ok(())
    }

    /// Clean up old rotated files
    async fn cleanup_old_files(config: &LogRotationConfig, rotation_dir: &Path) -> Result<()> {
        let mut rotated_files = Vec::new();

        // Collect all rotated files with their metadata
        let mut entries =
            fs::read_dir(rotation_dir)
                .await
                .map_err(|e| RustyPotatoError::InternalError {
                    message: format!("Failed to read rotation directory: {}", e),
                    component: Some("log_rotation".to_string()),
                    source: Some(Box::new(e)),
                })?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(file_name) = entry.file_name().into_string() {
                if Self::is_rotated_file_static(&file_name, &config.log_file_path) {
                    if let Ok(metadata) = entry.metadata().await {
                        if let Ok(modified) = metadata.modified() {
                            rotated_files.push((entry.path(), modified));
                        }
                    }
                }
            }
        }

        // Sort by modification time (oldest first)
        rotated_files.sort_by_key(|(_, modified)| *modified);

        // Remove excess files
        if rotated_files.len() > config.max_files {
            let files_to_remove = rotated_files.len() - config.max_files;

            for (file_path, _) in rotated_files.iter().take(files_to_remove) {
                if let Err(e) = fs::remove_file(file_path).await {
                    warn!(
                        "Failed to remove old rotated file {}: {}",
                        file_path.display(),
                        e
                    );
                } else {
                    debug!("Removed old rotated file: {}", file_path.display());
                }
            }
        }

        Ok(())
    }

    /// Check if a filename is a rotated log file
    fn is_rotated_file(&self, filename: &str) -> bool {
        Self::is_rotated_file_static(filename, &self.config.log_file_path)
    }

    /// Static version of is_rotated_file
    fn is_rotated_file_static(filename: &str, log_path: &Path) -> bool {
        let log_file_name = log_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("rustypotato.log");

        // Check for pattern: logfile.timestamp or logfile.timestamp.gz
        filename.starts_with(&format!("{}.", log_file_name))
            && (filename.ends_with(".gz")
                || filename
                    .chars()
                    .skip(log_file_name.len() + 1)
                    .all(|c| c.is_ascii_digit()))
    }

    /// Get the rotation directory
    fn get_rotation_dir(&self) -> PathBuf {
        if let Some(dir) = &self.config.rotation_dir {
            dir.clone()
        } else {
            self.config
                .log_file_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        }
    }

    /// Calculate next rotation time
    fn calculate_next_rotation(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        match &self.config.rotation_policy {
            RotationPolicy::Daily(hour) => {
                let now = chrono::Utc::now();
                let today = now.date_naive();
                let target_time = today.and_hms_opt(*hour as u32, 0, 0)?;
                let target_datetime = chrono::Utc.from_utc_datetime(&target_time);

                if target_datetime > now {
                    Some(target_datetime)
                } else {
                    // Next day
                    let tomorrow = today + chrono::Duration::days(1);
                    let target_time = tomorrow.and_hms_opt(*hour as u32, 0, 0)?;
                    Some(chrono::Utc.from_utc_datetime(&target_time))
                }
            }
            RotationPolicy::Hourly => {
                let now = chrono::Utc::now();
                let next_hour = now + chrono::Duration::hours(1);
                let next_hour_start = next_hour.date_naive().and_hms_opt(next_hour.hour(), 0, 0)?;
                Some(chrono::Utc.from_utc_datetime(&next_hour_start))
            }
            RotationPolicy::SizeOrDaily { hour, .. } => {
                // Use the daily calculation for time-based rotation
                let now = chrono::Utc::now();
                let today = now.date_naive();
                let target_time = today.and_hms_opt(*hour as u32, 0, 0)?;
                let target_datetime = chrono::Utc.from_utc_datetime(&target_time);

                if target_datetime > now {
                    Some(target_datetime)
                } else {
                    let tomorrow = today + chrono::Duration::days(1);
                    let target_time = tomorrow.and_hms_opt(*hour as u32, 0, 0)?;
                    Some(chrono::Utc.from_utc_datetime(&target_time))
                }
            }
            RotationPolicy::Size(_) | RotationPolicy::Manual => None,
        }
    }
}

impl Default for LogRotationConfig {
    fn default() -> Self {
        Self {
            log_file_path: PathBuf::from("rustypotato.log"),
            rotation_policy: RotationPolicy::SizeOrDaily {
                size_bytes: 100 * 1024 * 1024, // 100MB
                hour: 0,                       // Midnight
            },
            max_files: 7, // Keep 7 rotated files
            compress: true,
            rotation_dir: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_log_rotation_manager_creation() {
        let config = LogRotationConfig::default();
        let manager = LogRotationManager::new(config);

        let status = manager.get_status().await;
        assert_eq!(status.current_file_size, 0);
        assert_eq!(status.rotated_files_count, 0);
    }

    #[tokio::test]
    async fn test_is_rotated_file() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.log");

        let config = LogRotationConfig {
            log_file_path: log_path,
            ..Default::default()
        };

        let manager = LogRotationManager::new(config);

        assert!(manager.is_rotated_file("test.log.1234567890"));
        assert!(manager.is_rotated_file("test.log.1234567890.gz"));
        assert!(!manager.is_rotated_file("test.log"));
        assert!(!manager.is_rotated_file("other.log.1234567890"));
        assert!(!manager.is_rotated_file("test.log.abc"));
    }

    #[tokio::test]
    async fn test_size_based_rotation_policy() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.log");

        // Create a log file larger than the size limit
        let test_data = "x".repeat(1000);
        fs::write(&log_path, &test_data).await.unwrap();

        let config = LogRotationConfig {
            log_file_path: log_path.clone(),
            rotation_policy: RotationPolicy::Size(500), // 500 bytes limit
            max_files: 3,
            compress: false,
            rotation_dir: Some(temp_dir.path().to_path_buf()),
        };

        let manager = LogRotationManager::new(config);

        // Trigger rotation
        manager.rotate_now().await.unwrap();

        // Check that the original file was rotated
        assert!(!log_path.exists() || fs::metadata(&log_path).await.unwrap().len() == 0);

        // Check that a rotated file exists
        let mut entries = fs::read_dir(temp_dir.path()).await.unwrap();
        let mut found_rotated = false;

        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(name) = entry.file_name().into_string() {
                if name.starts_with("test.log.") && name != "test.log" {
                    found_rotated = true;
                    break;
                }
            }
        }

        assert!(found_rotated, "No rotated file found");
    }

    #[tokio::test]
    async fn test_manual_rotation_policy() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("manual.log");

        // Create a log file
        fs::write(&log_path, "test log content").await.unwrap();

        let config = LogRotationConfig {
            log_file_path: log_path.clone(),
            rotation_policy: RotationPolicy::Manual,
            max_files: 5,
            compress: false,
            rotation_dir: Some(temp_dir.path().to_path_buf()),
        };

        let manager = LogRotationManager::new(config);

        // Manual rotation should work
        manager.rotate_now().await.unwrap();

        // Check status
        let status = manager.get_status().await;
        assert!(status.last_rotation.is_some());
    }

    #[tokio::test]
    async fn test_status_update() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("status.log");

        // Create a log file
        let test_data = "test log content for status";
        fs::write(&log_path, &test_data).await.unwrap();

        let config = LogRotationConfig {
            log_file_path: log_path,
            rotation_policy: RotationPolicy::Manual,
            max_files: 5,
            compress: false,
            rotation_dir: Some(temp_dir.path().to_path_buf()),
        };

        let manager = LogRotationManager::new(config);

        let status = manager.get_status().await;
        assert_eq!(status.current_file_size, test_data.len() as u64);
        assert_eq!(status.rotated_files_count, 0);
    }

    #[test]
    fn test_rotation_policy_serialization() {
        let policies = vec![
            RotationPolicy::Size(1024),
            RotationPolicy::Daily(12),
            RotationPolicy::Hourly,
            RotationPolicy::SizeOrDaily {
                size_bytes: 1024,
                hour: 6,
            },
            RotationPolicy::Manual,
        ];

        for policy in policies {
            let json = serde_json::to_string(&policy).unwrap();
            let deserialized: RotationPolicy = serde_json::from_str(&json).unwrap();

            // Basic check that serialization/deserialization works
            match (&policy, &deserialized) {
                (RotationPolicy::Size(a), RotationPolicy::Size(b)) => assert_eq!(a, b),
                (RotationPolicy::Daily(a), RotationPolicy::Daily(b)) => assert_eq!(a, b),
                (RotationPolicy::Hourly, RotationPolicy::Hourly) => {}
                (
                    RotationPolicy::SizeOrDaily {
                        size_bytes: a1,
                        hour: a2,
                    },
                    RotationPolicy::SizeOrDaily {
                        size_bytes: b1,
                        hour: b2,
                    },
                ) => {
                    assert_eq!(a1, b1);
                    assert_eq!(a2, b2);
                }
                (RotationPolicy::Manual, RotationPolicy::Manual) => {}
                _ => panic!("Serialization/deserialization mismatch"),
            }
        }
    }
}
