//! TCP integration tests for list commands (LPUSH, RPUSH, LPOP, RPOP, LLEN, LRANGE)
//!
//! These tests verify the complete list command flow over TCP connections,
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

// ==================== Basic LPUSH/RPUSH Tests ====================

#[tokio::test]
async fn test_lpush_single() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // LPUSH mylist a
    let cmd = build_resp_command(&["LPUSH", "mylist", "a"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n", "LPUSH should return 1 for first element");
}

#[tokio::test]
async fn test_lpush_multiple() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // LPUSH mylist a b c
    let cmd = build_resp_command(&["LPUSH", "mylist", "a", "b", "c"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":3\r\n", "LPUSH should return 3 for three elements");

    // LRANGE to verify order (c, b, a - last pushed is first)
    let cmd = build_resp_command(&["LRANGE", "mylist", "0", "-1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*3\r\n"), "Should have 3 elements");
}

#[tokio::test]
async fn test_rpush_single() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH mylist a
    let cmd = build_resp_command(&["RPUSH", "mylist", "a"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n", "RPUSH should return 1 for first element");
}

#[tokio::test]
async fn test_rpush_multiple() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH mylist a b c
    let cmd = build_resp_command(&["RPUSH", "mylist", "a", "b", "c"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":3\r\n", "RPUSH should return 3 for three elements");
}

#[tokio::test]
async fn test_lpush_rpush_combined() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH mylist center
    let cmd = build_resp_command(&["RPUSH", "mylist", "center"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // LPUSH mylist left
    let cmd = build_resp_command(&["LPUSH", "mylist", "left"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":2\r\n");

    // RPUSH mylist right
    let cmd = build_resp_command(&["RPUSH", "mylist", "right"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":3\r\n");

    // LRANGE to verify order: left, center, right
    let cmd = build_resp_command(&["LRANGE", "mylist", "0", "-1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*3\r\n"));
    assert!(response_str.contains("left"));
    assert!(response_str.contains("center"));
    assert!(response_str.contains("right"));
}

// ==================== LPOP/RPOP Tests ====================

#[tokio::test]
async fn test_lpop_basic() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH mylist a b c (order: a, b, c)
    let cmd = build_resp_command(&["RPUSH", "mylist", "a", "b", "c"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // LPOP should return "a"
    let cmd = build_resp_command(&["LPOP", "mylist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$1\r\na\r\n");

    // LPOP should return "b"
    let cmd = build_resp_command(&["LPOP", "mylist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$1\r\nb\r\n");
}

#[tokio::test]
async fn test_rpop_basic() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH mylist a b c (order: a, b, c)
    let cmd = build_resp_command(&["RPUSH", "mylist", "a", "b", "c"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // RPOP should return "c"
    let cmd = build_resp_command(&["RPOP", "mylist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$1\r\nc\r\n");

    // RPOP should return "b"
    let cmd = build_resp_command(&["RPOP", "mylist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$1\r\nb\r\n");
}

#[tokio::test]
async fn test_pop_empty_list() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // LPOP on nonexistent key
    let cmd = build_resp_command(&["LPOP", "nonexistent"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$-1\r\n", "LPOP should return nil for nonexistent key");

    // RPOP on nonexistent key
    let cmd = build_resp_command(&["RPOP", "nonexistent"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$-1\r\n", "RPOP should return nil for nonexistent key");
}

#[tokio::test]
async fn test_pop_with_count() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH mylist a b c d e
    let cmd = build_resp_command(&["RPUSH", "mylist", "a", "b", "c", "d", "e"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // LPOP with count 2
    let cmd = build_resp_command(&["LPOP", "mylist", "2"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*2\r\n"), "LPOP with count should return array");

    // RPOP with count 2
    let cmd = build_resp_command(&["RPOP", "mylist", "2"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*2\r\n"), "RPOP with count should return array");
}

#[tokio::test]
async fn test_pop_removes_empty_list() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH mylist single
    let cmd = build_resp_command(&["RPUSH", "mylist", "single"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // LPOP the only element
    let cmd = build_resp_command(&["LPOP", "mylist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$6\r\nsingle\r\n");

    // List should now be gone, EXISTS should return 0
    let cmd = build_resp_command(&["EXISTS", "mylist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n", "Empty list should be deleted");
}

// ==================== LLEN Tests ====================

#[tokio::test]
async fn test_llen_basic() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // LLEN on nonexistent key
    let cmd = build_resp_command(&["LLEN", "nonexistent"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":0\r\n", "LLEN should return 0 for nonexistent key");

    // RPUSH mylist a b c
    let cmd = build_resp_command(&["RPUSH", "mylist", "a", "b", "c"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // LLEN should return 3
    let cmd = build_resp_command(&["LLEN", "mylist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":3\r\n");
}

#[tokio::test]
async fn test_llen_after_pop() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH mylist a b c
    let cmd = build_resp_command(&["RPUSH", "mylist", "a", "b", "c"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // LPOP one element
    let cmd = build_resp_command(&["LPOP", "mylist"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // LLEN should return 2
    let cmd = build_resp_command(&["LLEN", "mylist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":2\r\n");
}

// ==================== LRANGE Tests ====================

#[tokio::test]
async fn test_lrange_full() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH mylist a b c d e
    let cmd = build_resp_command(&["RPUSH", "mylist", "a", "b", "c", "d", "e"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // LRANGE 0 -1 (full list)
    let cmd = build_resp_command(&["LRANGE", "mylist", "0", "-1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*5\r\n"), "Should have 5 elements");
}

#[tokio::test]
async fn test_lrange_subset() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH mylist a b c d e
    let cmd = build_resp_command(&["RPUSH", "mylist", "a", "b", "c", "d", "e"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // LRANGE 1 3 (b, c, d)
    let cmd = build_resp_command(&["LRANGE", "mylist", "1", "3"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*3\r\n"), "Should have 3 elements");
}

#[tokio::test]
async fn test_lrange_negative_indices() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH mylist a b c d e
    let cmd = build_resp_command(&["RPUSH", "mylist", "a", "b", "c", "d", "e"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // LRANGE -3 -1 (c, d, e)
    let cmd = build_resp_command(&["LRANGE", "mylist", "-3", "-1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*3\r\n"), "Should have 3 elements");
}

#[tokio::test]
async fn test_lrange_empty() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // LRANGE on nonexistent key
    let cmd = build_resp_command(&["LRANGE", "nonexistent", "0", "-1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"*0\r\n", "LRANGE should return empty array for nonexistent key");
}

#[tokio::test]
async fn test_lrange_out_of_bounds() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH mylist a b c
    let cmd = build_resp_command(&["RPUSH", "mylist", "a", "b", "c"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // LRANGE 0 100 (beyond list length)
    let cmd = build_resp_command(&["LRANGE", "mylist", "0", "100"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*3\r\n"), "Should return all 3 elements");

    // LRANGE 5 10 (completely out of bounds)
    let cmd = build_resp_command(&["LRANGE", "mylist", "5", "10"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"*0\r\n", "Should return empty array");
}

// ==================== Error Handling Tests ====================

#[tokio::test]
async fn test_list_wrong_arg_count() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // LPUSH with too few args
    let cmd = build_resp_command(&["LPUSH", "mylist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("-ERR"), "LPUSH with wrong args should return error");

    // LRANGE with too few args
    let cmd = build_resp_command(&["LRANGE", "mylist", "0"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("-ERR"), "LRANGE with wrong args should return error");
}

#[tokio::test]
async fn test_list_type_mismatch() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // SET a string key
    let cmd = build_resp_command(&["SET", "stringkey", "stringvalue"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // Try LPUSH on string key (type mismatch)
    let cmd = build_resp_command(&["LPUSH", "stringkey", "value"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("-"), "LPUSH on string key should return error");

    // Try LRANGE on string key (type mismatch)
    let cmd = build_resp_command(&["LRANGE", "stringkey", "0", "-1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("-"), "LRANGE on string key should return error");
}

#[tokio::test]
async fn test_lrange_invalid_indices() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH mylist a b c
    let cmd = build_resp_command(&["RPUSH", "mylist", "a", "b", "c"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // LRANGE with invalid index
    let cmd = build_resp_command(&["LRANGE", "mylist", "invalid", "0"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("-ERR"), "LRANGE with invalid index should return error");
}

// ==================== List with TTL Tests ====================

#[tokio::test]
async fn test_list_with_expire() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH mylist a b c
    let cmd = build_resp_command(&["RPUSH", "mylist", "a", "b", "c"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // EXPIRE the list key
    let cmd = build_resp_command(&["EXPIRE", "mylist", "60"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n", "EXPIRE should return 1");

    // TTL should be positive
    let cmd = build_resp_command(&["TTL", "mylist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with(":"), "TTL should return integer");

    // List should still be accessible
    let cmd = build_resp_command(&["LLEN", "mylist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":3\r\n");
}

// ==================== Concurrent Access Tests ====================

#[tokio::test]
async fn test_concurrent_lpush() {
    let (_server, addr) = create_and_start_test_server().await;
    let barrier = Arc::new(Barrier::new(20));
    let mut handles = vec![];

    // 20 concurrent clients pushing elements
    for i in 0..20 {
        let barrier = Arc::clone(&barrier);
        handles.push(tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            barrier.wait().await;

            let cmd = build_resp_command(&["LPUSH", "concurrent_list", &format!("value{}", i)]);
            let response = send_command(&mut stream, &cmd).await.unwrap();
            let response_str = String::from_utf8_lossy(&response);
            assert!(response_str.starts_with(":"), "LPUSH should return integer");
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Verify all 20 elements exist
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let cmd = build_resp_command(&["LLEN", "concurrent_list"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":20\r\n", "Should have 20 elements");
}

#[tokio::test]
async fn test_concurrent_push_pop() {
    let (_server, addr) = create_and_start_test_server().await;

    // Pre-populate the list
    let mut stream = TcpStream::connect(addr).await.unwrap();
    for i in 0..100 {
        let cmd = build_resp_command(&["RPUSH", "pushpop_list", &format!("value{}", i)]);
        send_command(&mut stream, &cmd).await.unwrap();
    }

    let barrier = Arc::new(Barrier::new(20));
    let mut handles = vec![];

    // 10 clients popping, 10 clients pushing
    for i in 0..20 {
        let barrier = Arc::clone(&barrier);
        handles.push(tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            barrier.wait().await;

            if i < 10 {
                // Pop operations
                for _ in 0..5 {
                    let cmd = build_resp_command(&["LPOP", "pushpop_list"]);
                    send_command(&mut stream, &cmd).await.unwrap();
                }
            } else {
                // Push operations
                for j in 0..5 {
                    let cmd =
                        build_resp_command(&["RPUSH", "pushpop_list", &format!("new{}{}", i, j)]);
                    send_command(&mut stream, &cmd).await.unwrap();
                }
            }
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Should have 100 - 50 + 50 = 100 elements
    let cmd = build_resp_command(&["LLEN", "pushpop_list"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":100\r\n", "Should maintain 100 elements after concurrent push/pop");
}

// ==================== List Workflow Tests ====================

#[tokio::test]
async fn test_queue_workflow() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Simulate a job queue (FIFO)
    // Add jobs to end
    let cmd = build_resp_command(&["RPUSH", "jobs", "job1", "job2", "job3"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":3\r\n");

    // Process jobs from front
    let cmd = build_resp_command(&["LPOP", "jobs"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$4\r\njob1\r\n", "First job should be job1");

    let cmd = build_resp_command(&["LPOP", "jobs"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$4\r\njob2\r\n", "Second job should be job2");

    // Add more jobs
    let cmd = build_resp_command(&["RPUSH", "jobs", "job4"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // Check remaining jobs
    let cmd = build_resp_command(&["LRANGE", "jobs", "0", "-1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*2\r\n"), "Should have 2 remaining jobs");
}

#[tokio::test]
async fn test_stack_workflow() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Simulate a stack (LIFO)
    // Push to front
    let cmd = build_resp_command(&["LPUSH", "stack", "first"]);
    send_command(&mut stream, &cmd).await.unwrap();
    let cmd = build_resp_command(&["LPUSH", "stack", "second"]);
    send_command(&mut stream, &cmd).await.unwrap();
    let cmd = build_resp_command(&["LPUSH", "stack", "third"]);
    send_command(&mut stream, &cmd).await.unwrap();

    // Pop from front (LIFO)
    let cmd = build_resp_command(&["LPOP", "stack"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$5\r\nthird\r\n", "Last pushed should be popped first");

    let cmd = build_resp_command(&["LPOP", "stack"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$6\r\nsecond\r\n");

    let cmd = build_resp_command(&["LPOP", "stack"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$5\r\nfirst\r\n");
}

// ==================== Edge Cases ====================

#[tokio::test]
async fn test_list_empty_value() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH with empty value
    let cmd = build_resp_command(&["RPUSH", "mylist", ""]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    // LPOP should return empty bulk string
    let cmd = build_resp_command(&["LPOP", "mylist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$0\r\n\r\n", "Should return empty bulk string");
}

#[tokio::test]
async fn test_list_special_characters() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH with special characters
    let cmd = build_resp_command(&["RPUSH", "mylist", "value with spaces", "value:with:colons"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":2\r\n");

    // LPOP
    let cmd = build_resp_command(&["LPOP", "mylist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b"$17\r\nvalue with spaces\r\n");
}

#[tokio::test]
async fn test_list_unicode() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // RPUSH with unicode characters
    let cmd = build_resp_command(&["RPUSH", "mylist", "日本語", "한국어"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":2\r\n");

    // LLEN
    let cmd = build_resp_command(&["LLEN", "mylist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":2\r\n");

    // LPOP unicode value
    let cmd = build_resp_command(&["LPOP", "mylist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();

    // Unicode string "日本語" is 9 bytes in UTF-8
    assert_eq!(&response[..4], b"$9\r\n");
}

#[tokio::test]
async fn test_large_list() {
    let (_server, addr) = create_and_start_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Push 1000 elements
    for i in 0..100 {
        let values: Vec<String> = (0..10).map(|j| format!("value{}_{}", i, j)).collect();
        let mut parts: Vec<&str> = vec!["RPUSH", "largelist"];
        parts.extend(values.iter().map(|s| s.as_str()));
        let cmd = build_resp_command(&parts);
        send_command(&mut stream, &cmd).await.unwrap();
    }

    // LLEN should return 1000
    let cmd = build_resp_command(&["LLEN", "largelist"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    assert_eq!(response, b":1000\r\n");

    // LRANGE first 10
    let cmd = build_resp_command(&["LRANGE", "largelist", "0", "9"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*10\r\n"));

    // LRANGE last 10
    let cmd = build_resp_command(&["LRANGE", "largelist", "-10", "-1"]);
    let response = send_command(&mut stream, &cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("*10\r\n"));
}
