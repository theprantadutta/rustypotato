//! Comprehensive unit tests for configuration management
//!
//! Tests cover configuration loading, validation, defaults,
//! and environment variable handling.

use rustypotato::Config;
use std::env;
use tempfile::TempDir;
use std::fs;

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        
        // Server defaults
        assert_eq!(config.server.port, 6379);
        assert_eq!(config.server.bind_address, "127.0.0.1");
        assert_eq!(config.server.max_connections, 10000);
        
        // Storage defaults
        assert!(!config.storage.aof_enabled);
        assert_eq!(config.storage.aof_path.to_string_lossy(), "rustypotato.aof");
        assert_eq!(config.storage.aof_fsync_interval, 1);
        
        // Network defaults
        assert!(config.network.tcp_nodelay);
        assert_eq!(config.network.connection_timeout, 30);
        assert_eq!(config.network.read_timeout, 30);
        assert_eq!(config.network.write_timeout, 30);
        
        // Logging defaults
        assert_eq!(config.logging.level, "info");
        assert_eq!(config.logging.format, "Pretty");
    }

    #[test]
    fn test_config_validation() {
        let mut config = Config::default();
        
        // Valid configuration should pass
        assert!(config.validate().is_ok());
        
        // Invalid port
        config.server.port = 0;
        assert!(config.validate().is_err());
        
        config.server.port = 65536;
        assert!(config.validate().is_err());
        
        // Reset to valid port
        config.server.port = 6379;
        assert!(config.validate().is_ok());
        
        // Invalid max connections
        config.server.max_connections = 0;
        assert!(config.validate().is_err());
        
        // Reset to valid max connections
        config.server.max_connections = 1000;
        assert!(config.validate().is_ok());
        
        // Invalid timeouts
        config.network.connection_timeout = 0;
        assert!(config.validate().is_err());
        
        config.network.connection_timeout = 30;
        config.network.read_timeout = 0;
        assert!(config.validate().is_err());
        
        config.network.read_timeout = 30;
        config.network.write_timeout = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config.toml");
        
        let config_content = r#"
[server]
port = 8080
bind_address = "0.0.0.0"
max_connections = 5000

[storage]
aof_enabled = true
aof_path = "/tmp/test.aof"
aof_fsync_interval = 5

[network]
tcp_nodelay = false
connection_timeout = 60
read_timeout = 45
write_timeout = 45

[logging]
level = "debug"
format = "Json"
"#;
        
        fs::write(&config_path, config_content).unwrap();
        
        let config = Config::from_file(&config_path).unwrap();
        
        // Verify loaded values
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.server.bind_address, "0.0.0.0");
        assert_eq!(config.server.max_connections, 5000);
        
        assert!(config.storage.aof_enabled);
        assert_eq!(config.storage.aof_path.to_string_lossy(), "/tmp/test.aof");
        assert_eq!(config.storage.aof_fsync_interval, 5);
        
        assert!(!config.network.tcp_nodelay);
        assert_eq!(config.network.connection_timeout, 60);
        assert_eq!(config.network.read_timeout, 45);
        assert_eq!(config.network.write_timeout, 45);
        
        assert_eq!(config.logging.level, "debug");
        assert_eq!(config.logging.format, "Json");
    }

    #[test]
    fn test_config_from_nonexistent_file() {
        let result = Config::from_file("nonexistent.toml");
        assert!(result.is_err());
    }

    #[test]
    fn test_config_from_invalid_toml() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("invalid_config.toml");
        
        let invalid_content = r#"
