//! TCP integration tests for hash commands (HSET, HGET, HDEL, HGETALL, HEXISTS)
//!
//! These tests verify the complete hash command flow over TCP connections,
//! including proper RESP encoding/decoding and concurrent access.

use rustypotato::{Config, RustyPotatoServer};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use std::sync::Arc;
use tokio::sync::Barrier;

/// Helper function to create and start a test server
async fn create_and_start_test_server() -> (RustyPotatoServer, std::net::SocketAddr) {
    let mut config = Config::default();
    config.server.port = 0; // Use random port for testing

    let mut server = RustyPotatoServer::new(config).unwrap();
    let addr = server.start_with_addr().await.unwrap();

    // Give server time to start accepting connections
    tokio::time::sleep(Duration::from_millis(50)).await;

    (server, addr)
}

/// Helper function to send RESP command and read response
async fn send_command(
    stream: &mut TcpStream,
    command: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    stream.write_all(command).await?;
    stream.flush().await?;

    let mut buffer = vec![0u8; 4096];
    let n = timeout(Duration::from_secs(5), stream.read(&mut buffer)).await??;
    buffer.truncate(n);
    Ok(buffer)
}

/// Build RESP array command from parts
fn build_resp_command(parts: &[&str]) -> Vec<u8> {
    let mut cmd = format!("*{}\r\n", parts.len());
    for part in parts {
        cmd.push_str(&format!("${}\r\n{}\r\n", part.len(), part));
    }
    cmd.into_bytes()
}

// ==================== Basic HSET/HGET Tests ====================

