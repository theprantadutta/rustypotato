//! Persistence layer with AOF (Append-Only File) support
//!
//! This module provides append-only file persistence for RustyPotato,
//! ensuring data durability and recovery capabilities.

use crate::config::{Config, FsyncPolicy};
use crate::error::{Result, RustyPotatoError};
use crate::storage::ValueType;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

/// AOF command entry for serialization
#[derive(Debug, Clone)]
pub struct AofEntry {
    pub timestamp: u64,
    pub command: AofCommand,
}

/// Commands that can be persisted to AOF
#[derive(Debug, Clone)]
pub enum AofCommand {
    Set {
        key: String,
        value: ValueType,
    },
    SetWithExpiration {
        key: String,
        value: ValueType,
        expires_at: u64,
    },
    Delete {
        key: String,
    },
    Expire {
        key: String,
        expires_at: u64,
    },
}

/// AOF writer for append-only file operations with batched writes
#[derive(Debug)]
pub struct AofWriter {
    file: Option<File>,
    buffer: Vec<u8>,
    batch_buffer: Vec<AofEntry>,
    flush_interval: Duration,
    fsync_policy: FsyncPolicy,
    file_path: PathBuf,
    last_flush: Instant,
    pending_writes: usize,
    max_batch_size: usize,
    max_buffer_size: usize,
}

impl AofWriter {
    /// Create a new AOF writer with configuration
    pub async fn new(config: &Config) -> Result<Self> {
        let mut writer = Self {
            file: None,
            buffer: Vec::with_capacity(65536), // 64KB initial buffer for batching
            batch_buffer: Vec::with_capacity(1000), // Batch up to 1000 entries
            flush_interval: Duration::from_secs(1),
            fsync_policy: config.storage.aof_fsync_policy.clone(),
            file_path: config.storage.aof_path.clone(),
            last_flush: Instant::now(),
            pending_writes: 0,
            max_batch_size: 1000,
            max_buffer_size: 1024 * 1024, // 1MB max buffer
        };

        if config.storage.aof_enabled {
            writer.open_file().await?;
        }

        Ok(writer)
    }

    /// Open the AOF file for writing
    async fn open_file(&mut self) -> Result<()> {
        // Create parent directories if they don't exist
        if let Some(parent) = self.file_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
            .await?;

        self.file = Some(file);
        info!("AOF file opened: {:?}", self.file_path);
        Ok(())
    }

    /// Write an AOF entry to the batch buffer
    pub async fn write_entry(&mut self, entry: AofEntry) -> Result<()> {
        if self.file.is_none() {
            return Ok(()); // AOF disabled
        }

        self.batch_buffer.push(entry);
        self.pending_writes += 1;

        // Check if we need to flush based on batch size or policy
        let should_flush = match self.fsync_policy {
            FsyncPolicy::Always => true,
            FsyncPolicy::EverySecond => {
                self.batch_buffer.len() >= self.max_batch_size
                    || self.last_flush.elapsed() >= self.flush_interval
            }
            FsyncPolicy::Never => {
                self.batch_buffer.len() >= self.max_batch_size
                    || self.buffer.len() > self.max_buffer_size
            }
        };

        if should_flush {
            self.flush_batch().await?;
        }

        Ok(())
    }

    /// Flush the batch buffer to the main buffer and optionally to disk
    async fn flush_batch(&mut self) -> Result<()> {
        if self.batch_buffer.is_empty() {
            return Ok(());
        }

        // Serialize all batched entries at once
        for entry in &self.batch_buffer {
            let serialized = self.serialize_entry(entry)?;
            self.buffer.extend_from_slice(&serialized);
        }

        self.batch_buffer.clear();

        // Flush to disk based on policy (avoid recursion by calling flush_to_disk directly)
        match self.fsync_policy {
            FsyncPolicy::Always => {
                self.flush_to_disk(true).await?;
            }
            FsyncPolicy::EverySecond => {
                if self.last_flush.elapsed() >= self.flush_interval {
                    self.flush_to_disk(true).await?;
                } else {
                    // Just write to OS buffer, don't sync
                    self.flush_to_disk(false).await?;
                }
            }
            FsyncPolicy::Never => {
                if self.buffer.len() > self.max_buffer_size {
                    self.flush_to_disk(false).await?;
                }
            }
        }

        Ok(())
    }

