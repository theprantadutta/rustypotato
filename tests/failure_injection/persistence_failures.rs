//! Persistence failure injection tests
//!
//! Tests the system's resilience to AOF-related failures:
//! - Corrupted AOF file recovery
//! - Partial/incomplete AOF entries
//! - Missing AOF file handling
//! - Recovery with mixed valid/invalid entries

use rustypotato::config::{Config, FsyncPolicy, StorageConfig};
use rustypotato::storage::persistence::{AofCommand, AofEntry, AofWriter, RecoveryHandler};
use rustypotato::storage::ValueType;
use std::path::PathBuf;
use tempfile::tempdir;
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Create a test config with AOF enabled
fn create_test_config(aof_path: PathBuf) -> Config {
    Config {
        storage: StorageConfig {
            aof_enabled: true,
            aof_path,
            aof_fsync_policy: FsyncPolicy::Always,
            memory_limit: None,
        },
        ..Default::default()
    }
}

// ==================== Corrupted File Tests ====================

/// Test: Recovery handles completely corrupted/garbage file
#[tokio::test]
async fn test_recovery_completely_corrupted_file() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("corrupted.aof");

    // Write text-based garbage data to AOF file (avoiding binary that might cause UTF-8 issues)
    let mut file = fs::File::create(&aof_path).await.unwrap();
    file.write_all(b"garbage not valid aof format\n")
        .await
        .unwrap();
    file.write_all(b"also invalid line here\n")
        .await
        .unwrap();
    file.sync_all().await.unwrap();

    // Recovery should not panic, should skip invalid lines
    let recovery_handler = RecoveryHandler::new(aof_path);
    let result = recovery_handler.recover().await;

    assert!(result.is_ok(), "Recovery should not fail on corrupted file");
    let entries = result.unwrap();
    assert!(
        entries.is_empty(),
        "Corrupted entries should be skipped, got {} entries",
        entries.len()
    );
}

/// Test: Recovery handles mixed valid and invalid entries
#[tokio::test]
async fn test_recovery_mixed_valid_invalid() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("mixed.aof");

    // Write mix of valid and invalid entries
    let mut file = fs::File::create(&aof_path).await.unwrap();

    // Valid entry
    file.write_all(b"1234567890 SET key1 value1\n")
        .await
        .unwrap();
    // Invalid - malformed timestamp
    file.write_all(b"notanumber SET key2 value2\n")
        .await
        .unwrap();
    // Valid entry
    file.write_all(b"1234567891 SET key3 value3\n")
        .await
        .unwrap();
    // Invalid - unknown command
    file.write_all(b"1234567892 BADCMD key4 value4\n")
        .await
        .unwrap();
    // Valid entry
    file.write_all(b"1234567893 DEL key1\n").await.unwrap();
    // Invalid - incomplete entry
    file.write_all(b"1234567894\n").await.unwrap();

    file.sync_all().await.unwrap();

    let recovery_handler = RecoveryHandler::new(aof_path);
    let entries = recovery_handler.recover().await.unwrap();

    // Should have 3 valid entries
    assert_eq!(entries.len(), 3, "Should recover only valid entries");

    // Verify the valid entries
    match &entries[0].command {
        AofCommand::Set { key, value } => {
            assert_eq!(key, "key1");
            assert_eq!(value.to_string(), "value1");
        }
        _ => panic!("Expected SET command"),
    }

    match &entries[1].command {
        AofCommand::Set { key, value } => {
            assert_eq!(key, "key3");
            assert_eq!(value.to_string(), "value3");
        }
        _ => panic!("Expected SET command"),
    }

    match &entries[2].command {
        AofCommand::Delete { key } => {
            assert_eq!(key, "key1");
        }
        _ => panic!("Expected DEL command"),
    }
}

/// Test: Recovery handles partially written entry (simulating crash mid-write)
#[tokio::test]
async fn test_recovery_partial_entry() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("partial.aof");

    let mut file = fs::File::create(&aof_path).await.unwrap();

    // Complete valid entry
    file.write_all(b"1234567890 SET key1 value1\n")
        .await
        .unwrap();
    // Partial entry (no newline - simulates crash mid-write)
    file.write_all(b"1234567891 SET key2 val").await.unwrap();

    file.sync_all().await.unwrap();

    let recovery_handler = RecoveryHandler::new(aof_path);
    let entries = recovery_handler.recover().await.unwrap();

    // Should have at least the first complete entry
    assert!(!entries.is_empty(), "Should recover at least one entry");

    match &entries[0].command {
        AofCommand::Set { key, value } => {
            assert_eq!(key, "key1");
            assert_eq!(value.to_string(), "value1");
        }
        _ => panic!("Expected SET command"),
    }
}