[server
port = "not_a_number"
invalid_toml
"#;
        
        fs::write(&config_path, invalid_content).unwrap();
        
        let result = Config::from_file(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_partial_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("partial_config.toml");
        
        // Only specify some values, others should use defaults
        let partial_content = r#"
[server]
port = 9000

[logging]
level = "warn"
"#;
        
        fs::write(&config_path, partial_content).unwrap();
        
        let config = Config::from_file(&config_path).unwrap();
        
        // Specified values
        assert_eq!(config.server.port, 9000);
        assert_eq!(config.logging.level, "warn");
        
        // Default values
        assert_eq!(config.server.bind_address, "127.0.0.1");
        assert_eq!(config.server.max_connections, 10000);
        assert!(!config.storage.aof_enabled);
        assert_eq!(config.logging.format, "Pretty");
    }

    #[test]
    fn test_config_environment_variables() {
        // Set environment variables
        env::set_var("RUSTYPOTATO_SERVER_PORT", "7777");
        env::set_var("RUSTYPOTATO_SERVER_BIND_ADDRESS", "192.168.1.1");
        env::set_var("RUSTYPOTATO_STORAGE_AOF_ENABLED", "true");
        env::set_var("RUSTYPOTATO_LOGGING_LEVEL", "trace");
        
        let config = Config::from_env().unwrap();
        
        assert_eq!(config.server.port, 7777);
        assert_eq!(config.server.bind_address, "192.168.1.1");
        assert!(config.storage.aof_enabled);
        assert_eq!(config.logging.level, "trace");
        
        // Clean up environment variables
        env::remove_var("RUSTYPOTATO_SERVER_PORT");
        env::remove_var("RUSTYPOTATO_SERVER_BIND_ADDRESS");
        env::remove_var("RUSTYPOTATO_STORAGE_AOF_ENABLED");
        env::remove_var("RUSTYPOTATO_LOGGING_LEVEL");
    }

    #[test]
    fn test_config_environment_invalid_values() {
        // Set invalid environment variables
        env::set_var("RUSTYPOTATO_SERVER_PORT", "invalid_port");
        
        let result = Config::from_env();
        assert!(result.is_err());
        
        // Clean up
        env::remove_var("RUSTYPOTATO_SERVER_PORT");
    }

    #[test]
    fn test_config_merge_priority() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("merge_config.toml");
        
        // Create config file
        let file_content = r#"
[server]
port = 8080
bind_address = "0.0.0.0"
max_connections = 5000

[logging]
level = "info"
"#;
        
        fs::write(&config_path, file_content).unwrap();
        
        // Set environment variables that should override file values
        env::set_var("RUSTYPOTATO_SERVER_PORT", "9090");
        env::set_var("RUSTYPOTATO_LOGGING_LEVEL", "debug");
        
        let config = Config::from_file_and_env(&config_path).unwrap();
        
        // Environment should override file
        assert_eq!(config.server.port, 9090);
        assert_eq!(config.logging.level, "debug");
        
        // File values should be used where no env override
        assert_eq!(config.server.bind_address, "0.0.0.0");
        assert_eq!(config.server.max_connections, 5000);
        
        // Clean up
        env::remove_var("RUSTYPOTATO_SERVER_PORT");
        env::remove_var("RUSTYPOTATO_LOGGING_LEVEL");
    }

    #[test]
    fn test_config_edge_cases() {
        let mut config = Config::default();
        
        // Test minimum valid values
        config.server.port = 1;
        config.server.max_connections = 1;
        config.network.connection_timeout = 1;
        config.network.read_timeout = 1;
        config.network.write_timeout = 1;
        config.storage.aof_fsync_interval = 1;
        
        assert!(config.validate().is_ok());
        
        // Test maximum valid values
        config.server.port = 65535;
        config.server.max_connections = 1_000_000;
        config.network.connection_timeout = 3600;
        config.network.read_timeout = 3600;
        config.network.write_timeout = 3600;
        config.storage.aof_fsync_interval = 3600;
        
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_display() {
        let config = Config::default();
        let display_str = format!("{}", config);
        
        // Should contain key configuration values
        assert!(display_str.contains("port: 6379"));
        assert!(display_str.contains("bind_address: 127.0.0.1"));
        assert!(display_str.contains("max_connections: 10000"));
    }

    #[test]
    fn test_config_clone() {
        let config1 = Config::default();
        let config2 = config1.clone();
        
        assert_eq!(config1.server.port, config2.server.port);
        assert_eq!(config1.server.bind_address, config2.server.bind_address);
        assert_eq!(config1.storage.aof_enabled, config2.storage.aof_enabled);
    }

    #[test]
    fn test_config_debug() {
        let config = Config::default();
        let debug_str = format!("{:?}", config);
        
        // Should contain debug representation
        assert!(debug_str.contains("Config"));
        assert!(debug_str.contains("server"));
        assert!(debug_str.contains("storage"));
        assert!(debug_str.contains("network"));
        assert!(debug_str.contains("logging"));
    }

    #[test]
    fn test_config_special_paths() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("path_config.toml");
        
        let config_content = format!(r#"
[storage]
aof_path = "{}"
"#, temp_dir.path().join("custom.aof").to_string_lossy());
        
        fs::write(&config_path, config_content).unwrap();
        
        let config = Config::from_file(&config_path).unwrap();
        
        assert!(config.storage.aof_path.to_string_lossy().contains("custom.aof"));
    }

    #[test]
    fn test_config_boolean_parsing() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("bool_config.toml");
        
        let config_content = r#"
[storage]
aof_enabled = true

[network]
tcp_nodelay = false
"#;
        
        fs::write(&config_path, config_content).unwrap();
        
        let config = Config::from_file(&config_path).unwrap();
        
        assert!(config.storage.aof_enabled);
        assert!(!config.network.tcp_nodelay);
    }

    #[test]
    fn test_config_string_validation() {
        let mut config = Config::default();
        
        // Valid log levels
        let valid_levels = vec!["trace", "debug", "info", "warn", "error"];
        for level in valid_levels {
            config.logging.level = level.to_string();
            assert!(config.validate().is_ok());
        }
        
        // Invalid log level
        config.logging.level = "invalid".to_string();
        assert!(config.validate().is_err());
        
        // Valid log formats
        config.logging.level = "info".to_string();
        let valid_formats = vec!["Pretty", "Json", "Compact"];
        for format in valid_formats {
            config.logging.format = format.to_string();
            assert!(config.validate().is_ok());
        }
        
        // Invalid log format
        config.logging.format = "invalid".to_string();
        assert!(config.validate().is_err());
    }
}