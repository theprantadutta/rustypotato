//! Persistence failure injection tests against the RESP-framed AOF.
//!
//! Tests the system's resilience to AOF-related failures:
//! - Corrupted/truncated AOF file recovery
//! - Missing AOF file handling
//! - Recovery with garbage tail bytes after valid frames
//! - Disabled-AOF no-op behavior

use bytes::{BufMut, Bytes, BytesMut};
use rustypotato::config::{Config, FsyncPolicy, StorageConfig};
use rustypotato::storage::{replay_aof_file, PersistenceManager};
use std::path::PathBuf;
use tempfile::tempdir;
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Create a test config with AOF enabled.
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

/// Encode a SET command as a RESP frame.
fn frame_set(key: &str, value: &str) -> Bytes {
    let mut buf = BytesMut::new();
    buf.put_slice(b"*3\r\n$3\r\nSET\r\n");
    buf.put_slice(format!("${}\r\n", key.len()).as_bytes());
    buf.put_slice(key.as_bytes());
    buf.put_slice(b"\r\n");
    buf.put_slice(format!("${}\r\n", value.len()).as_bytes());
    buf.put_slice(value.as_bytes());
    buf.put_slice(b"\r\n");
    buf.freeze()
}

// ==================== Missing File ====================

/// Recovery on a missing AOF returns 0 commands and does not error.
#[tokio::test]
async fn test_recovery_missing_file() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("does-not-exist.aof");

    let count = replay_aof_file(&aof_path, |_| async { Ok(()) })
        .await
        .unwrap();
    assert_eq!(count, 0);
}

// ==================== Corrupted File ====================

/// Recovery on a completely garbage file does not panic. Replay stops at
/// the first parse error; commands replayed before the error remain valid.
#[tokio::test]
async fn test_recovery_completely_corrupted_file() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("corrupted.aof");

    let mut file = fs::File::create(&aof_path).await.unwrap();
    file.write_all(b"garbage not valid resp format\n")
        .await
        .unwrap();
    file.write_all(b"more nonsense here\n").await.unwrap();
    file.sync_all().await.unwrap();
    drop(file);

    let mut replayed = vec![];
    let count = replay_aof_file(&aof_path, |cmd| {
        replayed.push(cmd.name);
        async { Ok(()) }
    })
    .await
    .unwrap();

    assert_eq!(count, 0, "no valid frames in garbage file");
    assert!(replayed.is_empty());
}

// ==================== Tail Truncation ====================

/// AOF with valid frames followed by a half-frame tail: replays the valid
/// frames, then stops at the truncation without losing the prefix.
#[tokio::test]
async fn test_recovery_truncated_tail() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("truncated.aof");

    // Write two complete SET frames plus a partial third frame at the tail.
    let mut file = fs::File::create(&aof_path).await.unwrap();
    file.write_all(&frame_set("a", "1")).await.unwrap();
    file.write_all(&frame_set("b", "2")).await.unwrap();
    // Partial frame: incomplete bulk string declaration
    file.write_all(b"*3\r\n$3\r\nSET\r\n$5\r\nbroke")
        .await
        .unwrap();
    file.sync_all().await.unwrap();
    drop(file);

    let mut keys = vec![];
    let count = replay_aof_file(&aof_path, |cmd| {
        keys.push(cmd.args[0].clone());
        async { Ok(()) }
    })
    .await
    .unwrap();

    assert_eq!(count, 2, "should replay the two complete prefix frames");
    assert_eq!(keys, vec!["a", "b"]);
}

// ==================== Garbage After Valid Frames ====================

/// Recovery does not crash if an AOF has valid frames followed by junk.
/// Behavior: stop replay at the first parse error.
#[tokio::test]
async fn test_recovery_valid_then_garbage() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("valid_then_garbage.aof");

    let mut file = fs::File::create(&aof_path).await.unwrap();
    file.write_all(&frame_set("first", "ok")).await.unwrap();
    file.write_all(&frame_set("second", "ok")).await.unwrap();
    file.write_all(b"\xff\xfe\xfd not valid resp\r\n")
        .await
        .unwrap();
    file.sync_all().await.unwrap();
    drop(file);

    let mut replayed = vec![];
    let _ = replay_aof_file(&aof_path, |cmd| {
        replayed.push(cmd.args[0].clone());
        async { Ok(()) }
    })
    .await
    .unwrap();

    assert!(
        replayed.contains(&"first".to_string()),
        "first frame must be replayed before parser hits garbage"
    );
}

// ==================== Round Trip Through PersistenceManager ====================

/// End-to-end: log via PersistenceManager, shut down, re-open, replay.
#[tokio::test]
async fn test_persistence_manager_round_trip() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("roundtrip.aof");
    let cfg = create_test_config(aof_path.clone());

    let mgr = PersistenceManager::new(&cfg).await.unwrap();
    mgr.log_command(frame_set("k1", "v1")).await.unwrap();
    mgr.log_command(frame_set("k2", "v2")).await.unwrap();
    mgr.log_command(frame_set("k3", "v3")).await.unwrap();
    mgr.flush().await.unwrap();
    mgr.shutdown().await.unwrap();

    let mut keys = vec![];
    let count = replay_aof_file(&aof_path, |cmd| {
        keys.push(cmd.args[0].clone());
        async { Ok(()) }
    })
    .await
    .unwrap();

    assert_eq!(count, 3);
    assert_eq!(keys, vec!["k1", "k2", "k3"]);
}

// ==================== Disabled AOF ====================

/// When AOF is disabled, log_command and friends are no-ops, no file is
/// created.
#[tokio::test]
async fn test_disabled_aof_creates_no_file() {
    let temp_dir = tempdir().unwrap();
    let aof_path = temp_dir.path().join("nope.aof");
    let mut cfg = create_test_config(aof_path.clone());
    cfg.storage.aof_enabled = false;

    let mgr = PersistenceManager::new(&cfg).await.unwrap();
    assert!(!mgr.is_enabled());
    mgr.log_command(frame_set("ignored", "value"))
        .await
        .unwrap();
    mgr.flush().await.unwrap();
    assert_eq!(mgr.file_size().await.unwrap(), 0);
    assert!(!aof_path.exists());
}
