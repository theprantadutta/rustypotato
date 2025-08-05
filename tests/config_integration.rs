//! Integration tests for configuration management
//!
//! These tests verify that the configuration system works correctly
//! with the server components and can load from various sources.

use rustypotato::{Config, RustyPotatoServer};
use std::env;
use std::collections::HashMap;
use std::sync::Mutex;
use tempfile::TempDir;

// Global mutex to ensure all config-related tests run serially
// This prevents environment variable pollution between tests
static GLOBAL_CONFIG_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Helper function to clean up all RUSTYPOTATO_ environment variables
/// and return the original values for restoration
fn clean_rustypotato_env() -> HashMap<String, String> {
    let mut original_values = HashMap::new();
    
    // Collect all RUSTYPOTATO_ environment variables
    let rustypotato_vars: Vec<String> = env::vars()
        .filter(|(key, _)| key.starts_with("RUSTYPOTATO_"))
        .map(|(key, value)| {
            original_values.insert(key.clone(), value);
            key
        })
        .collect();
    
    // Remove all RUSTYPOTATO_ variables
    for var in rustypotato_vars {
        env::remove_var(&var);
    }
    
    original_values
}

/// Helper function to restore environment variables
fn restore_env(original_values: HashMap<String, String>) {
    // First remove any RUSTYPOTATO_ variables that might have been set
    for (key, _) in env::vars() {
        if key.starts_with("RUSTYPOTATO_") {
            env::remove_var(&key);
        }
    }
    
    // Then restore the original values
    for (key, value) in original_values {
        env::set_var(key, value);
    }
}

#[tokio::test]
async fn test_config_integration() {
    let _guard = GLOBAL_CONFIG_TEST_LOCK.lock().unwrap();
    
    // Clean up environment before test
    let original_env = clean_rustypotato_env();
    
    // Test that the server can be created with default configuration
    let config = Config::default();
    let server = RustyPotatoServer::new(config).unwrap();

    assert_eq!(server.config().server.port, 6379);
    assert_eq!(server.config().server.bind_address, "127.0.0.1");
    assert_eq!(server.config().server.max_connections, 10000);
    
    // Restore environment
    restore_env(original_env);
}

#[tokio::test]
async fn test_config_loading() {
    let _guard = GLOBAL_CONFIG_TEST_LOCK.lock().unwrap();

    // Clean up all RUSTYPOTATO_ environment variables
    let original_env = clean_rustypotato_env();

    // Test that configuration can be loaded from the load() method
    let config = Config::load().unwrap();

    // Should have default values when no config file or env vars are set
    assert_eq!(config.server.port, 6379);
    assert!(config.storage.aof_enabled);
    assert_eq!(config.logging.level, "info");

    // Restore original environment state
    restore_env(original_env);
}

#[tokio::test]
async fn test_config_from_file() {
    let _guard = GLOBAL_CONFIG_TEST_LOCK.lock().unwrap();

    // Clean up all RUSTYPOTATO_ environment variables
    let original_env = clean_rustypotato_env();

    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("test_config.toml");

    let toml_content = r#"
[server]
port = 8080
bind_address = "0.0.0.0"
max_connections = 5000

[storage]
aof_enabled = false
memory_limit = 1073741824

[logging]
level = "debug"
"#;

    std::fs::write(&config_path, toml_content).unwrap();

    let config = Config::load_from_file(Some(&config_path)).unwrap();

    assert_eq!(config.server.port, 8080);
    assert_eq!(config.server.bind_address, "0.0.0.0");
    assert_eq!(config.server.max_connections, 5000);
    assert!(!config.storage.aof_enabled);
    assert_eq!(config.storage.memory_limit, Some(1073741824));
    assert_eq!(config.logging.level, "debug");

    // Restore original environment state
    restore_env(original_env);
}

#[tokio::test]
async fn test_config_validation_errors() {
    let _guard = GLOBAL_CONFIG_TEST_LOCK.lock().unwrap();
    
    // Clean up environment before test
    let original_env = clean_rustypotato_env();
    
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("invalid_config.toml");

    let invalid_toml = r#"
[server]
port = 0
max_connections = 0
"#;

    std::fs::write(&config_path, invalid_toml).unwrap();

    let result = Config::load_from_file(Some(&config_path));
    assert!(result.is_err());
    
    // Restore environment
    restore_env(original_env);
}

#[tokio::test]
async fn test_sample_config_creation() {
    let _guard = GLOBAL_CONFIG_TEST_LOCK.lock().unwrap();
    
    // Clean up environment before test
    let original_env = clean_rustypotato_env();
    
    let temp_dir = TempDir::new().unwrap();
    let sample_path = temp_dir.path().join("sample.toml");

    Config::create_sample_config(&sample_path).unwrap();

    assert!(sample_path.exists());

    // Verify the sample config can be loaded and used
    let config = Config::load_from_file(Some(&sample_path)).unwrap();
    let server = RustyPotatoServer::new(config).unwrap();

    assert_eq!(server.config().server.port, 6379);
    
    // Restore environment
    restore_env(original_env);
}

#[test]
fn test_config_with_environment_variables() {
    let _guard = GLOBAL_CONFIG_TEST_LOCK.lock().unwrap();

    // Clean up all RUSTYPOTATO_ environment variables
    let original_env = clean_rustypotato_env();

    // Set test environment variables
    env::set_var("RUSTYPOTATO_SERVER.PORT", "9000");
    env::set_var("RUSTYPOTATO_SERVER.BIND_ADDRESS", "192.168.1.1");
    env::set_var("RUSTYPOTATO_STORAGE.AOF_ENABLED", "false");

    let config = Config::load().unwrap();

    assert_eq!(config.server.port, 9000);
    assert_eq!(config.server.bind_address, "192.168.1.1");
    assert!(!config.storage.aof_enabled);

    // Restore original environment state
    restore_env(original_env);
}

#[tokio::test]
async fn test_server_with_custom_config() {
    let (config, original_env) = {
        let _guard = GLOBAL_CONFIG_TEST_LOCK.lock().unwrap();
        
        // Clean up environment before test
        let original_env = clean_rustypotato_env();
        
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("server_test.toml");

        let toml_content = r#"
[server]
port = 0  # Use random port for testing
bind_address = "127.0.0.1"
max_connections = 100

[storage]
aof_enabled = false

[logging]
level = "warn"
"#;

        std::fs::write(&config_path, toml_content).unwrap();

        let config = Config::load_from_file(Some(&config_path)).unwrap();
        (config, original_env)
        // Guard is automatically dropped here
    };

    let mut server = RustyPotatoServer::new(config).unwrap();

    // Test that the server can start with the custom configuration
    let addr = server.start_with_addr().await.unwrap();

    // Verify the server is using the custom configuration
    assert_eq!(addr.ip().to_string(), "127.0.0.1");
    assert!(addr.port() > 0); // Should get a random port
    assert_eq!(server.config().server.max_connections, 100);
    assert!(!server.config().storage.aof_enabled);
    assert_eq!(server.config().logging.level, "warn");
    
    // Restore environment
    restore_env(original_env);
}