    /// Internal method to flush buffer to disk with optional sync
    async fn flush_to_disk(&mut self, sync: bool) -> Result<()> {
        if let Some(ref mut file) = self.file {
            if !self.buffer.is_empty() {
                file.write_all(&self.buffer).await?;
                file.flush().await?;

                if sync {
                    file.sync_all().await?;
                }

                self.buffer.clear();
                self.last_flush = Instant::now();

                debug!(
                    "Flushed {} pending writes to AOF (sync: {})",
                    self.pending_writes, sync
                );
                self.pending_writes = 0;
            }
        }
        Ok(())
    }

    /// Flush buffer to disk
    pub async fn flush(&mut self) -> Result<()> {
        // First serialize any pending batch entries without recursive flush
        if !self.batch_buffer.is_empty() {
            for entry in &self.batch_buffer {
                let serialized = self.serialize_entry(entry)?;
                self.buffer.extend_from_slice(&serialized);
            }
            self.batch_buffer.clear();
        }

        // Now flush to disk with sync
        self.flush_to_disk(true).await
    }

    /// Flush buffer without syncing to disk
    async fn flush_buffer_only(&mut self) -> Result<()> {
        // First serialize any pending batch entries
        if !self.batch_buffer.is_empty() {
            for entry in &self.batch_buffer {
                let serialized = self.serialize_entry(entry)?;
                self.buffer.extend_from_slice(&serialized);
            }
            self.batch_buffer.clear();
        }

        // Flush to disk without sync
        self.flush_to_disk(false).await
    }

    /// Serialize an AOF entry to bytes
    fn serialize_entry(&self, entry: &AofEntry) -> Result<Vec<u8>> {
        let mut result = Vec::new();

        // Format: TIMESTAMP COMMAND ARGS...
        match &entry.command {
            AofCommand::Set { key, value } => {
                let value_str = value.to_string();
                let line = format!("{} SET {} {}\n", entry.timestamp, key, value_str);
                result.extend_from_slice(line.as_bytes());
            }
            AofCommand::SetWithExpiration {
                key,
                value,
                expires_at,
            } => {
                let value_str = value.to_string();
                let line = format!(
                    "{} SETEX {} {} {}\n",
                    entry.timestamp, key, expires_at, value_str
                );
                result.extend_from_slice(line.as_bytes());
            }
            AofCommand::Delete { key } => {
                let line = format!("{} DEL {}\n", entry.timestamp, key);
                result.extend_from_slice(line.as_bytes());
            }
            AofCommand::Expire { key, expires_at } => {
                let line = format!("{} EXPIREAT {} {}\n", entry.timestamp, key, expires_at);
                result.extend_from_slice(line.as_bytes());
            }
        }

        Ok(result)
    }

    /// Get current file size
    pub async fn file_size(&self) -> Result<u64> {
        if let Some(ref file) = self.file {
            let metadata = file.metadata().await?;
            Ok(metadata.len())
        } else {
            Ok(0)
        }
    }

    /// Close the AOF file
    pub async fn close(&mut self) -> Result<()> {
        if self.file.is_some() {
            self.flush().await?;
            self.file = None;
            info!("AOF file closed");
        }
        Ok(())
    }
}

/// Recovery handler for loading data from AOF
#[derive(Debug)]
pub struct RecoveryHandler {
    file_path: PathBuf,
}

impl RecoveryHandler {
    /// Create a new recovery handler
    pub fn new(file_path: PathBuf) -> Self {
        Self { file_path }
    }

    /// Recover data from AOF file
    pub async fn recover(&self) -> Result<Vec<AofEntry>> {
        if !self.file_path.exists() {
            info!(
                "No AOF file found at {:?}, starting with empty database",
                self.file_path
            );
            return Ok(Vec::new());
        }

        info!("Starting AOF recovery from {:?}", self.file_path);
        let start_time = Instant::now();

        let file = File::open(&self.file_path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        let mut entries = Vec::new();
        let mut line_number = 0;

        while let Some(line) = lines.next_line().await? {
            line_number += 1;

            if line.trim().is_empty() {
                continue; // Skip empty lines
            }

            match self.parse_aof_line(&line) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    warn!(
                        "Failed to parse AOF line {}: {} - Error: {}",
                        line_number, line, e
                    );
                    // Continue recovery, don't fail on single line errors
                }
            }
        }

