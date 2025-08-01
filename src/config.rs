//! Configuration management for RustyPotato
//! 
//! This module handles loading and validating configuration from various sources
//! including files, environment variables, and command line arguments.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Main configuration structure for RustyPotato server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub network: NetworkConfig,
    pub logging: LoggingConfig,
}

/// Server-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    pub bind_address: String,
    pub max_connections: usize,
    pub worker_threads: Option<usize>,
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub aof_enabled: bool,
    pub aof_path: PathBuf,
    pub aof_fsync_policy: FsyncPolicy,
    pub memory_limit: Option<usize>,
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub tcp_nodelay: bool,
    pub tcp_keepalive: bool,
    pub connection_timeout: u64,
    pub read_timeout: u64,
    pub write_timeout: u64,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: LogFormat,
    pub file_path: Option<PathBuf>,
}

/// AOF fsync policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FsyncPolicy {
    Always,
    EverySecond,
    Never,
}

/// Log output format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogFormat {
    Json,
    Pretty,
    Compact,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            storage: StorageConfig::default(),
            network: NetworkConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 6379,
            bind_address: "127.0.0.1".to_string(),
            max_connections: 10000,
            worker_threads: None,
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            aof_enabled: true,
            aof_path: PathBuf::from("rustypotato.aof"),
            aof_fsync_policy: FsyncPolicy::EverySecond,
            memory_limit: None,
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            tcp_nodelay: true,
            tcp_keepalive: true,
            connection_timeout: 30,
            read_timeout: 30,
            write_timeout: 30,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: LogFormat::Pretty,
            file_path: None,
        }
    }
}