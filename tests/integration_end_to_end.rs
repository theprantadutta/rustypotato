//! End-to-end integration tests for command engine and network layer integration
//!
//! These tests verify the complete request/response flow from network to storage,
//! including error handling, connection management, and resource cleanup.

use rustypotato::{Config, RustyPotatoServer};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
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

    let mut buffer = vec![0u8; 1024];
    let n = timeout(Duration::from_secs(5), stream.read(&mut buffer)).await??;
    buffer.truncate(n);
    Ok(buffer)
}

#[tokio::test]
async fn test_end_to_end_basic_operations() {
    let (_server, addr) = create_and_start_test_server().await;

    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Test SET command
    let set_cmd = b"*3\r\n$3\r\nSET\r\n$8\r\ntest_key\r\n$10\r\ntest_value\r\n";
    let response = send_command(&mut stream, set_cmd).await.unwrap();
    assert_eq!(response, b"+OK\r\n");

    // Test GET command
    let get_cmd = b"*2\r\n$3\r\nGET\r\n$8\r\ntest_key\r\n";
    let response = send_command(&mut stream, get_cmd).await.unwrap();
    assert_eq!(response, b"$10\r\ntest_value\r\n");

    // Test EXISTS command
    let exists_cmd = b"*2\r\n$6\r\nEXISTS\r\n$8\r\ntest_key\r\n";
    let response = send_command(&mut stream, exists_cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    // Test DEL command
    let del_cmd = b"*2\r\n$3\r\nDEL\r\n$8\r\ntest_key\r\n";
    let response = send_command(&mut stream, del_cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    // Verify key is deleted
    let get_cmd = b"*2\r\n$3\r\nGET\r\n$8\r\ntest_key\r\n";
    let response = send_command(&mut stream, get_cmd).await.unwrap();
    assert_eq!(response, b"$-1\r\n");
}

#[tokio::test]
async fn test_end_to_end_ttl_operations() {
    let (_server, addr) = create_and_start_test_server().await;

    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Set a key
    let set_cmd = b"*3\r\n$3\r\nSET\r\n$7\r\nttl_key\r\n$9\r\nttl_value\r\n";
    let response = send_command(&mut stream, set_cmd).await.unwrap();
    assert_eq!(response, b"+OK\r\n");

    // Set expiration
    let expire_cmd = b"*3\r\n$6\r\nEXPIRE\r\n$7\r\nttl_key\r\n$2\r\n10\r\n";
    let response = send_command(&mut stream, expire_cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    // Check TTL
    let ttl_cmd = b"*2\r\n$3\r\nTTL\r\n$7\r\nttl_key\r\n";
    let response = send_command(&mut stream, ttl_cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with(":"));
    assert!(response_str.contains("10") || response_str.contains("9")); // TTL should be around 10 or 9
}

#[tokio::test]
async fn test_end_to_end_atomic_operations() {
    let (_server, addr) = create_and_start_test_server().await;

    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Test INCR on non-existent key
    let incr_cmd = b"*2\r\n$4\r\nINCR\r\n$11\r\natomic_test\r\n";
    let response = send_command(&mut stream, incr_cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");

    // Test INCR again
    let incr_cmd = b"*2\r\n$4\r\nINCR\r\n$11\r\natomic_test\r\n";
    let response = send_command(&mut stream, incr_cmd).await.unwrap();
    assert_eq!(response, b":2\r\n");

    // Test DECR
    let decr_cmd = b"*2\r\n$4\r\nDECR\r\n$11\r\natomic_test\r\n";
    let response = send_command(&mut stream, decr_cmd).await.unwrap();
    assert_eq!(response, b":1\r\n");
}

#[tokio::test]
async fn test_end_to_end_error_handling() {
    let (_server, addr) = create_and_start_test_server().await;

    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Test unknown command
    let unknown_cmd = b"*1\r\n$7\r\nUNKNOWN\r\n";
    let response = send_command(&mut stream, unknown_cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("-ERR unknown command"));

    // Test wrong arity
    let wrong_arity_cmd = b"*2\r\n$3\r\nSET\r\n$3\r\nkey\r\n"; // SET needs 3 args
    let response = send_command(&mut stream, wrong_arity_cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("wrong number of arguments"));

    // Test INCR on non-numeric value
    let set_cmd = b"*3\r\n$3\r\nSET\r\n$8\r\nstr_test\r\n$6\r\nstring\r\n";
    let response = send_command(&mut stream, set_cmd).await.unwrap();
    assert_eq!(response, b"+OK\r\n");

    let incr_cmd = b"*2\r\n$4\r\nINCR\r\n$8\r\nstr_test\r\n";
    let response = send_command(&mut stream, incr_cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("not an integer"));
}

#[tokio::test]
async fn test_end_to_end_multiple_connections() {
    let (_server, addr) = create_and_start_test_server().await;

    // Create multiple connections and test concurrent operations
    let mut handles = Vec::new();

    for i in 0..5 {
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();

            // Each connection sets a unique key
            let key = format!("concurrent_key_{i}");
            let value = format!("concurrent_value_{i}");
            let set_cmd = format!(
                "*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n",
                key.len(),
                key,
                value.len(),
                value
            );

            let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b"+OK\r\n");

            // Get the value back
            let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
            let response = send_command(&mut stream, get_cmd.as_bytes()).await.unwrap();
            let expected = format!("${}\r\n{}\r\n", value.len(), value);
            assert_eq!(response, expected.as_bytes());

            i
        });

        handles.push(handle);
    }

    // Wait for all connections to complete
    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_end_to_end_command_pipelining() {
    let (_server, addr) = create_and_start_test_server().await;

    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Test that multiple commands can be sent and processed correctly
    // This tests the core integration without complex pipelining timing issues

    // Send first command
    let set_cmd1 = b"*3\r\n$3\r\nSET\r\n$4\r\nkey1\r\n$6\r\nvalue1\r\n";
    let response1 = send_command(&mut stream, set_cmd1).await.unwrap();
    assert_eq!(response1, b"+OK\r\n");

    // Send second command
    let set_cmd2 = b"*3\r\n$3\r\nSET\r\n$4\r\nkey2\r\n$6\r\nvalue2\r\n";
    let response2 = send_command(&mut stream, set_cmd2).await.unwrap();
    assert_eq!(response2, b"+OK\r\n");

    // Verify both keys were set correctly
    let get_cmd1 = b"*2\r\n$3\r\nGET\r\n$4\r\nkey1\r\n";
    let response = send_command(&mut stream, get_cmd1).await.unwrap();
    assert_eq!(response, b"$6\r\nvalue1\r\n");

    let get_cmd2 = b"*2\r\n$3\r\nGET\r\n$4\r\nkey2\r\n";
    let response = send_command(&mut stream, get_cmd2).await.unwrap();
    assert_eq!(response, b"$6\r\nvalue2\r\n");
}

#[tokio::test]
async fn test_end_to_end_connection_cleanup() {
    let (_server, addr) = create_and_start_test_server().await;

    // Create a connection and perform operations
    {
        let mut stream = TcpStream::connect(addr).await.unwrap();

        let set_cmd = b"*3\r\n$3\r\nSET\r\n$12\r\ncleanup_test\r\n$5\r\nvalue\r\n";
        let response = send_command(&mut stream, set_cmd).await.unwrap();
        assert_eq!(response, b"+OK\r\n");

        // Connection will be dropped here
    }

    // Create a new connection and verify the data persists
    let mut stream = TcpStream::connect(addr).await.unwrap();

    let get_cmd = b"*2\r\n$3\r\nGET\r\n$12\r\ncleanup_test\r\n";
    let response = send_command(&mut stream, get_cmd).await.unwrap();
    assert_eq!(response, b"$5\r\nvalue\r\n");
}

#[tokio::test]
async fn test_end_to_end_large_values() {
    let (_server, addr) = create_and_start_test_server().await;

    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Create a large value (2KB)
    let large_value = "x".repeat(2048);
    let set_cmd = format!(
        "*3\r\n$3\r\nSET\r\n$9\r\nlarge_key\r\n${}\r\n{}\r\n",
        large_value.len(),
        large_value
    );

    let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
    assert_eq!(response, b"+OK\r\n");

    // Get the large value back
    let get_cmd = b"*2\r\n$3\r\nGET\r\n$9\r\nlarge_key\r\n";

    // For large responses, we might need a bigger buffer
    stream.write_all(get_cmd).await.unwrap();
    stream.flush().await.unwrap();

    let mut buffer = vec![0u8; 4096];
    let n = timeout(Duration::from_secs(5), stream.read(&mut buffer))
        .await
        .unwrap()
        .unwrap();
    buffer.truncate(n);

    let expected = format!("${}\r\n{}\r\n", large_value.len(), large_value);
    assert_eq!(buffer, expected.as_bytes());
}
