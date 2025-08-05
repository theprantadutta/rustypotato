//! Test utilities for RustyPotato
//!
//! This module provides shared utilities for testing, particularly for
//! managing environment variables in tests to prevent pollution between tests.

use std::collections::HashMap;
use std::env;
use std::sync::Mutex;

/// Global mutex to ensure all config-related tests run serially
/// This prevents environment variable pollution between tests
pub static GLOBAL_CONFIG_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Helper function to clean up all RUSTYPOTATO_ environment variables
/// and return the original values for restoration
pub fn clean_rustypotato_env() -> HashMap<String, String> {
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
pub fn restore_env(original_values: HashMap<String, String>) {
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