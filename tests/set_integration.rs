//! TCP integration tests for set commands (SADD, SREM, SMEMBERS, SCARD, SISMEMBER, SPOP, SRANDMEMBER)
//!
//! These tests verify the complete set command flow over TCP connections,
//! including proper RESP encoding/decoding and concurrent access.

use rustypotato::{Config, RustyPotatoServer};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Barrier;
use tokio::time::timeout;

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

// ==================== SADD Tests ====================

#[tokio::test]
async fn test_sadd_single() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // SADD myset a
    let cmd = build_resp_command(&["SADD", "myset", "a"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n", "SADD should return 1 for new member");
}

#[tokio::test]
async fn test_sadd_multiple() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // SADD myset a b c
    let cmd = build_resp_command(&["SADD", "myset", "a", "b", "c"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":3\r\n", "SADD should return 3 for three new members");
}

#[tokio::test]
async fn test_sadd_duplicates() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // SADD myset a b
    let cmd = build_resp_command(&["SADD", "myset", "a", "b"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":2\r\n");

    // SADD myset a c (a already exists)
    let cmd = build_resp_command(&["SADD", "myset", "a", "c"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n", "SADD should return 1 (only c is new)");
}

// ==================== SREM Tests ====================

#[tokio::test]
async fn test_srem_existing() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: SADD myset a b c
    let cmd = build_resp_command(&["SADD", "myset", "a", "b", "c"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // SREM myset a b
    let cmd = build_resp_command(&["SREM", "myset", "a", "b"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":2\r\n", "SREM should return 2 for two removed members");
}

#[tokio::test]
async fn test_srem_nonexistent() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: SADD myset a
    let cmd = build_resp_command(&["SADD", "myset", "a"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // SREM myset x y (don't exist)
    let cmd = build_resp_command(&["SREM", "myset", "x", "y"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n", "SREM should return 0 for nonexistent members");
}

#[tokio::test]
async fn test_srem_nonexistent_key() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // SREM nonexistent a
    let cmd = build_resp_command(&["SREM", "nonexistent", "a"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n", "SREM should return 0 for nonexistent key");
}

// ==================== SMEMBERS Tests ====================

#[tokio::test]
async fn test_smembers() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: SADD myset a b c
    let cmd = build_resp_command(&["SADD", "myset", "a", "b", "c"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // SMEMBERS myset
    let cmd = build_resp_command(&["SMEMBERS", "myset"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*3\r\n"), "SMEMBERS should return array with 3 elements");
}

#[tokio::test]
async fn test_smembers_empty_set() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // SMEMBERS nonexistent
    let cmd = build_resp_command(&["SMEMBERS", "nonexistent"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"*0\r\n", "SMEMBERS should return empty array for nonexistent key");
}

// ==================== SCARD Tests ====================

#[tokio::test]
async fn test_scard() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: SADD myset a b c
    let cmd = build_resp_command(&["SADD", "myset", "a", "b", "c"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // SCARD myset
    let cmd = build_resp_command(&["SCARD", "myset"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":3\r\n", "SCARD should return 3");
}

#[tokio::test]
async fn test_scard_nonexistent() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // SCARD nonexistent
    let cmd = build_resp_command(&["SCARD", "nonexistent"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n", "SCARD should return 0 for nonexistent key");
}

// ==================== SISMEMBER Tests ====================

#[tokio::test]
async fn test_sismember_exists() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: SADD myset a b
    let cmd = build_resp_command(&["SADD", "myset", "a", "b"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // SISMEMBER myset a
    let cmd = build_resp_command(&["SISMEMBER", "myset", "a"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n", "SISMEMBER should return 1 for existing member");
}

#[tokio::test]
async fn test_sismember_not_exists() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: SADD myset a
    let cmd = build_resp_command(&["SADD", "myset", "a"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // SISMEMBER myset x
    let cmd = build_resp_command(&["SISMEMBER", "myset", "x"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n", "SISMEMBER should return 0 for nonexistent member");
}

#[tokio::test]
async fn test_sismember_nonexistent_key() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // SISMEMBER nonexistent a
    let cmd = build_resp_command(&["SISMEMBER", "nonexistent", "a"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n", "SISMEMBER should return 0 for nonexistent key");
}

// ==================== SPOP Tests ====================

#[tokio::test]
async fn test_spop_single() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: SADD myset a b c
    let cmd = build_resp_command(&["SADD", "myset", "a", "b", "c"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // SPOP myset
    let cmd = build_resp_command(&["SPOP", "myset"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("$1\r\n"), "SPOP should return bulk string");

    // Verify set size decreased
    let cmd = build_resp_command(&["SCARD", "myset"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":2\r\n", "SCARD should return 2 after SPOP");
}

#[tokio::test]
async fn test_spop_with_count() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: SADD myset a b c
    let cmd = build_resp_command(&["SADD", "myset", "a", "b", "c"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // SPOP myset 2
    let cmd = build_resp_command(&["SPOP", "myset", "2"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*2\r\n"), "SPOP with count should return array with 2 elements");

    // Verify set size decreased
    let cmd = build_resp_command(&["SCARD", "myset"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n", "SCARD should return 1 after SPOP 2");
}

#[tokio::test]
async fn test_spop_nonexistent() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // SPOP nonexistent
    let cmd = build_resp_command(&["SPOP", "nonexistent"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$-1\r\n", "SPOP should return nil for nonexistent key");
}

// ==================== SRANDMEMBER Tests ====================

#[tokio::test]
async fn test_srandmember_single() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: SADD myset a b c
    let cmd = build_resp_command(&["SADD", "myset", "a", "b", "c"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // SRANDMEMBER myset
    let cmd = build_resp_command(&["SRANDMEMBER", "myset"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("$1\r\n"), "SRANDMEMBER should return bulk string");

    // Verify set size unchanged (SRANDMEMBER doesn't remove)
    let cmd = build_resp_command(&["SCARD", "myset"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":3\r\n", "SCARD should still return 3 after SRANDMEMBER");
}

#[tokio::test]
async fn test_srandmember_with_positive_count() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: SADD myset a b c
    let cmd = build_resp_command(&["SADD", "myset", "a", "b", "c"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // SRANDMEMBER myset 2
    let cmd = build_resp_command(&["SRANDMEMBER", "myset", "2"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*2\r\n"), "SRANDMEMBER with count should return array with 2 elements");
}

#[tokio::test]
async fn test_srandmember_with_negative_count() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: SADD myset a b
    let cmd = build_resp_command(&["SADD", "myset", "a", "b"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // SRANDMEMBER myset -5 (allows duplicates, can return more than set size)
    let cmd = build_resp_command(&["SRANDMEMBER", "myset", "-5"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*5\r\n"), "SRANDMEMBER with negative count should return 5 elements");
}

#[tokio::test]
async fn test_srandmember_nonexistent() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // SRANDMEMBER nonexistent
    let cmd = build_resp_command(&["SRANDMEMBER", "nonexistent"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$-1\r\n", "SRANDMEMBER should return nil for nonexistent key");
}

// ==================== Type Mismatch Tests ====================

#[tokio::test]
async fn test_set_operations_on_string_key() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Setup: SET mykey value
    let cmd = build_resp_command(&["SET", "mykey", "value"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // SADD on string key should return WRONGTYPE error
    let cmd = build_resp_command(&["SADD", "mykey", "a"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("WRONGTYPE"), "SADD on string key should return WRONGTYPE");

    // SREM on string key
    let cmd = build_resp_command(&["SREM", "mykey", "a"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("WRONGTYPE"), "SREM on string key should return WRONGTYPE");

    // SMEMBERS on string key
    let cmd = build_resp_command(&["SMEMBERS", "mykey"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("WRONGTYPE"), "SMEMBERS on string key should return WRONGTYPE");

    // SCARD on string key
    let cmd = build_resp_command(&["SCARD", "mykey"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("WRONGTYPE"), "SCARD on string key should return WRONGTYPE");

    // SISMEMBER on string key
    let cmd = build_resp_command(&["SISMEMBER", "mykey", "a"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("WRONGTYPE"), "SISMEMBER on string key should return WRONGTYPE");

    // SPOP on string key
    let cmd = build_resp_command(&["SPOP", "mykey"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("WRONGTYPE"), "SPOP on string key should return WRONGTYPE");

    // SRANDMEMBER on string key
    let cmd = build_resp_command(&["SRANDMEMBER", "mykey"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("WRONGTYPE"), "SRANDMEMBER on string key should return WRONGTYPE");
}

// ==================== Concurrent Access Tests ====================

#[tokio::test]
async fn test_concurrent_sadd() {
    let (_server, addr) = create_and_start_test_server().await;

    let barrier = Arc::new(Barrier::new(10));
    let mut handles = vec![];

    for i in 0..10 {
        let barrier_clone = Arc::clone(&barrier);
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();

            // Wait for all tasks to be ready
            barrier_clone.wait().await;

            // Each task adds 10 unique members
            for j in 0..10 {
                let member = format!("member_{}_{}", i, j);
                let cmd = build_resp_command(&["SADD", "concurrent_set", &member]);
                send_command(&mut stream, &cmd).await.unwrap();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Verify final count
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let cmd = build_resp_command(&["SCARD", "concurrent_set"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":100\r\n", "SCARD should return 100 after concurrent SADDs");
}

#[tokio::test]
async fn test_concurrent_srem() {
    let (_server, addr) = create_and_start_test_server().await;

    // Setup: Add 100 members
    let mut stream = TcpStream::connect(addr).await.unwrap();
    for i in 0..100 {
        let member = format!("member_{}", i);
        let cmd = build_resp_command(&["SADD", "concurrent_set", &member]);
        send_command(&mut stream, &cmd).await.unwrap();
    }

    let barrier = Arc::new(Barrier::new(10));
    let mut handles = vec![];

    for i in 0..10 {
        let barrier_clone = Arc::clone(&barrier);
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();

            barrier_clone.wait().await;

            // Each task removes 10 members
            for j in 0..10 {
                let member = format!("member_{}", i * 10 + j);
                let cmd = build_resp_command(&["SREM", "concurrent_set", &member]);
                send_command(&mut stream, &cmd).await.unwrap();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Verify final count
    let cmd = build_resp_command(&["SCARD", "concurrent_set"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n", "SCARD should return 0 after concurrent SREMs");
}

// ==================== Integration Tests ====================

#[tokio::test]
async fn test_set_operations_workflow() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Add members
    let cmd = build_resp_command(&["SADD", "myset", "apple", "banana", "cherry"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":3\r\n");

    // Check cardinality
    let cmd = build_resp_command(&["SCARD", "myset"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":3\r\n");

    // Check membership
    let cmd = build_resp_command(&["SISMEMBER", "myset", "banana"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    let cmd = build_resp_command(&["SISMEMBER", "myset", "grape"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n");

    // Remove member
    let cmd = build_resp_command(&["SREM", "myset", "banana"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    // Verify removal
    let cmd = build_resp_command(&["SCARD", "myset"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":2\r\n");

    let cmd = build_resp_command(&["SISMEMBER", "myset", "banana"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n");
}

#[tokio::test]
async fn test_srem_deletes_empty_set() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Add single member
    let cmd = build_resp_command(&["SADD", "myset", "only_member"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    // Remove the only member
    let cmd = build_resp_command(&["SREM", "myset", "only_member"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    // Key should no longer exist (EXISTS returns 0)
    let cmd = build_resp_command(&["EXISTS", "myset"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n", "Key should be deleted when set becomes empty");
}

#[tokio::test]
async fn test_spop_count_larger_than_set() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Add 3 members
    let cmd = build_resp_command(&["SADD", "myset", "a", "b", "c"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // SPOP with count larger than set size
    let cmd = build_resp_command(&["SPOP", "myset", "10"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*3\r\n"), "SPOP should return all 3 elements");

    // Set should be empty
    let cmd = build_resp_command(&["SCARD", "myset"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n");
}
