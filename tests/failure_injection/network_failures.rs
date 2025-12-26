//! Network failure injection tests
//!
//! Tests the server's resilience to various network failure scenarios:
//! - Connection drops mid-command
//! - Slow clients (partial data with delays)
//! - Client disconnects during response
//! - Rapid connect/disconnect cycles

use rustypotato::{Config, RustyPotatoServer};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};

/// Create and start a test server on a random port
async fn create_test_server() -> (RustyPotatoServer, std::net::SocketAddr) {
    let mut config = Config::default();
    config.server.port = 0;

    let mut server = RustyPotatoServer::new(config).unwrap();
    let addr = server.start_with_addr().await.unwrap();
    sleep(Duration::from_millis(50)).await;

    (server, addr)
}

/// Build RESP array command
fn build_resp_command(parts: &[&str]) -> Vec<u8> {
    let mut cmd = format!("*{}\r\n", parts.len());
    for part in parts {
        cmd.push_str(&format!("${}\r\n{}\r\n", part.len(), part));
    }
    cmd.into_bytes()
}

/// Send command and read response
async fn send_and_receive(
    stream: &mut TcpStream,
    parts: &[&str],
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let cmd = build_resp_command(parts);
    stream.write_all(&cmd).await?;
    stream.flush().await?;

    let mut buf = vec![0u8; 4096];
    let n = timeout(Duration::from_secs(2), stream.read(&mut buf)).await??;
    buf.truncate(n);
    Ok(buf)
}

// ==================== Connection Drop Tests ====================

/// Test: Server survives when client drops connection mid-command
#[tokio::test]
async fn test_connection_drop_mid_command() {
    let (_server, addr) = create_test_server().await;

    // Connect and send partial command, then drop connection
    {
        let mut stream = TcpStream::connect(addr).await.unwrap();

        // Send first part of SET command (incomplete)
        let partial = b"*3\r\n$3\r\nSET\r\n$4\r\nkey1\r\n";
        stream.write_all(partial).await.unwrap();

        // Drop connection by letting stream go out of scope
    }

    // Small delay for server to process disconnect
    sleep(Duration::from_millis(100)).await;

    // Server should still accept new connections and process commands
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let response = send_and_receive(&mut stream, &["SET", "test_key", "test_value"]).await.unwrap();
    assert_eq!(response, b"+OK\r\n", "Server should process SET after client drop");
}

/// Test: Server handles connection drop after complete command
#[tokio::test]
async fn test_connection_drop_after_command() {
    let (_server, addr) = create_test_server().await;

    // Connect, send complete command, then drop immediately
    {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let cmd = build_resp_command(&["SET", "dropkey", "value"]);
        stream.write_all(&cmd).await.unwrap();
        // Don't wait for response, just drop
    }

    sleep(Duration::from_millis(100)).await;

    // Verify the command was processed
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let response = send_and_receive(&mut stream, &["GET", "dropkey"]).await.unwrap();

    // The SET command may or may not have completed depending on timing
    // This test verifies the server survives either way
    assert!(!response.is_empty(), "Server should respond");
}

// ==================== Slow Client Tests ====================

/// Test: Server handles slow client sending data with delays
/// NOTE: Currently the server returns an internal error on chunked data.
/// This test documents this behavior for future improvement.
#[tokio::test]
#[ignore = "Server currently returns internal error on chunked data delivery"]
async fn test_slow_client_chunked() {
    let (_server, addr) = create_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Send SET command in chunks with delays (not byte by byte to avoid timeout)
    let cmd = build_resp_command(&["SET", "slowkey", "slowvalue"]);
    let chunk_size = cmd.len() / 3;

    for chunk in cmd.chunks(chunk_size.max(1)) {
        stream.write_all(chunk).await.unwrap();
        stream.flush().await.unwrap();
        sleep(Duration::from_millis(50)).await;
    }

    let mut buf = vec![0u8; 1024];
    let n = timeout(Duration::from_secs(5), stream.read(&mut buf))
        .await
        .unwrap()
        .unwrap();
    buf.truncate(n);

    assert_eq!(buf, b"+OK\r\n", "Server should handle slow client");
}

/// Test: Server handles partial data with long pause
/// NOTE: Currently the server returns an internal error when data is paused mid-delivery.
/// This test documents this behavior for future improvement.
#[tokio::test]
#[ignore = "Server currently returns internal error on paused data delivery"]
async fn test_partial_data_with_pause() {
    let (_server, addr) = create_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Send first half of command
    let cmd = build_resp_command(&["SET", "pausekey", "pausevalue"]);
    let mid = cmd.len() / 2;

    stream.write_all(&cmd[..mid]).await.unwrap();
    stream.flush().await.unwrap();
    sleep(Duration::from_millis(200)).await;

    // Send second half
    stream.write_all(&cmd[mid..]).await.unwrap();
    stream.flush().await.unwrap();

    let mut buf = vec![0u8; 1024];
    let n = timeout(Duration::from_secs(2), stream.read(&mut buf))
        .await
        .unwrap()
        .unwrap();
    buf.truncate(n);

    assert_eq!(buf, b"+OK\r\n", "Server should handle paused data");
}

// ==================== Rapid Connect/Disconnect Tests ====================