        let recovery_time = start_time.elapsed();
        info!(
            "AOF recovery completed: {} entries loaded in {:?}",
            entries.len(),
            recovery_time
        );

        Ok(entries)
    }

    /// Parse a single AOF line into an entry
    fn parse_aof_line(&self, line: &str) -> Result<AofEntry> {
        let parts: Vec<&str> = line.split_whitespace().collect();

        if parts.len() < 2 {
            return Err(RustyPotatoError::PersistenceError {
                message: "Invalid AOF line format".to_string(),
                source: Some(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid AOF line format",
                )),
                recoverable: false,
            });
        }

        let timestamp =
            parts[0]
                .parse::<u64>()
                .map_err(|_| RustyPotatoError::PersistenceError {
                    message: "Invalid timestamp".to_string(),
                    source: Some(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Invalid timestamp",
                    )),
                    recoverable: false,
                })?;

        let command =
            match parts[1] {
                "SET" => {
                    if parts.len() < 4 {
                        return Err(RustyPotatoError::PersistenceError {
                            message: "Invalid SET command".to_string(),
                            source: Some(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "Invalid SET command",
                            )),
                            recoverable: false,
                        });
                    }
                    let key = parts[2].to_string();
                    let value_str = parts[3..].join(" "); // Handle values with spaces
                    let value = self.parse_value(&value_str);
                    AofCommand::Set { key, value }
                }
                "SETEX" => {
                    if parts.len() < 5 {
                        return Err(RustyPotatoError::PersistenceError {
                            message: "Invalid SETEX command".to_string(),
                            source: Some(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "Invalid SETEX command",
                            )),
                            recoverable: false,
                        });
                    }
                    let key = parts[2].to_string();
                    let expires_at = parts[3].parse::<u64>().map_err(|_| {
                        RustyPotatoError::PersistenceError {
                            message: "Invalid expiration timestamp".to_string(),
                            source: Some(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "Invalid expiration timestamp",
                            )),
                            recoverable: false,
                        }
                    })?;
                    let value_str = parts[4..].join(" ");
                    let value = self.parse_value(&value_str);
                    AofCommand::SetWithExpiration {
                        key,
                        value,
                        expires_at,
                    }
                }
                "DEL" => {
                    if parts.len() < 3 {
                        return Err(RustyPotatoError::PersistenceError {
                            message: "Invalid DEL command".to_string(),
                            source: Some(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "Invalid DEL command",
                            )),
                            recoverable: false,
                        });
                    }
                    let key = parts[2].to_string();
                    AofCommand::Delete { key }
                }
                "EXPIREAT" => {
                    if parts.len() < 4 {
                        return Err(RustyPotatoError::PersistenceError {
                            message: "Invalid EXPIREAT command".to_string(),
                            source: Some(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "Invalid EXPIREAT command",
                            )),
                            recoverable: false,
                        });
                    }
                    let key = parts[2].to_string();
                    let expires_at = parts[3].parse::<u64>().map_err(|_| {
                        RustyPotatoError::PersistenceError {
                            message: "Invalid expiration timestamp".to_string(),
                            source: Some(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "Invalid expiration timestamp",
                            )),
                            recoverable: false,
                        }
                    })?;
                    AofCommand::Expire { key, expires_at }
                }
                _ => {
                    return Err(RustyPotatoError::PersistenceError {
                        message: format!("Unknown AOF command: {}", parts[1]),
                        source: Some(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Unknown AOF command: {}", parts[1]),
                        )),
                        recoverable: false,
                    });
                }
            };

        Ok(AofEntry { timestamp, command })
    }

    /// Parse a value string into ValueType
    fn parse_value(&self, value_str: &str) -> ValueType {
        // Try to parse as integer first
        if let Ok(int_val) = value_str.parse::<i64>() {
            ValueType::Integer(int_val)
        } else {
            ValueType::String(value_str.to_string())
        }
    }
}

/// Persistence manager that coordinates AOF writing and recovery
#[derive(Debug)]
pub struct PersistenceManager {
    aof_writer: Option<AofWriter>,
    recovery_handler: RecoveryHandler,
    write_sender: Option<mpsc::UnboundedSender<AofEntry>>,
    _background_task: Option<tokio::task::JoinHandle<()>>,
}

