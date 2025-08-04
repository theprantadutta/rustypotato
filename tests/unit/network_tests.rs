//! Comprehensive unit tests for network layer components
//!
//! Tests cover TCP server, connection handling, protocol parsing,
//! and network-related error conditions.

use rustypotato::{
    TcpServer, Config, MemoryStore, CommandRegistry,
    SetCommand, GetCommand, DelCommand, ExistsCommand
};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

#[cfg(test)]
mod tcp_server_tests {
    use super::*;

    async fn create_test_server() -> (TcpServer, Arc<Config>) {
        let mut config = Config::default();
        config.server.port = 0; // Use random port
        let config = Arc::new(config);
        
        let storage = Arc::new(MemoryStore::new());
        let mut command_registry = CommandRegistry::new();
        
        command_registry.register(Box::new(SetCommand));
        command_registry.register(Box::new(GetCommand));
        command_registry.register(Box::new(DelCommand));
        command_registry.register(Box::new(ExistsCommand));
        
        let command_registry = Arc::new(command_registry);
        let server = TcpServer::new(config.clone(), storage, command_registry);
        
        (server, config)
    }

    #[test]
    fn test_tcp_server_creation() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (server, config) = create_test_server().await;
            
            assert_eq!(server.bind_address(), "127.0.0.1:0");
            assert!(!server.is_running());
            assert_eq!(server.config().server.port, config.server.port);
        });
    }

    #[tokio::test]
    async fn test_tcp_server_start_stop() {
        let (mut server, _) = create_test_server().await;
        
        // Start server
        let addr = server.start_with_addr().await.unwrap();
        assert!(addr.port() > 0);
        
        // Give server time to start
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // Verify we can connect
        let stream = TcpStream::connect(addr).await;
        assert!(stream.is_ok());
        
        // Shutdown server
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_tcp_server_stats() {
        let (server, _) = create_test_server().await;
        
        let stats = server.stats().await;
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.total_connections_accepted, 0);
        assert!(stats.max_connections > 0);
    }

    #[tokio::test]
    async fn test_tcp_server_bind_address_formats() {
        // Test different bind address formats
        let test_cases = vec![
            ("127.0.0.1", 0),
            ("localhost", 0),
            ("0.0.0.0", 0),
        ];

        for (bind_addr, port) in test_cases {
            let mut config = Config::default();
            config.server.bind_address = bind_addr.to_string();
            config.server.port = port;
            let config = Arc::new(config);
            
            let storage = Arc::new(MemoryStore::new());
            let command_registry = Arc::new(CommandRegistry::new());
            
            let server = TcpServer::new(config, storage, command_registry);
            assert_eq!(server.bind_address(), format!("{}:{}", bind_addr, port));
        }
    }
}

#[cfg(test)]
mod connection_tests {
    use super::*;

    async fn send_command(stream: &mut TcpStream, command: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        stream.write_all(command).await?;
        stream.flush().await?;
        
        let mut buffer = vec![0u8; 1024];
        let n = timeout(Duration::from_secs(5), stream.read(&mut buffer)).await??;
        buffer.truncate(n);
        Ok(buffer)
    }

