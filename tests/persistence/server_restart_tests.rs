//! Persistence tests with server restart scenarios
//!
//! These tests verify that data survives server restarts through the AOF
//! persistence mechanism and that recovery works correctly.

use rustypotato::{Config, RustyPotatoServer, MemoryStore};
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};

/// Helper function to send RESP command and read response
async fn send_command(stream: &mut TcpStream, command: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    stream.write_all(command).await?;
    stream.flush().await?;
    
    let mut buffer = vec![0u8; 4096];
    let n = timeout(Duration::from_secs(10), stream.read(&mut buffer)).await??;
    buffer.truncate(n);
    Ok(buffer)
}

/// Create a test configuration with persistence enabled
fn create_persistent_config(aof_path: PathBuf) -> Config {
    let mut config = Config::default();
    config.server.port = 0; // Use random port
    config.storage.aof_enabled = true;
    config.storage.aof_path = aof_path;
    // Note: aof_fsync_interval removed, using aof_fsync_policy instead
    config
}

#[tokio::test]
async fn test_basic_persistence_and_recovery() {
    let temp_dir = TempDir::new().unwrap();
    let aof_path = temp_dir.path().join("test_basic.aof");
    
    // Phase 1: Start server, add data, shutdown
    let addr = {
        let config = create_persistent_config(aof_path.clone());
        let mut server = RustyPotatoServer::new(config).unwrap();
        let addr = server.start_with_addr().await.unwrap();
        
        sleep(Duration::from_millis(100)).await;
        
        // Connect and add test data
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Set multiple keys
        let test_data = vec![
            ("key1", "value1"),
            ("key2", "value2"),
            ("counter", "42"),
            ("unicode_key", "🚀 unicode value 🎯"),
        ];
        
        for (key, value) in &test_data {
            let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                                 key.len(), key, value.len(), value);
            let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b"+OK\r\n");
        }
        
        // Increment counter
        let incr_cmd = b"*2\r\n$4\r\nINCR\r\n$7\r\ncounter\r\n";
        let response = send_command(&mut stream, incr_cmd).await.unwrap();
        assert_eq!(response, b":43\r\n");
        
        drop(stream);
        server.shutdown().await.unwrap();
        
        addr
    };
    
    // Verify AOF file was created
    assert!(aof_path.exists());
    
    // Phase 2: Start new server instance and verify data recovery
    {
        let config = create_persistent_config(aof_path.clone());
        let mut server = RustyPotatoServer::new(config).unwrap();
        let new_addr = server.start_with_addr().await.unwrap();
        
        sleep(Duration::from_millis(100)).await;
        
        // Connect and verify data was recovered
        let mut stream = TcpStream::connect(new_addr).await.unwrap();
        
        // Check all original keys
        let get_key1 = b"*2\r\n$3\r\nGET\r\n$4\r\nkey1\r\n";
        let response = send_command(&mut stream, get_key1).await.unwrap();
        assert_eq!(response, b"$6\r\nvalue1\r\n");
        
        let get_key2 = b"*2\r\n$3\r\nGET\r\n$4\r\nkey2\r\n";
        let response = send_command(&mut stream, get_key2).await.unwrap();
        assert_eq!(response, b"$6\r\nvalue2\r\n");
        
        // Check incremented counter
        let get_counter = b"*2\r\n$3\r\nGET\r\n$7\r\ncounter\r\n";
        let response = send_command(&mut stream, get_counter).await.unwrap();
        assert_eq!(response, b"$2\r\n43\r\n");
        
        // Check unicode key
        let get_unicode = b"*2\r\n$3\r\nGET\r\n$11\r\nunicode_key\r\n";
        let response = send_command(&mut stream, get_unicode).await.unwrap();
        let expected_unicode = "🚀 unicode value 🎯";
        let expected_response = format!("${}\r\n{}\r\n", expected_unicode.len(), expected_unicode);
        assert_eq!(response, expected_response.as_bytes());
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn test_persistence_with_ttl() {
    let temp_dir = TempDir::new().unwrap();
    let aof_path = temp_dir.path().join("test_ttl.aof");
    
    // Phase 1: Set keys with TTL
    {
        let config = create_persistent_config(aof_path.clone());
        let mut server = RustyPotatoServer::new(config).unwrap();
        let addr = server.start_with_addr().await.unwrap();
        
        sleep(Duration::from_millis(100)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Set key with long TTL
        let set_cmd = b"*3\r\n$3\r\nSET\r\n$8\r\nlong_ttl\r\n$5\r\nvalue\r\n";
        let response = send_command(&mut stream, set_cmd).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        let expire_cmd = b"*3\r\n$6\r\nEXPIRE\r\n$8\r\nlong_ttl\r\n$3\r\n300\r\n";
        let response = send_command(&mut stream, expire_cmd).await.unwrap();
        assert_eq!(response, b":1\r\n");
        
        // Set key with short TTL (will expire before restart)
        let set_short_cmd = b"*3\r\n$3\r\nSET\r\n$9\r\nshort_ttl\r\n$5\r\nvalue\r\n";
        let response = send_command(&mut stream, set_short_cmd).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        let expire_short_cmd = b"*3\r\n$6\r\nEXPIRE\r\n$9\r\nshort_ttl\r\n$1\r\n1\r\n";
        let response = send_command(&mut stream, expire_short_cmd).await.unwrap();
        assert_eq!(response, b":1\r\n");
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
    
    // Wait for short TTL to expire
    sleep(Duration::from_millis(1100)).await;
    
    // Phase 2: Restart and verify TTL handling
    {
        let config = create_persistent_config(aof_path.clone());
        let mut server = RustyPotatoServer::new(config).unwrap();
        let addr = server.start_with_addr().await.unwrap();
        
        sleep(Duration::from_millis(100)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Long TTL key should still exist
        let get_long = b"*2\r\n$3\r\nGET\r\n$8\r\nlong_ttl\r\n";
        let response = send_command(&mut stream, get_long).await.unwrap();
        assert_eq!(response, b"$5\r\nvalue\r\n");
        
        // Check TTL is still set
        let ttl_long = b"*2\r\n$3\r\nTTL\r\n$8\r\nlong_ttl\r\n";
        let response = send_command(&mut stream, ttl_long).await.unwrap();
        let response_str = String::from_utf8_lossy(&response);
        assert!(response_str.starts_with(":"));
        let ttl_value: i64 = response_str.trim_start_matches(':').trim_end_matches("\r\n").parse().unwrap();
        assert!(ttl_value > 0 && ttl_value <= 300);
        
        // Short TTL key should not exist (expired during recovery)
        let get_short = b"*2\r\n$3\r\nGET\r\n$9\r\nshort_ttl\r\n";
        let response = send_command(&mut stream, get_short).await.unwrap();
        assert_eq!(response, b"$-1\r\n");
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn test_persistence_with_deletions() {
    let temp_dir = TempDir::new().unwrap();
    let aof_path = temp_dir.path().join("test_deletions.aof");
    
    // Phase 1: Set and delete keys
    {
        let config = create_persistent_config(aof_path.clone());
        let mut server = RustyPotatoServer::new(config).unwrap();
        let addr = server.start_with_addr().await.unwrap();
        
        sleep(Duration::from_millis(100)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Set multiple keys
        for i in 0..10 {
            let key = format!("delete_test_{}", i);
            let value = format!("value_{}", i);
            let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                                 key.len(), key, value.len(), value);
            let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b"+OK\r\n");
        }
        
        // Delete some keys
        for i in [1, 3, 5, 7, 9] {
            let key = format!("delete_test_{}", i);
            let del_cmd = format!("*2\r\n$3\r\nDEL\r\n${}\r\n{}\r\n", key.len(), key);
            let response = send_command(&mut stream, del_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b":1\r\n");
        }
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
    
    // Phase 2: Restart and verify deletions persisted
    {
        let config = create_persistent_config(aof_path.clone());
        let mut server = RustyPotatoServer::new(config).unwrap();
        let addr = server.start_with_addr().await.unwrap();
        
        sleep(Duration::from_millis(100)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Check that remaining keys exist
        for i in [0, 2, 4, 6, 8] {
            let key = format!("delete_test_{}", i);
            let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
            let response = send_command(&mut stream, get_cmd.as_bytes()).await.unwrap();
            let expected_value = format!("value_{}", i);
            let expected_response = format!("${}\r\n{}\r\n", expected_value.len(), expected_value);
            assert_eq!(response, expected_response.as_bytes());
        }
        
        // Check that deleted keys don't exist
        for i in [1, 3, 5, 7, 9] {
            let key = format!("delete_test_{}", i);
            let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
            let response = send_command(&mut stream, get_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b"$-1\r\n");
        }
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn test_persistence_with_overwrites() {
    let temp_dir = TempDir::new().unwrap();
    let aof_path = temp_dir.path().join("test_overwrites.aof");
    
    // Phase 1: Set and overwrite keys multiple times
    {
        let config = create_persistent_config(aof_path.clone());
        let mut server = RustyPotatoServer::new(config).unwrap();
        let addr = server.start_with_addr().await.unwrap();
        
        sleep(Duration::from_millis(100)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Overwrite the same key multiple times
        let key = "overwrite_test";
        for i in 0..5 {
            let value = format!("version_{}", i);
            let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                                 key.len(), key, value.len(), value);
            let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b"+OK\r\n");
        }
        
        // Change type from string to integer
        let incr_cmd = format!("*2\r\n$4\r\nINCR\r\n${}\r\n{}\r\n", key.len(), key);
        let response = send_command(&mut stream, incr_cmd.as_bytes()).await.unwrap();
        // This should fail because current value is not numeric
        assert!(response.starts_with(b"-ERR"));
        
        // Set as integer
        let set_int_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n$2\r\n42\r\n", key.len(), key);
        let response = send_command(&mut stream, set_int_cmd.as_bytes()).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        // Increment it
        let incr_cmd = format!("*2\r\n$4\r\nINCR\r\n${}\r\n{}\r\n", key.len(), key);
        let response = send_command(&mut stream, incr_cmd.as_bytes()).await.unwrap();
        assert_eq!(response, b":43\r\n");
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
    
    // Phase 2: Restart and verify final value
    {
        let config = create_persistent_config(aof_path.clone());
        let mut server = RustyPotatoServer::new(config).unwrap();
        let addr = server.start_with_addr().await.unwrap();
        
        sleep(Duration::from_millis(100)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Should have the final incremented value
        let get_cmd = b"*2\r\n$3\r\nGET\r\n$14\r\noverwrite_test\r\n";
        let response = send_command(&mut stream, get_cmd).await.unwrap();
        assert_eq!(response, b"$2\r\n43\r\n");
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn test_multiple_restart_cycles() {
    let temp_dir = TempDir::new().unwrap();
    let aof_path = temp_dir.path().join("test_multiple_restarts.aof");
    
    // Perform multiple restart cycles, adding data each time
    for cycle in 0..3 {
        let config = create_persistent_config(aof_path.clone());
        let mut server = RustyPotatoServer::new(config).unwrap();
        let addr = server.start_with_addr().await.unwrap();
        
        sleep(Duration::from_millis(100)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Add data for this cycle
        let key = format!("cycle_{}_key", cycle);
        let value = format!("cycle_{}_value", cycle);
        let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                             key.len(), key, value.len(), value);
        let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        // Increment a shared counter
        let incr_cmd = b"*2\r\n$4\r\nINCR\r\n$14\r\nrestart_counter\r\n";
        let response = send_command(&mut stream, incr_cmd).await.unwrap();
        let expected_counter = format!(":{}\r\n", cycle + 1);
        assert_eq!(response, expected_counter.as_bytes());
        
        // Verify all previous cycle data still exists
        for prev_cycle in 0..cycle {
            let prev_key = format!("cycle_{}_key", prev_cycle);
            let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", prev_key.len(), prev_key);
            let response = send_command(&mut stream, get_cmd.as_bytes()).await.unwrap();
            let expected_value = format!("cycle_{}_value", prev_cycle);
            let expected_response = format!("${}\r\n{}\r\n", expected_value.len(), expected_value);
            assert_eq!(response, expected_response.as_bytes());
        }
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
    
    // Final verification
    {
        let config = create_persistent_config(aof_path.clone());
        let mut server = RustyPotatoServer::new(config).unwrap();
        let addr = server.start_with_addr().await.unwrap();
        
        sleep(Duration::from_millis(100)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Verify all cycle data exists
        for cycle in 0..3 {
            let key = format!("cycle_{}_key", cycle);
            let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
            let response = send_command(&mut stream, get_cmd.as_bytes()).await.unwrap();
            let expected_value = format!("cycle_{}_value", cycle);
            let expected_response = format!("${}\r\n{}\r\n", expected_value.len(), expected_value);
            assert_eq!(response, expected_response.as_bytes());
        }
        
        // Verify counter has correct final value
        let get_counter = b"*2\r\n$3\r\nGET\r\n$14\r\nrestart_counter\r\n";
        let response = send_command(&mut stream, get_counter).await.unwrap();
        assert_eq!(response, b"$1\r\n3\r\n");
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn test_persistence_with_large_dataset() {
    let temp_dir = TempDir::new().unwrap();
    let aof_path = temp_dir.path().join("test_large_dataset.aof");
    
    let key_count = 1000;
    
    // Phase 1: Create large dataset
    {
        let config = create_persistent_config(aof_path.clone());
        let mut server = RustyPotatoServer::new(config).unwrap();
        let addr = server.start_with_addr().await.unwrap();
        
        sleep(Duration::from_millis(100)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Set many keys
        for i in 0..key_count {
            let key = format!("large_key_{:04}", i);
            let value = format!("large_value_{:04}_{}", i, "x".repeat(100)); // ~100 char values
            let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                                 key.len(), key, value.len(), value);
            let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b"+OK\r\n");
            
            // Add some variety with counters
            if i % 10 == 0 {
                let counter_key = format!("counter_{}", i / 10);
                let incr_cmd = format!("*2\r\n$4\r\nINCR\r\n${}\r\n{}\r\n", counter_key.len(), counter_key);
                let _response = send_command(&mut stream, incr_cmd.as_bytes()).await.unwrap();
            }
        }
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
    
    // Verify AOF file is substantial
    let aof_metadata = std::fs::metadata(&aof_path).unwrap();
    assert!(aof_metadata.len() > 10000); // Should be substantial
    
    // Phase 2: Restart and verify all data
    {
        let config = create_persistent_config(aof_path.clone());
        let mut server = RustyPotatoServer::new(config).unwrap();
        let addr = server.start_with_addr().await.unwrap();
        
        sleep(Duration::from_millis(500)).await; // Give more time for large recovery
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Verify random sample of keys
        let sample_indices = [0, 100, 250, 500, 750, 999];
        for &i in &sample_indices {
            let key = format!("large_key_{:04}", i);
            let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
            let response = send_command(&mut stream, get_cmd.as_bytes()).await.unwrap();
            
            let expected_value = format!("large_value_{:04}_{}", i, "x".repeat(100));
            let expected_response = format!("${}\r\n{}\r\n", expected_value.len(), expected_value);
            assert_eq!(response, expected_response.as_bytes());
        }
        
        // Verify some counters
        for i in [0, 50, 99] {
            let counter_key = format!("counter_{}", i);
            let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", counter_key.len(), counter_key);
            let response = send_command(&mut stream, get_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b"$1\r\n1\r\n");
        }
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn test_persistence_error_recovery() {
    let temp_dir = TempDir::new().unwrap();
    let aof_path = temp_dir.path().join("test_error_recovery.aof");
    
    // Phase 1: Create valid AOF file
    {
        let config = create_persistent_config(aof_path.clone());
        let mut server = RustyPotatoServer::new(config).unwrap();
        let addr = server.start_with_addr().await.unwrap();
        
        sleep(Duration::from_millis(100)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Add some valid data
        let set_cmd = b"*3\r\n$3\r\nSET\r\n$9\r\nvalid_key\r\n$11\r\nvalid_value\r\n";
        let response = send_command(&mut stream, set_cmd).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
    
    // Phase 2: Corrupt AOF file (append invalid data)
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        
        let mut file = OpenOptions::new()
            .append(true)
            .open(&aof_path)
            .unwrap();
        
        // Append some invalid data
        writeln!(file, "INVALID_AOF_ENTRY").unwrap();
        writeln!(file, "ANOTHER_INVALID_LINE").unwrap();
    }
    
    // Phase 3: Restart and verify recovery handles corruption gracefully
    {
        let config = create_persistent_config(aof_path.clone());
        let server_result = RustyPotatoServer::new(config);
        
        // Server should either:
        // 1. Start successfully and recover valid data (ignoring invalid entries)
        // 2. Fail to start with a clear error message
        
        match server_result {
            Ok(mut server) => {
                // If server starts, it should have recovered the valid data
                let addr = server.start_with_addr().await.unwrap();
                sleep(Duration::from_millis(100)).await;
                
                let mut stream = TcpStream::connect(addr).await.unwrap();
                
                // Valid data should still be there
                let get_cmd = b"*2\r\n$3\r\nGET\r\n$9\r\nvalid_key\r\n";
                let response = send_command(&mut stream, get_cmd).await.unwrap();
                assert_eq!(response, b"$11\r\nvalid_value\r\n");
                
                drop(stream);
                server.shutdown().await.unwrap();
            }
            Err(_) => {
                // If server fails to start, that's also acceptable behavior
                // for corrupted AOF files
            }
        }
    }
}

#[tokio::test]
async fn test_persistence_without_aof_enabled() {
    let temp_dir = TempDir::new().unwrap();
    let aof_path = temp_dir.path().join("test_no_aof.aof");
    
    // Phase 1: Start server without persistence
    {
        let mut config = Config::default();
        config.server.port = 0;
        config.storage.aof_enabled = false; // Explicitly disable
        config.storage.aof_path = aof_path.clone();
        
        let mut server = RustyPotatoServer::new(config).unwrap();
        let addr = server.start_with_addr().await.unwrap();
        
        sleep(Duration::from_millis(100)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Add data
        let set_cmd = b"*3\r\n$3\r\nSET\r\n$12\r\nno_persist_key\r\n$14\r\nno_persist_value\r\n";
        let response = send_command(&mut stream, set_cmd).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
    
    // Verify no AOF file was created
    assert!(!aof_path.exists());
    
    // Phase 2: Start new server - data should not be recovered
    {
        let mut config = Config::default();
        config.server.port = 0;
        config.storage.aof_enabled = false;
        config.storage.aof_path = aof_path.clone();
        
        let mut server = RustyPotatoServer::new(config).unwrap();
        let addr = server.start_with_addr().await.unwrap();
        
        sleep(Duration::from_millis(100)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Data should not exist
        let get_cmd = b"*2\r\n$3\r\nGET\r\n$12\r\nno_persist_key\r\n";
        let response = send_command(&mut stream, get_cmd).await.unwrap();
        assert_eq!(response, b"$-1\r\n");
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
}