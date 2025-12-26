//! Fuzz target for command execution
//!
//! This fuzzer tests command parsing and validation with arbitrary inputs
//! to find edge cases in argument validation and command execution paths.
//!
//! Run with: cargo +nightly fuzz run command_execution
//!
//! Note: cargo-fuzz requires Linux/macOS. On Windows, run in WSL or CI.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rustypotato::commands::{Command, CommandRegistry, ParsedCommand};
use rustypotato::MemoryStore;
use std::sync::Arc;

/// Generate a RESP array command from arbitrary strings
fn make_command(parts: &[String]) -> Vec<u8> {
    let mut result = format!("*{}\r\n", parts.len());
    for part in parts {
        result.push_str(&format!("${}\r\n{}\r\n", part.len(), part));
    }
    result.into_bytes()
}

fuzz_target!(|data: Vec<String>| {
    // Skip empty inputs
    if data.is_empty() {
        return;
    }

    // Create a runtime for async operations
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        let store = Arc::new(MemoryStore::new());
        let registry = CommandRegistry::new(store);

        // Create a parsed command from the fuzz input
        let name = data[0].clone();
        let args: Vec<String> = data.iter().skip(1).cloned().collect();

        let parsed = ParsedCommand::new(name, args);

        // Try to get and execute the command
        // This should never panic, only return errors for invalid commands
        if let Some(cmd) = registry.get(&parsed.name) {
            // Validate should not panic
            let _ = cmd.validate(&parsed);

            // Execute should not panic (even if validation failed)
            let _ = cmd.execute(&parsed).await;
        }
    });
});