impl PersistenceManager {
    /// Create a new persistence manager
    pub async fn new(config: &Config) -> Result<Self> {
        let aof_writer = if config.storage.aof_enabled {
            Some(AofWriter::new(config).await?)
        } else {
            None
        };

        let recovery_handler = RecoveryHandler::new(config.storage.aof_path.clone());

        let (write_sender, background_task) = if config.storage.aof_enabled {
            let (sender, receiver) = mpsc::unbounded_channel();
            let task = Self::start_background_writer(receiver, config.clone()).await?;
            (Some(sender), Some(task))
        } else {
            (None, None)
        };

        Ok(Self {
            aof_writer,
            recovery_handler,
            write_sender,
            _background_task: background_task,
        })
    }

    /// Start background writer task for async AOF writing
    async fn start_background_writer(
        mut receiver: mpsc::UnboundedReceiver<AofEntry>,
        config: Config,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let mut writer = AofWriter::new(&config).await?;
        let mut flush_interval = interval(Duration::from_secs(1));

        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    entry = receiver.recv() => {
                        match entry {
                            Some(entry) => {
                                if let Err(e) = writer.write_entry(entry).await {
                                    error!("Failed to write AOF entry: {}", e);
                                }
                            }
                            None => {
                                // Channel closed, flush and exit
                                if let Err(e) = writer.flush().await {
                                    error!("Failed to flush AOF on shutdown: {}", e);
                                }
                                break;
                            }
                        }
                    }
                    _ = flush_interval.tick() => {
                        if let Err(e) = writer.flush().await {
                            error!("Failed to flush AOF periodically: {}", e);
                        }
                    }
                }
            }

            // Final cleanup
            if let Err(e) = writer.close().await {
                error!("Failed to close AOF writer: {}", e);
            }
        });

        Ok(handle)
    }

    /// Log a SET operation to AOF
    pub async fn log_set(&self, key: String, value: ValueType) -> Result<()> {
        if let Some(ref sender) = self.write_sender {
            let entry = AofEntry {
                timestamp: current_timestamp(),
                command: AofCommand::Set { key, value },
            };

            sender
                .send(entry)
                .map_err(|_| RustyPotatoError::PersistenceError {
                    message: "AOF writer channel closed".to_string(),
                    source: Some(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "AOF writer channel closed",
                    )),
                    recoverable: true,
                })?;
        }
        Ok(())
    }

    /// Log a SET operation with expiration to AOF
    pub async fn log_set_with_expiration(
        &self,
        key: String,
        value: ValueType,
        expires_at: Instant,
    ) -> Result<()> {
        if let Some(ref sender) = self.write_sender {
            let expires_at_timestamp = instant_to_timestamp(expires_at);
            let entry = AofEntry {
                timestamp: current_timestamp(),
                command: AofCommand::SetWithExpiration {
                    key,
                    value,
                    expires_at: expires_at_timestamp,
                },
            };

            sender
                .send(entry)
                .map_err(|_| RustyPotatoError::PersistenceError {
                    message: "AOF writer channel closed".to_string(),
                    source: Some(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "AOF writer channel closed",
                    )),
                    recoverable: true,
                })?;
        }
        Ok(())
    }

    /// Log a DELETE operation to AOF
    pub async fn log_delete(&self, key: String) -> Result<()> {
        if let Some(ref sender) = self.write_sender {
            let entry = AofEntry {
                timestamp: current_timestamp(),
                command: AofCommand::Delete { key },
            };

            sender
                .send(entry)
                .map_err(|_| RustyPotatoError::PersistenceError {
                    message: "AOF writer channel closed".to_string(),
                    source: Some(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "AOF writer channel closed",
                    )),
                    recoverable: true,
                })?;
        }
        Ok(())
    }

    /// Log an EXPIRE operation to AOF
    pub async fn log_expire(&self, key: String, expires_at: Instant) -> Result<()> {
        if let Some(ref sender) = self.write_sender {
            let expires_at_timestamp = instant_to_timestamp(expires_at);
            let entry = AofEntry {
                timestamp: current_timestamp(),
                command: AofCommand::Expire {
                    key,
                    expires_at: expires_at_timestamp,
                },
            };

            sender
                .send(entry)
                .map_err(|_| RustyPotatoError::PersistenceError {
                    message: "AOF writer channel closed".to_string(),
                    source: Some(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        "AOF writer channel closed",
                    )),
                    recoverable: true,
                })?;
        }
        Ok(())
    }

    /// Recover data from AOF file
    pub async fn recover(&self) -> Result<Vec<AofEntry>> {
        self.recovery_handler.recover().await
    }

    /// Force flush of pending writes
    pub async fn flush(&mut self) -> Result<()> {
        if let Some(ref mut writer) = self.aof_writer {
            writer.flush().await?;
        }
        Ok(())
    }

    /// Get AOF file size
    pub async fn file_size(&self) -> Result<u64> {
        if let Some(ref writer) = self.aof_writer {
            writer.file_size().await
        } else {
            Ok(0)
        }
    }

    /// Check if AOF is enabled
    pub fn is_enabled(&self) -> bool {
        self.aof_writer.is_some()
    }
}

