//! Integration tests for TCP server functionality
//! 
//! These tests verify the complete TCP server implementation including
//! connection handling, command processing, and graceful shutdown.

use rustypotato::{
    commands::{CommandRegistry, SetCommand, GetCommand, DelCommand, ExistsCommand},
    config::Config,
    network::TcpServer,
    storage::MemoryStore,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Helper function to create a test server with basic commands and start it
async fn create_and_start_test_server() -> (TcpServer, std::net::SocketAddr) {
    let mut config = Config::default();
    config.server.port = 0; // Use random port for testing
    let config = Arc::new(config);
    
    let storage = Arc::new(MemoryStore::new());
    let mut command_registry = CommandRegistry::new();
    
    // Register basic commands
    command_registry.register(Box::new(SetCommand));
    command_registry.register(Box::new(GetCommand));
    command_registry.register(Box::new(DelCommand));
    command_registry.register(Box::new(ExistsCommand));
    
    let command_registry = Arc::new(command_registry);
    let mut server = TcpServer::new(config.clone(), storage, command_registry);
    
    // Start the server and get the listening address
    let addr = server.start_with_addr().await.unwrap();
    
    // Give server time to start accepting connections
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    (server, addr)
}

/// Helper function to create a test server with basic commands (without starting)
async fn create_test_server() -> (TcpServer, Arc<Config>) {
    let mut config = Config::default();
    config.server.port = 0; // Use random port for testing
    let config = Arc::new(config);
    
    let storage = Arc::new(MemoryStore::new());
    let mut command_registry = CommandRegistry::new();
    
    // Register basic commands
    command_registry.register(Box::new(SetCommand));
    command_registry.register(Box::new(GetCommand));
    command_registry.register(Box::new(DelCommand));
    command_registry.register(Box::new(ExistsCommand));
    
    let command_registry = Arc::new(command_registry);
    let server = TcpServer::new(config.clone(), storage, command_registry);
    
    (server, config)
}

/// Helper function to send RESP command and read response
async fn send_command(stream: &mut TcpStream, command: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    stream.write_all(command).await?;
    stream.flush().await?;
    
    let mut buffer = vec![0u8; 1024];
    let n = timeout(Duration::from_secs(5), stream.read(&mut buffer)).await??;
    buffer.truncate(n);
    Ok(buffer)
}

#[tokio::test]
async fn test_server_basic_functionality() {
    let (server, addr) = create_and_start_test_server().await;
    
    // Connect to server
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    // Test SET command
    let set_cmd = b"*3\r\n$3\r\nSET\r\n$4\r\ntest\r\n$5\r\nvalue\r\n";
    let response = send_command(&mut stream, set_cmd).await.unwrap();
    assert_eq!(response, b"+OK\r\n");
    
    // Test GET command
    let get_cmd = b"*2\r\n$3\r\nGET\r\n$4\r\ntest\r\n";
    let response = send_command(&mut stream, get_cmd).await.unwrap();
    assert_eq!(response, b"$5\r\nvalue\r\n");
    
    // Cleanup
    drop(stream);
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_server_multiple_connections() {
    let (server, addr) = create_and_start_test_server().await;
    
    // Create multiple connections
    let mut connections = Vec::new();
    for i in 0..5 {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Each connection sets a different key
        let key = format!("key{}", i);
        let value = format!("value{}", i);
        let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                             key.len(), key, value.len(), value);
        
        let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        connections.push(stream);
    }
    
    // Verify all keys were set by reading them back
    for (i, stream) in connections.iter_mut().enumerate() {
        let key = format!("key{}", i);
        let value = format!("value{}", i);
        let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
        
        let response = send_command(stream, get_cmd.as_bytes()).await.unwrap();
        let expected = format!("${}\r\n{}\r\n", value.len(), value);
        assert_eq!(response, expected.as_bytes());
    }
    
    // Cleanup
    drop(connections);
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_server_concurrent_operations() {
    let (server, addr) = create_and_start_test_server().await;
    
    // Spawn multiple concurrent tasks
    let mut handles = Vec::new();
    
    for i in 0..10 {
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            
            // Set a key
            let key = format!("concurrent_key_{}", i);
            let value = format!("concurrent_value_{}", i);
            let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                                 key.len(), key, value.len(), value);
            
            let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b"+OK\r\n");
            
            // Get the key back
            let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
            let response = send_command(&mut stream, get_cmd.as_bytes()).await.unwrap();
            let expected = format!("${}\r\n{}\r\n", value.len(), value);
            assert_eq!(response, expected.as_bytes());
            
            i
        });
        
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Cleanup
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_server_error_handling() {
    let (server, addr) = create_and_start_test_server().await;
    
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    // Test unknown command
    let unknown_cmd = b"*1\r\n$7\r\nUNKNOWN\r\n";
    let response = send_command(&mut stream, unknown_cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("-ERR unknown command"));
    
    // Test invalid protocol
    let invalid_cmd = b"invalid protocol data\r\n";
    let response = send_command(&mut stream, invalid_cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("-ERR"));
    
    // Test wrong arity
    let wrong_arity_cmd = b"*2\r\n$3\r\nSET\r\n$3\r\nkey\r\n"; // SET needs 3 args
    let response = send_command(&mut stream, wrong_arity_cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("wrong number of arguments"));
    
    // Cleanup
    drop(stream);
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_server_connection_persistence() {
    let (server, addr) = create_and_start_test_server().await;
    
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    // Send multiple commands on the same connection
    for i in 0..5 {
        let key = format!("persistent_key_{}", i);
        let value = format!("persistent_value_{}", i);
        let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                             key.len(), key, value.len(), value);
        
        let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
    }
    
    // Verify all keys exist
    for i in 0..5 {
        let key = format!("persistent_key_{}", i);
        let exists_cmd = format!("*2\r\n$6\r\nEXISTS\r\n${}\r\n{}\r\n", key.len(), key);
        
        let response = send_command(&mut stream, exists_cmd.as_bytes()).await.unwrap();
        assert_eq!(response, b":1\r\n");
    }
    
    // Cleanup
    drop(stream);
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_server_partial_commands() {
    let (server, addr) = create_and_start_test_server().await;
    
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    // Send command in parts to test buffering
    let cmd_part1 = b"*3\r\n$3\r\nSET\r\n";
    let cmd_part2 = b"$4\r\ntest\r\n$5\r\nvalue\r\n";
    
    stream.write_all(cmd_part1).await.unwrap();
    stream.flush().await.unwrap();
    
    // Wait a bit
    tokio::time::sleep(Duration::from_millis(10)).await;
    
    stream.write_all(cmd_part2).await.unwrap();
    stream.flush().await.unwrap();
    
    // Read response
    let mut buffer = vec![0u8; 1024];
    let n = timeout(Duration::from_secs(5), stream.read(&mut buffer)).await.unwrap().unwrap();
    buffer.truncate(n);
    
    assert_eq!(buffer, b"+OK\r\n");
    
    // Cleanup
    drop(stream);
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_server_stats() {
    let (server, _config) = create_test_server().await;
    
    let stats = server.stats().await;
    assert_eq!(stats.active_connections, 0);
    assert_eq!(stats.max_connections, 10000);
    assert_eq!(stats.total_connections_accepted, 0);
    assert_eq!(stats.bind_address, "127.0.0.1:0");
}

#[tokio::test]
async fn test_server_bind_address() {
    let (server, _config) = create_test_server().await;
    assert_eq!(server.bind_address(), "127.0.0.1:0");
    assert!(!server.is_running());
}

#[tokio::test]
async fn test_server_graceful_shutdown() {
    let (server, addr) = create_and_start_test_server().await;
    
    // Connect a client
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    // Send a command
    let set_cmd = b"*3\r\n$3\r\nSET\r\n$4\r\ntest\r\n$5\r\nvalue\r\n";
    let response = send_command(&mut stream, set_cmd).await.unwrap();
    assert_eq!(response, b"+OK\r\n");
    
    // Initiate graceful shutdown
    server.shutdown().await.unwrap();
    
    // Give some time for shutdown to propagate
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Verify connection is closed
    let mut buffer = [0u8; 1024];
    let result = timeout(Duration::from_secs(1), stream.read(&mut buffer)).await;
    
    // Should either timeout or read 0 bytes (connection closed)
    match result {
        Ok(Ok(0)) => {}, // Connection closed
        Err(_) => {},    // Timeout
        _ => panic!("Expected connection to be closed or timeout"),
    }
}

#[tokio::test]
async fn test_server_command_pipelining() {
    let (server, addr) = create_and_start_test_server().await;
    
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    // Send multiple commands without waiting for responses (pipelining)
    let commands = vec![
        b"*3\r\n$3\r\nSET\r\n$4\r\nkey1\r\n$6\r\nvalue1\r\n".to_vec(),
        b"*3\r\n$3\r\nSET\r\n$4\r\nkey2\r\n$6\r\nvalue2\r\n".to_vec(),
        b"*2\r\n$3\r\nGET\r\n$4\r\nkey1\r\n".to_vec(),
        b"*2\r\n$3\r\nGET\r\n$4\r\nkey2\r\n".to_vec(),
    ];
    
    // Send all commands at once
    for cmd in &commands {
        stream.write_all(cmd).await.unwrap();
    }
    stream.flush().await.unwrap();
    
    // Read all responses
    let expected_responses = vec![
        b"+OK\r\n".to_vec(),
        b"+OK\r\n".to_vec(),
        b"$6\r\nvalue1\r\n".to_vec(),
        b"$6\r\nvalue2\r\n".to_vec(),
    ];
    
    for expected in expected_responses {
        let mut buffer = vec![0u8; 1024];
        let n = timeout(Duration::from_secs(5), stream.read(&mut buffer)).await.unwrap().unwrap();
        buffer.truncate(n);
        assert_eq!(buffer, expected);
    }
    
    // Cleanup
    drop(stream);
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_server_large_values() {
    let (server, addr) = create_and_start_test_server().await;
    
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    // Create a large value (1KB)
    let large_value = "x".repeat(1024);
    let set_cmd = format!("*3\r\n$3\r\nSET\r\n$9\r\nlarge_key\r\n${}\r\n{}\r\n", 
                         large_value.len(), large_value);
    
    let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
    assert_eq!(response, b"+OK\r\n");
    
    // Get the large value back
    let get_cmd = b"*2\r\n$3\r\nGET\r\n$9\r\nlarge_key\r\n";
    let response = send_command(&mut stream, get_cmd).await.unwrap();
    
    let expected = format!("${}\r\n{}\r\n", large_value.len(), large_value);
    assert_eq!(response, expected.as_bytes());
    
    // Cleanup
    drop(stream);
    server.shutdown().await.unwrap();
}