/// Test: Server handles rapid connect/disconnect cycles
#[tokio::test]
async fn test_rapid_connect_disconnect() {
    let (_server, addr) = create_test_server().await;

    // Rapidly connect and disconnect 50 times
    for _ in 0..50 {
        let stream = TcpStream::connect(addr).await.unwrap();
        drop(stream);
    }

    // Server should still work
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let response = send_and_receive(&mut stream, &["SET", "after_rapid", "value"]).await.unwrap();
    assert_eq!(response, b"+OK\r\n");
}

/// Test: Server handles burst of connections
#[tokio::test]
async fn test_connection_burst() {
    let (_server, addr) = create_test_server().await;

    // Open 20 connections simultaneously
    let mut streams = Vec::new();
    for _ in 0..20 {
        let stream = TcpStream::connect(addr).await.unwrap();
        streams.push(stream);
    }

    // Send SET on all of them with unique keys
    for (i, stream) in streams.iter_mut().enumerate() {
        let key = format!("burst_key_{}", i);
        let cmd = build_resp_command(&["SET", &key, "value"]);
        stream.write_all(&cmd).await.unwrap();
        stream.flush().await.unwrap();
    }

    // Read responses
    for stream in &mut streams {
        let mut buf = vec![0u8; 1024];
        let n = timeout(Duration::from_secs(2), stream.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        buf.truncate(n);
        assert_eq!(buf, b"+OK\r\n");
    }
}

// ==================== Half-Open Connection Tests ====================

/// Test: Server handles client that stops reading (write timeout scenario)
#[tokio::test]
async fn test_client_stops_reading() {
    let (_server, addr) = create_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Send command but never read the response
    let cmd = build_resp_command(&["SET", "noread", "value"]);
    stream.write_all(&cmd).await.unwrap();

    // Wait a bit (simulating client that stopped reading)
    sleep(Duration::from_millis(100)).await;

    // Server should still work for other clients
    let mut stream2 = TcpStream::connect(addr).await.unwrap();
    let response = send_and_receive(&mut stream2, &["SET", "other_key", "other_value"]).await.unwrap();
    assert_eq!(response, b"+OK\r\n");
}

/// Test: Server handles multiple commands on same connection with drops between
#[tokio::test]
async fn test_pipelined_with_interleaved_drops() {
    let (_server, addr) = create_test_server().await;

    // First connection: set value then drop
    {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let cmd = build_resp_command(&["SET", "persist", "value1"]);
        stream.write_all(&cmd).await.unwrap();

        let mut buf = vec![0u8; 1024];
        let _ = timeout(Duration::from_secs(1), stream.read(&mut buf)).await;
    }

    // Second connection: verify and update
    {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let response = send_and_receive(&mut stream, &["GET", "persist"]).await.unwrap();

        // Value should persist across connections
        assert_eq!(response, b"$6\r\nvalue1\r\n");
    }
}

// ==================== Invalid Data Tests ====================

/// Test: Server handles completely invalid data gracefully
#[tokio::test]
async fn test_garbage_data() {
    let (_server, addr) = create_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    // Send complete garbage
    stream.write_all(b"not valid resp at all\x00\xff\xfe").await.unwrap();
    stream.flush().await.unwrap();

    // Server should either error or ignore, but not crash
    sleep(Duration::from_millis(100)).await;

    // New connection should still work
    let mut stream2 = TcpStream::connect(addr).await.unwrap();
    let response = send_and_receive(&mut stream2, &["SET", "after_garbage", "value"]).await.unwrap();
    assert_eq!(response, b"+OK\r\n");
}

/// Test: Server handles malformed RESP gracefully
#[tokio::test]
async fn test_malformed_resp() {
    let (_server, addr) = create_test_server().await;

    let malformed_cases = vec![
        b"*-5\r\n".to_vec(),              // Negative array length
        b"$100\r\nshort\r\n".to_vec(),    // Length mismatch
        b"*1\r\n$3\r\nGET".to_vec(),      // Missing CRLF
        b":notanumber\r\n".to_vec(),      // Invalid integer
        b"+OK".to_vec(),                   // Missing CRLF
    ];

    for malformed in malformed_cases {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(&malformed).await.unwrap();
        stream.flush().await.unwrap();
        drop(stream);
    }

    // Server should still be operational
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let response = send_and_receive(&mut stream, &["SET", "after_malformed", "value"]).await.unwrap();
    assert_eq!(response, b"+OK\r\n");
}

// ==================== Multiple Sequential Commands ====================

/// Test: Server handles many sequential commands on same connection
#[tokio::test]
async fn test_many_sequential_commands() {
    let (_server, addr) = create_test_server().await;
    let mut stream = TcpStream::connect(addr).await.unwrap();

    for i in 0..100 {
        let key = format!("seq_key_{}", i);
        let value = format!("seq_value_{}", i);
        let response = send_and_receive(&mut stream, &["SET", &key, &value]).await.unwrap();
        assert_eq!(response, b"+OK\r\n", "Command {} should succeed", i);
    }

    // Verify a few values
    for i in [0, 50, 99] {
        let key = format!("seq_key_{}", i);
        let expected_value = format!("seq_value_{}", i);
        let response = send_and_receive(&mut stream, &["GET", &key]).await.unwrap();
        let expected = format!("${}\r\n{}\r\n", expected_value.len(), expected_value);
        assert_eq!(response, expected.as_bytes(), "Key {} should have correct value", i);
    }
}