/// Get current timestamp in seconds since Unix epoch
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Convert Instant to timestamp (seconds since Unix epoch)
fn instant_to_timestamp(instant: Instant) -> u64 {
    let now = Instant::now();
    let system_now = SystemTime::now();

    if instant > now {
        // Future time
        let duration_from_now = instant - now;
        system_now
            .checked_add(duration_from_now)
            .unwrap_or(system_now)
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    } else {
        // Past time
        let duration_ago = now - instant;
        system_now
            .checked_sub(duration_ago)
            .unwrap_or(system_now)
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

/// Convert timestamp to Instant
fn timestamp_to_instant(timestamp: u64) -> Instant {
    let now = Instant::now();
    let system_now = SystemTime::now();
    let target_system_time = UNIX_EPOCH + Duration::from_secs(timestamp);

    if target_system_time > system_now {
        // Future time
        let duration_from_now = target_system_time
            .duration_since(system_now)
            .unwrap_or_default();
        now + duration_from_now
    } else {
        // Past time (treat as expired)
        now - Duration::from_secs(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, StorageConfig};
    use std::path::PathBuf;
    use tempfile::tempdir;
    use tokio::time::sleep;

    fn create_test_config(aof_path: PathBuf) -> Config {
        let mut config = Config::default();
        config.storage = StorageConfig {
            aof_enabled: true,
            aof_path,
            aof_fsync_policy: FsyncPolicy::Always,
            memory_limit: None,
        };
        config
    }

    #[tokio::test]
    async fn test_aof_writer_creation() {
        let temp_dir = tempdir().unwrap();
        let aof_path = temp_dir.path().join("test.aof");
        let config = create_test_config(aof_path);

        let writer = AofWriter::new(&config).await;
        assert!(writer.is_ok());
    }

    #[tokio::test]
    async fn test_aof_entry_serialization() {
        let temp_dir = tempdir().unwrap();
        let aof_path = temp_dir.path().join("test.aof");
        let config = create_test_config(aof_path);

        let mut writer = AofWriter::new(&config).await.unwrap();

        let entry = AofEntry {
            timestamp: 1234567890,
            command: AofCommand::Set {
                key: "test_key".to_string(),
                value: ValueType::String("test_value".to_string()),
            },
        };

        writer.write_entry(entry).await.unwrap();
        writer.flush().await.unwrap();

        // Verify file was written
        let file_size = writer.file_size().await.unwrap();
        assert!(file_size > 0);
    }

    #[tokio::test]
    async fn test_aof_recovery() {
        let temp_dir = tempdir().unwrap();
        let aof_path = temp_dir.path().join("test.aof");
        let config = create_test_config(aof_path.clone());

        // Write some entries
        {
            let mut writer = AofWriter::new(&config).await.unwrap();

            let entries = vec![
                AofEntry {
                    timestamp: 1234567890,
                    command: AofCommand::Set {
                        key: "key1".to_string(),
                        value: ValueType::String("value1".to_string()),
                    },
                },
                AofEntry {
                    timestamp: 1234567891,
                    command: AofCommand::Set {
                        key: "key2".to_string(),
                        value: ValueType::Integer(42),
                    },
                },
                AofEntry {
                    timestamp: 1234567892,
                    command: AofCommand::Delete {
                        key: "key1".to_string(),
                    },
                },
            ];

            for entry in entries {
                writer.write_entry(entry).await.unwrap();
            }
            writer.flush().await.unwrap();
        }

        // Recover entries
        let recovery_handler = RecoveryHandler::new(aof_path);
        let recovered_entries = recovery_handler.recover().await.unwrap();

        assert_eq!(recovered_entries.len(), 3);

        // Verify first entry
        match &recovered_entries[0].command {
            AofCommand::Set { key, value } => {
                assert_eq!(key, "key1");
                assert_eq!(value.to_string(), "value1");
            }
            _ => panic!("Expected SET command"),
        }

        // Verify second entry
        match &recovered_entries[1].command {
            AofCommand::Set { key, value } => {
                assert_eq!(key, "key2");
                assert_eq!(value.to_integer().unwrap(), 42);
            }
            _ => panic!("Expected SET command"),
        }

        // Verify third entry
        match &recovered_entries[2].command {
            AofCommand::Delete { key } => {
                assert_eq!(key, "key1");
            }
            _ => panic!("Expected DELETE command"),
        }
    }

    #[tokio::test]
    async fn test_persistence_manager() {
        let temp_dir = tempdir().unwrap();
        let aof_path = temp_dir.path().join("test.aof");
        let config = create_test_config(aof_path);

        let manager = PersistenceManager::new(&config).await.unwrap();
        assert!(manager.is_enabled());

        // Test logging operations
        manager
            .log_set(
                "test_key".to_string(),
                ValueType::String("test_value".to_string()),
            )
            .await
            .unwrap();
        manager.log_delete("test_key".to_string()).await.unwrap();

        // Give background writer time to process
        sleep(Duration::from_millis(100)).await;

        // Verify file was created and has content
        let file_size = manager.file_size().await.unwrap();
        assert!(file_size > 0);
    }

    #[tokio::test]
    async fn test_aof_with_expiration() {
        let temp_dir = tempdir().unwrap();
        let aof_path = temp_dir.path().join("test.aof");
        let config = create_test_config(aof_path.clone());

        // Write entry with expiration
        {
            let mut writer = AofWriter::new(&config).await.unwrap();

            let expires_at = Instant::now() + Duration::from_secs(60);
            let entry = AofEntry {
                timestamp: current_timestamp(),
                command: AofCommand::SetWithExpiration {
                    key: "expiring_key".to_string(),
                    value: ValueType::String("expiring_value".to_string()),
                    expires_at: instant_to_timestamp(expires_at),
                },
            };

            writer.write_entry(entry).await.unwrap();
            writer.flush().await.unwrap();
        }

        // Recover and verify
        let recovery_handler = RecoveryHandler::new(aof_path);
        let recovered_entries = recovery_handler.recover().await.unwrap();

        assert_eq!(recovered_entries.len(), 1);
        match &recovered_entries[0].command {
            AofCommand::SetWithExpiration {
                key,
                value,
                expires_at: _,
            } => {
                assert_eq!(key, "expiring_key");
                assert_eq!(value.to_string(), "expiring_value");
            }
            _ => panic!("Expected SETEX command"),
        }
    }

    #[tokio::test]
    async fn test_aof_disabled() {
        let temp_dir = tempdir().unwrap();
        let aof_path = temp_dir.path().join("test.aof");
        let mut config = create_test_config(aof_path);
        config.storage.aof_enabled = false;

        let manager = PersistenceManager::new(&config).await.unwrap();
        assert!(!manager.is_enabled());

        // Operations should succeed but not write to file
        manager
            .log_set(
                "test_key".to_string(),
                ValueType::String("test_value".to_string()),
            )
            .await
            .unwrap();

        let file_size = manager.file_size().await.unwrap();
        assert_eq!(file_size, 0);
    }

    #[test]
    fn test_timestamp_conversion() {
        let now = Instant::now();
        let timestamp = instant_to_timestamp(now);
        let converted_back = timestamp_to_instant(timestamp);

        // Should be approximately the same (within a few seconds due to conversion precision)
        let diff = if converted_back > now {
            converted_back - now
        } else {
            now - converted_back
        };

        assert!(diff < Duration::from_secs(5));
    }
}
