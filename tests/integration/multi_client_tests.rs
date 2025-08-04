//! Integration tests for multi-client scenarios
//!
//! These tests verify that RustyPotato can handle multiple concurrent clients
//! correctly, including data consistency, connection management, and resource cleanup.

use rustypotato::{Config, RustyPotatoServer};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};
use tokio::sync::Barrier;

/// Helper function to create and start a test server
async fn create_and_start_test_server() -> (RustyPotatoServer, std::net::SocketAddr) {
    let mut config = Config::default();
    config.server.port = 0; // Use random port for testing
    config.server.max_connections = 1000; // Allow many connections for testing
    config.storage.aof_enabled = false; // Disable persistence for faster tests
    
    let mut server = RustyPotatoServer::new(config).unwrap();
    let addr = server.start_with_addr().await.unwrap();
    
    // Give server time to start accepting connections
    sleep(Duration::from_millis(100)).await;
    
    (server, addr)
}

/// Helper function to send RESP command and read response
async fn send_command(stream: &mut TcpStream, command: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    stream.write_all(command).await?;
    stream.flush().await?;
    
    let mut buffer = vec![0u8; 4096];
    let n = timeout(Duration::from_secs(10), stream.read(&mut buffer)).await??;
    buffer.truncate(n);
    Ok(buffer)
}