// ==================== Missing File Tests ====================

/// Test: Recovery handles missing AOF file gracefully
#[tokio::test]
async fn test_recovery_missing_file() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("nonexistent.aof");

    // File doesn't exist
    let recovery_handler = RecoveryHandler::new(aof_path);
    let result = recovery_handler.recover().await;

    assert!(result.is_ok(), "Recovery should handle missing file");
    assert!(
        result.unwrap().is_empty(),
        "Missing file should return empty entries"
    );
}

/// Test: Recovery handles empty AOF file
#[tokio::test]
async fn test_recovery_empty_file() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("empty.aof");

    // Create empty file
    fs::File::create(&aof_path).await.unwrap();

    let recovery_handler = RecoveryHandler::new(aof_path);
    let entries = recovery_handler.recover().await.unwrap();

    assert!(entries.is_empty(), "Empty file should return no entries");
}

/// Test: Recovery handles file with only whitespace/empty lines
#[tokio::test]
async fn test_recovery_whitespace_only() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("whitespace.aof");

    let mut file = fs::File::create(&aof_path).await.unwrap();
    file.write_all(b"\n\n   \n\t\n  \n").await.unwrap();
    file.sync_all().await.unwrap();

    let recovery_handler = RecoveryHandler::new(aof_path);
    let entries = recovery_handler.recover().await.unwrap();

    assert!(entries.is_empty(), "Whitespace-only file should return no entries");
}

// ==================== Write Failure Tests ====================

/// Test: AOF writer handles write after close gracefully
#[tokio::test]
async fn test_write_after_close() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("closed.aof");
    let config = create_test_config(aof_path);

    let mut writer = AofWriter::new(&config).await.unwrap();

    // Write one entry
    let entry = AofEntry {
        timestamp: 1234567890,
        command: AofCommand::Set {
            key: "key1".to_string(),
            value: ValueType::String("value1".to_string()),
        },
    };
    writer.write_entry(entry).await.unwrap();

    // Close the writer
    writer.close().await.unwrap();

    // After close, file is None, writes should be silently ignored (not panic)
    let entry = AofEntry {
        timestamp: 1234567891,
        command: AofCommand::Set {
            key: "key2".to_string(),
            value: ValueType::String("value2".to_string()),
        },
    };
    // This should not panic even though the file is closed
    let result = writer.write_entry(entry).await;
    assert!(result.is_ok(), "Write after close should not panic");
}

// ==================== Entry Format Tests ====================

/// Test: Recovery handles all command types correctly
#[tokio::test]
async fn test_recovery_all_command_types() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("all_commands.aof");

    let mut file = fs::File::create(&aof_path).await.unwrap();

    // SET command
    file.write_all(b"1234567890 SET mykey myvalue\n")
        .await
        .unwrap();
    // SETEX command (set with expiration)
    file.write_all(b"1234567891 SETEX mykey2 1734567890 expiring_value\n")
        .await
        .unwrap();
    // DEL command
    file.write_all(b"1234567892 DEL mykey\n").await.unwrap();
    // EXPIREAT command
    file.write_all(b"1234567893 EXPIREAT mykey2 1734567900\n")
        .await
        .unwrap();

    file.sync_all().await.unwrap();

    let recovery_handler = RecoveryHandler::new(aof_path);
    let entries = recovery_handler.recover().await.unwrap();

    assert_eq!(entries.len(), 4);

    // Verify SET
    match &entries[0].command {
        AofCommand::Set { key, value } => {
            assert_eq!(key, "mykey");
            assert_eq!(value.to_string(), "myvalue");
        }
        _ => panic!("Expected SET command"),
    }

    // Verify SETEX
    match &entries[1].command {
        AofCommand::SetWithExpiration {
            key,
            value,
            expires_at,
        } => {
            assert_eq!(key, "mykey2");
            assert_eq!(value.to_string(), "expiring_value");
            assert_eq!(*expires_at, 1734567890);
        }
        _ => panic!("Expected SETEX command"),
    }

    // Verify DEL
    match &entries[2].command {
        AofCommand::Delete { key } => {
            assert_eq!(key, "mykey");
        }
        _ => panic!("Expected DEL command"),
    }

    // Verify EXPIREAT
    match &entries[3].command {
        AofCommand::Expire { key, expires_at } => {
            assert_eq!(key, "mykey2");
            assert_eq!(*expires_at, 1734567900);
        }
        _ => panic!("Expected EXPIREAT command"),
    }
}