    #[tokio::test]
    async fn test_single_connection_basic_commands() {
        let (mut server, _) = create_test_server().await;
        let addr = server.start_with_addr().await.unwrap();
        
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Test SET command
        let set_cmd = b"*3\r\n$3\r\nSET\r\n$4\r\ntest\r\n$5\r\nvalue\r\n";
        let response = send_command(&mut stream, set_cmd).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        // Test GET command
        let get_cmd = b"*2\r\n$3\r\nGET\r\n$4\r\ntest\r\n";
        let response = send_command(&mut stream, get_cmd).await.unwrap();
        assert_eq!(response, b"$5\r\nvalue\r\n");
        
        drop(stream);
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_multiple_connections() {
        let (mut server, _) = create_test_server().await;
        let addr = server.start_with_addr().await.unwrap();
        
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        let mut handles = Vec::new();
        
        // Create multiple concurrent connections
        for i in 0..5 {
            let handle = tokio::spawn(async move {
                let mut stream = TcpStream::connect(addr).await.unwrap();
                
                let key = format!("key{}", i);
                let value = format!("value{}", i);
                let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                                     key.len(), key, value.len(), value);
                
                let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
                assert_eq!(response, b"+OK\r\n");
                
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
        
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_connection_persistence() {
        let (mut server, _) = create_test_server().await;
        let addr = server.start_with_addr().await.unwrap();
        
        tokio::time::sleep(Duration::from_millis(50)).await;
        
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
        
        drop(stream);
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_connection_error_handling() {
        let (mut server, _) = create_test_server().await;
        let addr = server.start_with_addr().await.unwrap();
        
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Test unknown command
        let unknown_cmd = b"*1\r\n$7\r\nUNKNOWN\r\n";
        let response = send_command(&mut stream, unknown_cmd).await.unwrap();
        let response_str = String::from_utf8_lossy(&response);
        assert!(response_str.starts_with("-ERR"));
        
        // Test invalid protocol
        let invalid_cmd = b"invalid protocol data\r\n";
        let response = send_command(&mut stream, invalid_cmd).await.unwrap();
        let response_str = String::from_utf8_lossy(&response);
        assert!(response_str.starts_with("-ERR"));
        
        // Connection should still be alive after errors
        let set_cmd = b"*3\r\n$3\r\nSET\r\n$4\r\ntest\r\n$5\r\nvalue\r\n";
        let response = send_command(&mut stream, set_cmd).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        drop(stream);
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_partial_command_handling() {
        let (mut server, _) = create_test_server().await;
        let addr = server.start_with_addr().await.unwrap();
        
        tokio::time::sleep(Duration::from_millis(50)).await;
        
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
        
        drop(stream);
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_large_command_handling() {
        let (mut server, _) = create_test_server().await;
        let addr = server.start_with_addr().await.unwrap();
        
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Create a large value (1KB)
        let large_value = "x".repeat(1024);
        let set_cmd = format!("*3\r\n$3\r\nSET\r\n$9\r\nlarge_key\r\n${}\r\n{}\r\n", 
                             large_value.len(), large_value);
        
        let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        // Get the large value back
        let get_cmd = b"*2\r\n$3\r\nGET\r\n$9\r\nlarge_key\r\n";
        
        stream.write_all(get_cmd).await.unwrap();
        stream.flush().await.unwrap();
        
        // For large responses, we might need a bigger buffer
        let mut buffer = vec![0u8; 2048];
        let n = timeout(Duration::from_secs(5), stream.read(&mut buffer)).await.unwrap().unwrap();
        buffer.truncate(n);
        
        let expected = format!("${}\r\n{}\r\n", large_value.len(), large_value);
        assert_eq!(buffer, expected.as_bytes());
        
        drop(stream);
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_connection_timeout_handling() {
        let (mut server, _) = create_test_server().await;
        let addr = server.start_with_addr().await.unwrap();
        
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Send a valid command first
        let set_cmd = b"*3\r\n$3\r\nSET\r\n$4\r\ntest\r\n$5\r\nvalue\r\n";
        let response = send_command(&mut stream, set_cmd).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        // Keep connection idle for a while (but not long enough to timeout in test)
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Connection should still work
        let get_cmd = b"*2\r\n$3\r\nGET\r\n$4\r\ntest\r\n";
        let response = send_command(&mut stream, get_cmd).await.unwrap();
        assert_eq!(response, b"$5\r\nvalue\r\n");
        
        drop(stream);
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_graceful_connection_close() {
        let (mut server, _) = create_test_server().await;
        let addr = server.start_with_addr().await.unwrap();
        
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Send a command
        let set_cmd = b"*3\r\n$3\r\nSET\r\n$4\r\ntest\r\n$5\r\nvalue\r\n";
        let response = send_command(&mut stream, set_cmd).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        // Close connection gracefully
        stream.shutdown().await.unwrap();
        
        // Server should handle the closed connection without issues
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        server.shutdown().await.unwrap();
    }
}

#[cfg(test)]
mod protocol_tests {
    use super::*;

    #[tokio::test]
    async fn test_resp_protocol_parsing() {
        let (mut server, _) = create_test_server().await;
        let addr = server.start_with_addr().await.unwrap();
        
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Test various RESP protocol formats
        let test_cases = vec![
            // Simple SET command
            (b"*3\r\n$3\r\nSET\r\n$4\r\nkey1\r\n$6\r\nvalue1\r\n".to_vec(), b"+OK\r\n".to_vec()),
            // GET command
            (b"*2\r\n$3\r\nGET\r\n$4\r\nkey1\r\n".to_vec(), b"$6\r\nvalue1\r\n".to_vec()),
            // EXISTS command
            (b"*2\r\n$6\r\nEXISTS\r\n$4\r\nkey1\r\n".to_vec(), b":1\r\n".to_vec()),
            // DEL command
            (b"*2\r\n$3\r\nDEL\r\n$4\r\nkey1\r\n".to_vec(), b":1\r\n".to_vec()),
        ];
        
        for (command, expected_response) in test_cases {
            let response = send_command(&mut stream, &command).await.unwrap();
            assert_eq!(response, expected_response);
        }
        
        drop(stream);
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_resp_protocol_edge_cases() {
        let (mut server, _) = create_test_server().await;
        let addr = server.start_with_addr().await.unwrap();
        
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Test edge cases
        
        // Empty string value
        let set_empty = b"*3\r\n$3\r\nSET\r\n$5\r\nempty\r\n$0\r\n\r\n";
        let response = send_command(&mut stream, set_empty).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        let get_empty = b"*2\r\n$3\r\nGET\r\n$5\r\nempty\r\n";
        let response = send_command(&mut stream, get_empty).await.unwrap();
        assert_eq!(response, b"$0\r\n\r\n");
        
        // Key with special characters
        let special_key = "key\r\nwith\tspecial\nchars";
        let set_special = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n$5\r\nvalue\r\n", 
                                 special_key.len(), special_key);
        let response = send_command(&mut stream, set_special.as_bytes()).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        drop(stream);
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_malformed_protocol_handling() {
        let (mut server, _) = create_test_server().await;
        let addr = server.start_with_addr().await.unwrap();
        
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        let mut stream = TcpStream::connect(addr).await.unwrap();
        
        // Test malformed protocol messages
        let malformed_cases = vec![
            b"invalid\r\n".to_vec(),
            b"*\r\n".to_vec(),
            b"*abc\r\n".to_vec(),
            b"*3\r\n$\r\n".to_vec(),
            b"*3\r\n$abc\r\n".to_vec(),
            b"*3\r\n$3\r\nSET\r\n$4\r\nkey\r\n".to_vec(), // Missing value
        ];
        
        for malformed_cmd in malformed_cases {
            let response = send_command(&mut stream, &malformed_cmd).await.unwrap();
            let response_str = String::from_utf8_lossy(&response);
            assert!(response_str.starts_with("-ERR"), 
                   "Expected error response for malformed command, got: {}", response_str);
        }
        
        // Connection should still work after errors
        let valid_cmd = b"*3\r\n$3\r\nSET\r\n$4\r\ntest\r\n$5\r\nvalue\r\n";
        let response = send_command(&mut stream, valid_cmd).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        drop(stream);
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_command_pipelining() {
        let (mut server, _) = create_test_server().await;
        let addr = server.start_with_addr().await.unwrap();
        
        tokio::time::sleep(Duration::from_millis(50)).await;
        
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
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
}

#[cfg(test)]
mod stress_tests {
    use super::*;

    #[tokio::test]
    async fn test_high_connection_count() {
        let (mut server, _) = create_test_server().await;
        let addr = server.start_with_addr().await.unwrap();
        
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        let connection_count = 50;
        let mut handles = Vec::new();
        
        for i in 0..connection_count {
            let handle = tokio::spawn(async move {
                let mut stream = TcpStream::connect(addr).await.unwrap();
                
                let key = format!("stress_key_{}", i);
                let value = format!("stress_value_{}", i);
                let set_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n", 
                                     key.len(), key, value.len(), value);
                
                let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
                assert_eq!(response, b"+OK\r\n");
                
                // Keep connection alive briefly
                tokio::time::sleep(Duration::from_millis(10)).await;
                
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
        
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_rapid_connect_disconnect() {
        let (mut server, _) = create_test_server().await;
        let addr = server.start_with_addr().await.unwrap();
        
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // Rapidly connect and disconnect
        for i in 0..20 {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            
            let set_cmd = format!("*3\r\n$3\r\nSET\r\n$7\r\nrapid_{}\r\n$5\r\nvalue\r\n", i);
            let response = send_command(&mut stream, set_cmd.as_bytes()).await.unwrap();
            assert_eq!(response, b"+OK\r\n");
            
            // Immediately close connection
            drop(stream);
        }
        
        // Verify server is still responsive
        let mut stream = TcpStream::connect(addr).await.unwrap();
        let set_cmd = b"*3\r\n$3\r\nSET\r\n$4\r\ntest\r\n$5\r\nvalue\r\n";
        let response = send_command(&mut stream, set_cmd).await.unwrap();
        assert_eq!(response, b"+OK\r\n");
        
        drop(stream);
        server.shutdown().await.unwrap();
    }
}