#[tokio::test]
async fn test_multiple_clients_basic_operations() {
    let (_server, addr) = create_and_start_test_server().await;
    
    let client_count = 10;
    let mut handles = Vec::new();
    
    // Spawn multiple clients performing basic operations
    for i in 0..client_count {
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            
            // Each client sets a unique key
            let key = format!("client_{}_key", i);
            let value = format!("client_{}_value", i);
            let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                                 key.len(), key, value.len(), value);
            
            let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b"+OK\r\n");
            
            // Get the value back
            let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
            let response = send_command(&mut stream, get_cmd.as_bytes()).await.unwrap();
            let expected = format!("${}\r\n{}\r\n", value.len(), value);
            assert_eq!(response, expected.as_bytes());
            
            // Check if key exists
            let exists_cmd = format!("*2\r\n$6\r\nEXISTS\r\n${}\r\n{}\r\n", key.len(), key);
            let response = send_command(&mut stream, exists_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b":1\r\n");
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all clients to complete
    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_multiple_clients_shared_data() {
    let (_server, addr) = create_and_start_test_server().await;
    
    let client_count = 20;
    let barrier = Arc::new(Barrier::new(client_count));
    let mut handles = Vec::new();
    
    // All clients will increment the same counter
    for i in 0..client_count {
        let barrier_clone = Arc::clone(&barrier);
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            
            // Wait for all clients to be ready
            barrier_clone.wait().await;
            
            // Each client increments the shared counter
            let incr_cmd = b"*2\r\n$4\r\nINCR\r\n$14\r\nshared_counter\r\n";
            let response = send_command(&mut stream, incr_cmd).await.unwrap();
            
            // Response should be an integer
            assert!(response.starts_with(b":"));
            
            // Parse the integer response
            let response_str = String::from_utf8_lossy(&response);
            let value_str = response_str.trim_start_matches(':').trim_end_matches("\r\n");
            let value: i64 = value_str.parse().unwrap();
            
            // Value should be between 1 and client_count
            assert!(value >= 1 && value <= client_count as i64);
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all clients to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Verify final counter value
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let get_cmd = b"*2\r\n$3\r\nGET\r\n$14\r\nshared_counter\r\n";
    let response = send_command(&mut stream, get_cmd).await.unwrap();
    
    let expected = format!("${}\r\n{}\r\n", client_count.to_string().len(), client_count);
    assert_eq!(response, expected.as_bytes());
}

#[tokio::test]
async fn test_multiple_clients_mixed_operations() {
    let (_server, addr) = create_and_start_test_server().await;
    
    let client_count = 15;
    let mut handles = Vec::new();
    
    // Clients perform different types of operations
    for i in 0..client_count {
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            
            match i % 5 {
                0 => {
                    // SET operations
                    for j in 0..5 {
                        let key = format!("set_client_{}_key_{}", i, j);
                        let value = format!("value_{}", j);
                        let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                                             key.len(), key, value.len(), value);
                        let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
                        assert_eq!(response, b"+OK\r\n");
                    }
                }
                1 => {
                    // GET operations (may not find keys initially)
                    for j in 0..5 {
                        let key = format!("get_test_key_{}", j);
                        let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
                        let _response = send_command(&mut stream, get_cmd.as_bytes()).await.unwrap();
                        // Response could be nil or a value, both are valid
                    }
                }
                2 => {
                    // INCR operations on different counters
                    for j in 0..3 {
                        let counter = format!("counter_{}", j);
                        let incr_cmd = format!("*2\r\n$4\r\nINCR\r\n${}\r\n{}\r\n", counter.len(), counter);
                        let response = send_command(&mut stream, incr_cmd.as_bytes()).await.unwrap();
                        assert!(response.starts_with(b":"));
                    }
                }
                3 => {
                    // EXISTS operations
                    for j in 0..5 {
                        let key = format!("exists_test_key_{}", j);
                        let exists_cmd = format!("*2\r\n$6\r\nEXISTS\r\n${}\r\n{}\r\n", key.len(), key);
                        let response = send_command(&mut stream, exists_cmd.as_bytes()).await.unwrap();
                        assert!(response.starts_with(b":"));
                    }
                }
                4 => {
                    // TTL operations
                    let key = format!("ttl_test_key_{}", i);
                    let value = "ttl_value";
                    
                    // Set key
                    let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                                         key.len(), key, value.len(), value);
                    let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
                    assert_eq!(response, b"+OK\r\n");
                    
                    // Set expiration
                    let expire_cmd = format!("*3\r\n$6\r\nEXPIRE\r\n${}\r\n{}\r\n$2\r\n60\r\n", key.len(), key);
                    let response = send_command(&mut stream, expire_cmd.as_bytes()).await.unwrap();
                    assert_eq!(response, b":1\r\n");
                    
                    // Check TTL
                    let ttl_cmd = format!("*2\r\n$3\r\nTTL\r\n${}\r\n{}\r\n", key.len(), key);
                    let response = send_command(&mut stream, ttl_cmd.as_bytes()).await.unwrap();
                    assert!(response.starts_with(b":"));
                }
                _ => unreachable!(),
            }
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all clients to complete
    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_multiple_clients_connection_persistence() {
    let (_server, addr) = create_and_start_test_server().await;
    
    let client_count = 8;
    let operations_per_client = 10;
    let mut handles = Vec::new();
    
    // Each client maintains a persistent connection and performs multiple operations
    for i in 0..client_count {
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            
            // Perform multiple operations on the same connection
            for j in 0..operations_per_client {
                let key = format!("persistent_client_{}_op_{}", i, j);
                let value = format!("value_{}", j);
                
                // SET
                let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                                     key.len(), key, value.len(), value);
                let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
                assert_eq!(response, b"+OK\r\n");
                
                // GET
                let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
                let response = send_command(&mut stream, get_cmd.as_bytes()).await.unwrap();
                let expected = format!("${}\r\n{}\r\n", value.len(), value);
                assert_eq!(response, expected.as_bytes());
                
                // Small delay between operations
                sleep(Duration::from_millis(10)).await;
            }
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all clients to complete
    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_multiple_clients_rapid_connect_disconnect() {
    let (_server, addr) = create_and_start_test_server().await;
    
    let connection_cycles = 50;
    let mut handles = Vec::new();
    
    // Rapidly connect, perform operation, and disconnect
    for i in 0..connection_cycles {
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            
            let key = format!("rapid_key_{}", i);
            let value = format!("rapid_value_{}", i);
            let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                                 key.len(), key, value.len(), value);
            
            let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b"+OK\r\n");
            
            // Immediately close connection
            drop(stream);
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all connections to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Verify server is still responsive
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let test_cmd = b"*3\r\n$3\r\nSET\r\n$4\r\ntest\r\n$5\r\nvalue\r\n";
    let response = send_command(&mut stream, test_cmd).await.unwrap();
    assert_eq!(response, b"+OK\r\n");
}

#[tokio::test]
async fn test_multiple_clients_error_handling() {
    let (_server, addr) = create_and_start_test_server().await;
    
    let client_count = 10;
    let mut handles = Vec::new();
    
    // Clients intentionally send invalid commands
    for i in 0..client_count {
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            
            // Send invalid command
            let invalid_cmd = format!("*1\r\n$7\r\nINVALID\r\n");
            let response = send_command(&mut stream, invalid_cmd.as_bytes()).await.unwrap();
            let response_str = String::from_utf8_lossy(&response);
            assert!(response_str.starts_with("-ERR"));
            
            // Connection should still work after error
            let valid_cmd = format!("*3\r\n$3\r\nSET\r\n$7\r\nerror_{}\r\n$5\r\nvalue\r\n", i);
            let response = send_command(&mut stream, valid_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b"+OK\r\n");
            
            // Send command with wrong arity
            let wrong_arity = b"*2\r\n$3\r\nSET\r\n$3\r\nkey\r\n"; // Missing value
            let response = send_command(&mut stream, wrong_arity).await.unwrap();
            let response_str = String::from_utf8_lossy(&response);
            assert!(response_str.starts_with("-ERR"));
            
            // Connection should still work
            let get_cmd = format!("*2\r\n$3\r\nGET\r\n$7\r\nerror_{}\r\n", i);
            let response = send_command(&mut stream, get_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b"$5\r\nvalue\r\n");
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all clients to complete
    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_multiple_clients_large_data() {
    let (_server, addr) = create_and_start_test_server().await;
    
    let client_count = 5;
    let mut handles = Vec::new();
    
    // Each client handles large values
    for i in 0..client_count {
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            
            // Create large value (10KB per client)
            let large_value = format!("client_{}_", i).repeat(1000); // ~10KB
            let key = format!("large_key_{}", i);
            let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                                 key.len(), key, large_value.len(), large_value);
            
            let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b"+OK\r\n");
            
            // Get the large value back
            let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
            
            stream.write_all(get_cmd.as_bytes()).await.unwrap();
            stream.flush().await.unwrap();
            
            // Read response with larger buffer for large data
            let mut buffer = vec![0u8; 20480]; // 20KB buffer
            let n = timeout(Duration::from_secs(10), stream.read(&mut buffer)).await.unwrap().unwrap();
            buffer.truncate(n);
            
            let expected = format!("${}\r\n{}\r\n", large_value.len(), large_value);
            assert_eq!(buffer, expected.as_bytes());
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all clients to complete
    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_multiple_clients_command_pipelining() {
    let (_server, addr) = create_and_start_test_server().await;
    
    let client_count = 5;
    let commands_per_client = 10;
    let mut handles = Vec::new();
    
    // Each client sends multiple commands in pipeline
    for i in 0..client_count {
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            
            // Prepare multiple commands
            let mut all_commands = Vec::new();
            let mut expected_responses = Vec::new();
            
            for j in 0..commands_per_client {
                let key = format!("pipeline_client_{}_cmd_{}", i, j);
                let value = format!("value_{}", j);
                
                // SET command
                let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                                     key.len(), key, value.len(), value);
                all_commands.push(set_cmd.as_bytes().to_vec());
                expected_responses.push(b"+OK\r\n".to_vec());
                
                // GET command
                let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
                all_commands.push(get_cmd.as_bytes().to_vec());
                let expected_get = format!("${}\r\n{}\r\n", value.len(), value);
                expected_responses.push(expected_get.as_bytes().to_vec());
            }
            
            // Send all commands at once
            for cmd in &all_commands {
                stream.write_all(cmd).await.unwrap();
            }
            stream.flush().await.unwrap();
            
            // Read all responses
            for expected in expected_responses {
                let mut buffer = vec![0u8; 1024];
                let n = timeout(Duration::from_secs(10), stream.read(&mut buffer)).await.unwrap().unwrap();
                buffer.truncate(n);
                assert_eq!(buffer, expected);
            }
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all clients to complete
    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_multiple_clients_stress_test() {
    let (_server, addr) = create_and_start_test_server().await;
    
    let client_count = 25;
    let operations_per_client = 20;
    let mut handles = Vec::new();
    
    // High-intensity stress test with many clients and operations
    for i in 0..client_count {
        let handle = tokio::spawn(async move {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            
            for j in 0..operations_per_client {
                match j % 4 {
                    0 => {
                        // SET operation
                        let key = format!("stress_{}_{}", i, j);
                        let value = format!("value_{}_{}", i, j);
                        let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                                             key.len(), key, value.len(), value);
                        let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
                        assert_eq!(response, b"+OK\r\n");
                    }
                    1 => {
                        // INCR operation
                        let counter = format!("stress_counter_{}", i % 5);
                        let incr_cmd = format!("*2\r\n$4\r\nINCR\r\n${}\r\n{}\r\n", counter.len(), counter);
                        let response = send_command(&mut stream, incr_cmd.as_bytes()).await.unwrap();
                        assert!(response.starts_with(b":"));
                    }
                    2 => {
                        // GET operation
                        let key = format!("stress_{}_{}", i, j - 2);
                        let get_cmd = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
                        let _response = send_command(&mut stream, get_cmd.as_bytes()).await.unwrap();
                        // Response could be nil or a value
                    }
                    3 => {
                        // EXISTS operation
                        let key = format!("stress_{}_{}", i, j - 3);
                        let exists_cmd = format!("*2\r\n$6\r\nEXISTS\r\n${}\r\n{}\r\n", key.len(), key);
                        let response = send_command(&mut stream, exists_cmd.as_bytes()).await.unwrap();
                        assert!(response.starts_with(b":"));
                    }
                    _ => unreachable!(),
                }
                
                // Small random delay to simulate real usage
                if j % 10 == 0 {
                    sleep(Duration::from_millis(1)).await;
                }
            }
            
            i
        });
        handles.push(handle);
    }
    
    // Wait for all clients to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Verify server is still responsive after stress test
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let final_test = b"*3\r\n$3\r\nSET\r\n$10\r\nfinal_test\r\n$2\r\nOK\r\n";
    let response = send_command(&mut stream, final_test).await.unwrap();
    assert_eq!(response, b"+OK\r\n");
}