/// Test: Recovery handles values with spaces
#[tokio::test]
async fn test_recovery_value_with_spaces() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("spaces.aof");

    let mut file = fs::File::create(&aof_path).await.unwrap();
    file.write_all(b"1234567890 SET mykey hello world with spaces\n")
        .await
        .unwrap();
    file.sync_all().await.unwrap();

    let recovery_handler = RecoveryHandler::new(aof_path);
    let entries = recovery_handler.recover().await.unwrap();

    assert_eq!(entries.len(), 1);

    match &entries[0].command {
        AofCommand::Set { key, value } => {
            assert_eq!(key, "mykey");
            assert_eq!(value.to_string(), "hello world with spaces");
        }
        _ => panic!("Expected SET command"),
    }
}

/// Test: Recovery correctly parses integer values
#[tokio::test]
async fn test_recovery_integer_value() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("integer.aof");

    let mut file = fs::File::create(&aof_path).await.unwrap();
    file.write_all(b"1234567890 SET counter 42\n")
        .await
        .unwrap();
    file.write_all(b"1234567891 SET negative -100\n")
        .await
        .unwrap();
    file.sync_all().await.unwrap();

    let recovery_handler = RecoveryHandler::new(aof_path);
    let entries = recovery_handler.recover().await.unwrap();

    assert_eq!(entries.len(), 2);

    // First entry should be parsed as integer
    match &entries[0].command {
        AofCommand::Set { key, value } => {
            assert_eq!(key, "counter");
            assert_eq!(value.to_integer().unwrap(), 42);
        }
        _ => panic!("Expected SET command"),
    }

    // Negative integer
    match &entries[1].command {
        AofCommand::Set { key, value } => {
            assert_eq!(key, "negative");
            assert_eq!(value.to_integer().unwrap(), -100);
        }
        _ => panic!("Expected SET command"),
    }
}

// ==================== Write-Read Roundtrip Tests ====================

/// Test: Written entries can be recovered correctly
#[tokio::test]
async fn test_write_read_roundtrip() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("roundtrip.aof");
    let config = create_test_config(aof_path.clone());

    // Write entries
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
        writer.close().await.unwrap();
    }

    // Read entries back
    let recovery_handler = RecoveryHandler::new(aof_path);
    let entries = recovery_handler.recover().await.unwrap();

    assert_eq!(entries.len(), 3);

    // Verify roundtrip
    assert_eq!(entries[0].timestamp, 1234567890);
    assert_eq!(entries[1].timestamp, 1234567891);
    assert_eq!(entries[2].timestamp, 1234567892);
}

/// Test: Large number of entries can be written and recovered
#[tokio::test]
async fn test_large_aof_roundtrip() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("large.aof");
    let config = create_test_config(aof_path.clone());

    let num_entries = 1000;

    // Write many entries
    {
        let mut writer = AofWriter::new(&config).await.unwrap();

        for i in 0..num_entries {
            let entry = AofEntry {
                timestamp: 1234567890 + i,
                command: AofCommand::Set {
                    key: format!("key_{}", i),
                    value: ValueType::String(format!("value_{}", i)),
                },
            };
            writer.write_entry(entry).await.unwrap();
        }
        writer.flush().await.unwrap();
        writer.close().await.unwrap();
    }

    // Recover all entries
    let recovery_handler = RecoveryHandler::new(aof_path);
    let entries = recovery_handler.recover().await.unwrap();

    assert_eq!(
        entries.len(),
        num_entries as usize,
        "Should recover all {} entries",
        num_entries
    );
}