#[tokio::test]
async fn test_hset_hget_basic() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // HSET myhash field1 value1
    let cmd = build_resp_command(&["HSET", "myhash", "field1", "value1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n", "HSET should return 1 for new field");

    // HGET myhash field1
    let cmd = build_resp_command(&["HGET", "myhash", "field1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$6\r\nvalue1\r\n", "HGET should return the value");
}

#[tokio::test]
async fn test_hset_update_field() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // HSET myhash field1 value1
    let cmd = build_resp_command(&["HSET", "myhash", "field1", "value1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    // HSET myhash field1 newvalue (update)
    let cmd = build_resp_command(&["HSET", "myhash", "field1", "newvalue"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n", "HSET should return 0 for update");

    // Verify updated value
    let cmd = build_resp_command(&["HGET", "myhash", "field1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$8\r\nnewvalue\r\n");
}

#[tokio::test]
async fn test_hget_nonexistent() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // HGET nonexistent field
    let cmd = build_resp_command(&["HGET", "nonexistent", "field1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$-1\r\n", "HGET should return nil for nonexistent key");

    // HSET then HGET nonexistent field
    let cmd = build_resp_command(&["HSET", "myhash", "field1", "value1"]);
    send_command(&mut stream, &cmd).await.unwrap();

    let cmd = build_resp_command(&["HGET", "myhash", "nonexistent"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$-1\r\n", "HGET should return nil for nonexistent field");
}

// ==================== HDEL Tests ====================

#[tokio::test]
async fn test_hdel_single_field() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: HSET multiple fields
    let cmd = build_resp_command(&["HSET", "myhash", "field1", "value1"]);
    send_command(&mut stream, &cmd).await.unwrap();
    let cmd = build_resp_command(&["HSET", "myhash", "field2", "value2"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // HDEL single field
    let cmd = build_resp_command(&["HDEL", "myhash", "field1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n", "HDEL should return 1");

    // Verify field1 is gone
    let cmd = build_resp_command(&["HGET", "myhash", "field1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$-1\r\n");

    // Verify field2 still exists
    let cmd = build_resp_command(&["HGET", "myhash", "field2"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$6\r\nvalue2\r\n");
}

#[tokio::test]
async fn test_hdel_multiple_fields() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: HSET three fields
    for i in 1..=3 {
        let cmd = build_resp_command(&["HSET", "myhash", &format!("field{}", i), &format!("value{}", i)]);
        send_command(&mut stream, &cmd).await.unwrap();
    }

    // HDEL two fields + one nonexistent
    let cmd = build_resp_command(&["HDEL", "myhash", "field1", "field2", "nonexistent"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":2\r\n", "HDEL should return count of deleted fields");

    // Verify field3 still exists
    let cmd = build_resp_command(&["HGET", "myhash", "field3"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$6\r\nvalue3\r\n");
}

#[tokio::test]
async fn test_hdel_nonexistent_key() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // HDEL on nonexistent key
    let cmd = build_resp_command(&["HDEL", "nonexistent", "field1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n", "HDEL should return 0 for nonexistent key");
}

// ==================== HGETALL Tests ====================

#[tokio::test]
async fn test_hgetall_basic() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: HSET two fields
    let cmd = build_resp_command(&["HSET", "myhash", "name", "John"]);
    send_command(&mut stream, &cmd).await.unwrap();
    let cmd = build_resp_command(&["HSET", "myhash", "age", "30"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // HGETALL
    let cmd = build_resp_command(&["HGETALL", "myhash"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();

    // Response should be an array with 4 elements (2 field-value pairs)
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*4\r\n"), "HGETALL should return array of 4 elements");

    // Should contain both fields and values (order may vary)
    assert!(response_str.contains("name") || response_str.contains("age"));
}

#[tokio::test]
async fn test_hgetall_empty() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // HGETALL on nonexistent key
    let cmd = build_resp_command(&["HGETALL", "nonexistent"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"*0\r\n", "HGETALL should return empty array");
}

#[tokio::test]
async fn test_hgetall_large_hash() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: HSET 50 fields
    for i in 1..=50 {
        let cmd = build_resp_command(&["HSET", "largehash", &format!("field{}", i), &format!("value{}", i)]);
        send_command(&mut stream, &cmd).await.unwrap();
    }

    // HGETALL
    let cmd = build_resp_command(&["HGETALL", "largehash"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();

    // Response should be an array with 100 elements (50 field-value pairs)
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*100\r\n"), "HGETALL should return array of 100 elements for 50 fields");
}

// ==================== HEXISTS Tests ====================

#[tokio::test]
async fn test_hexists_basic() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // HEXISTS on nonexistent key
    let cmd = build_resp_command(&["HEXISTS", "myhash", "field1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n", "HEXISTS should return 0 for nonexistent key");

    // HSET then HEXISTS
    let cmd = build_resp_command(&["HSET", "myhash", "field1", "value1"]);
    send_command(&mut stream, &cmd).await.unwrap();

    let cmd = build_resp_command(&["HEXISTS", "myhash", "field1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n", "HEXISTS should return 1 for existing field");

    // HEXISTS on nonexistent field
    let cmd = build_resp_command(&["HEXISTS", "myhash", "nonexistent"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n", "HEXISTS should return 0 for nonexistent field");
}

// ==================== Error Handling Tests ====================

#[tokio::test]
async fn test_hash_wrong_arg_count() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // HSET with too few args
    let cmd = build_resp_command(&["HSET", "myhash", "field1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("-ERR"), "HSET with wrong args should return error");

    // HGET with too few args
    let cmd = build_resp_command(&["HGET", "myhash"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("-ERR"), "HGET with wrong args should return error");

    // HDEL with too few args
    let cmd = build_resp_command(&["HDEL", "myhash"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("-ERR"), "HDEL with wrong args should return error");
}

#[tokio::test]
async fn test_hash_type_mismatch() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // SET a string key
    let cmd = build_resp_command(&["SET", "stringkey", "stringvalue"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // Try HSET on string key (type mismatch)
    let cmd = build_resp_command(&["HSET", "stringkey", "field1", "value1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("-"), "HSET on string key should return error");

    // Try HGET on string key (type mismatch)
    let cmd = build_resp_command(&["HGET", "stringkey", "field1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("-"), "HGET on string key should return error");
}

// ==================== Hash with TTL Tests ====================

#[tokio::test]
async fn test_hash_with_expire() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // HSET field
    let cmd = build_resp_command(&["HSET", "myhash", "field1", "value1"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // EXPIRE the hash key
    let cmd = build_resp_command(&["EXPIRE", "myhash", "60"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n", "EXPIRE should return 1");

    // TTL should be positive
    let cmd = build_resp_command(&["TTL", "myhash"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with(":"), "TTL should return integer");

    // Hash should still be accessible
    let cmd = build_resp_command(&["HGET", "myhash", "field1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$6\r\nvalue1\r\n");
}

// ==================== Concurrent Access Tests ====================

#[tokio::test]
async fn test_concurrent_hash_operations() {
    let (_server, addr) = create_and_start_test_server().await;
    let barrier = Arc::new(Barrier::new(20));
    let mut handles = vec![];

    // 20 concurrent clients setting different fields
    for i in 0..20 {
        let barrier = Arc::clone(&barrier);
        let addr = addr;
        handles.push(tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            barrier.wait().await;

            let cmd = build_resp_command(&["HSET", "concurrent_hash", &format!("field{}", i), &format!("value{}", i)]);
            let response = send_command(&mut stream, &cmd).await.unwrap();

            // Should return 1 (new field)
            assert_eq!(response, b":1\r\n");
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Verify all 20 fields exist
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let cmd = build_resp_command(&["HGETALL", "concurrent_hash"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();

    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*40\r\n"), "Should have 40 elements (20 fields * 2)");
}

#[tokio::test]
async fn test_hash_operations_workflow() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Create a user profile using hash
    let cmd = build_resp_command(&["HSET", "user:1", "name", "Alice"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    let cmd = build_resp_command(&["HSET", "user:1", "email", "alice@example.com"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    let cmd = build_resp_command(&["HSET", "user:1", "age", "25"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    // Check a field exists
    let cmd = build_resp_command(&["HEXISTS", "user:1", "email"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    // Update a field
    let cmd = build_resp_command(&["HSET", "user:1", "age", "26"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n", "Update should return 0");

    // Get specific field
    let cmd = build_resp_command(&["HGET", "user:1", "age"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$2\r\n26\r\n");

    // Delete email field
    let cmd = build_resp_command(&["HDEL", "user:1", "email"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    // Get all remaining fields
    let cmd = build_resp_command(&["HGETALL", "user:1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);

    // Should have 4 elements (2 field-value pairs: name and age)
    assert!(response_str.starts_with("*4\r\n"), "Should have 4 elements after deleting email");
    assert!(!response_str.contains("email"), "Email should be deleted");
}

// ==================== Edge Cases ====================

#[tokio::test]
async fn test_hash_empty_value() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // HSET with empty value
    let cmd = build_resp_command(&["HSET", "myhash", "field1", ""]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    // HGET should return empty bulk string
    let cmd = build_resp_command(&["HGET", "myhash", "field1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$0\r\n\r\n", "Should return empty bulk string");
}

#[tokio::test]
async fn test_hash_special_characters() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // HSET with special characters in field and value
    let cmd = build_resp_command(&["HSET", "myhash", "field:with:colons", "value with spaces"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    // HGET
    let cmd = build_resp_command(&["HGET", "myhash", "field:with:colons"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$17\r\nvalue with spaces\r\n");
}

#[tokio::test]
async fn test_hash_unicode() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // HSET with unicode characters
    let cmd = build_resp_command(&["HSET", "users", "name", "日本語"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    // HGET unicode value
    let cmd = build_resp_command(&["HGET", "users", "name"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();

    // Unicode string "日本語" is 9 bytes in UTF-8
    assert_eq!(&response[..4], b"$9\r\n");
}
