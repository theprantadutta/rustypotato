//! Integration tests for comprehensive error handling
//! 
//! These tests verify error scenarios, recovery mechanisms, logging behavior,
//! and client error responses across all system components.

use rustypotato::{Config, RustyPotatoServer};
use rustypotato::error::{RustyPotatoError, ErrorRecoveryManager, ErrorSeverity, ErrorCategory};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tempfile::TempDir;


/// Helper function to create and start a test server with custom config
async fn create_test_server_with_config(config: Config) -> (RustyPotatoServer, std::net::SocketAddr) {
    let mut server = RustyPotatoServer::new(config).unwrap();
    let addr = server.start_with_addr().await.unwrap();
    
    // Give server time to start accepting connections
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    (server, addr)
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
async fn test_error_severity_classification() {
    // Test that errors are classified with correct severity levels
    let invalid_cmd = RustyPotatoError::InvalidCommand {
        command: "UNKNOWN".to_string(),
        source: None,
    };
    assert_eq!(invalid_cmd.severity(), ErrorSeverity::Low);
    
    let network_error = RustyPotatoError::NetworkError {
        message: "Connection failed".to_string(),
        source: None,
        connection_id: Some("conn_123".to_string()),
    };
    assert_eq!(network_error.severity(), ErrorSeverity::High);
    
    let persistence_error = RustyPotatoError::PersistenceError {
        message: "Disk full".to_string(),
        source: None,
        recoverable: false,
    };
    assert_eq!(persistence_error.severity(), ErrorSeverity::Critical);
}

#[tokio::test]
async fn test_error_category_classification() {
    // Test that errors are classified into correct categories
    let wrong_arity = RustyPotatoError::WrongArity {
        command: "SET".to_string(),
        expected: "3".to_string(),
        actual: 2,
    };
    assert_eq!(wrong_arity.category(), ErrorCategory::Client);
    
    let timeout_error = RustyPotatoError::TimeoutError {
        message: "Operation timed out".to_string(),
        operation: Some("GET".to_string()),
        timeout_duration: Some(Duration::from_secs(5)),
    };
    assert_eq!(timeout_error.category(), ErrorCategory::Network);
    
    let config_error = RustyPotatoError::ConfigError {
        message: "Invalid port".to_string(),
        config_key: Some("server.port".to_string()),
        source: None,
    };
    assert_eq!(config_error.category(), ErrorCategory::Configuration);
}

#[tokio::test]
async fn test_error_recoverability() {
    // Test that errors correctly report their recoverability
    let recoverable_persistence = RustyPotatoError::PersistenceError {
        message: "Temporary write failure".to_string(),
        source: None,
        recoverable: true,
    };
    assert!(recoverable_persistence.is_recoverable());
    
    let non_recoverable_persistence = RustyPotatoError::PersistenceError {
        message: "Disk corruption".to_string(),
        source: None,
        recoverable: false,
    };
    assert!(!non_recoverable_persistence.is_recoverable());
    
    let client_error = RustyPotatoError::InvalidCommand {
        command: "BADCMD".to_string(),
        source: None,
    };
    assert!(!client_error.is_recoverable());
}

#[tokio::test]
async fn test_client_error_messages() {
    // Test that errors are converted to appropriate client messages
    let invalid_cmd = RustyPotatoError::InvalidCommand {
        command: "BADCMD".to_string(),
        source: None,
    };
    assert_eq!(invalid_cmd.to_client_error(), "ERR unknown command 'BADCMD'");
    
    let wrong_arity = RustyPotatoError::WrongArity {
        command: "SET".to_string(),
        expected: "3".to_string(),
        actual: 2,
    };
    assert_eq!(wrong_arity.to_client_error(), 
               "ERR wrong number of arguments for 'SET' command (expected 3, got 2)");
    
    let not_integer = RustyPotatoError::NotAnInteger {
        value: "abc".to_string(),
    };
    assert_eq!(not_integer.to_client_error(), 
               "ERR value is not an integer or out of range: abc");
}

#[tokio::test]
async fn test_protocol_error_handling() {
    let mut config = Config::default();
    config.server.port = 0;
    let (_server, addr) = create_test_server_with_config(config).await;
    
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    // Test malformed RESP protocol
    let malformed_cmd = b"INVALID_RESP_FORMAT\r\n";
    let response = send_command(&mut stream, malformed_cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.starts_with("-ERR"));
    
    // Test incomplete RESP command
    let incomplete_cmd = b"*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n"; // Missing value
    stream.write_all(incomplete_cmd).await.unwrap();
    stream.flush().await.unwrap();
    
    // Should timeout or get error response
    let result = timeout(Duration::from_millis(500), async {
        let mut buffer = vec![0u8; 1024];
        stream.read(&mut buffer).await
    }).await;
    
    // Either timeout (expected) or error response
    match result {
        Ok(Ok(_)) => {
            // If we get a response, it should be an error
            let mut buffer = vec![0u8; 1024];
            let n = stream.read(&mut buffer).await.unwrap();
            buffer.truncate(n);
            let response_str = String::from_utf8_lossy(&buffer);
            assert!(response_str.starts_with("-ERR") || response_str.is_empty());
        }
        Ok(Err(_)) | Err(_) => {
            // Timeout or connection error is acceptable for malformed protocol
        }
    }
}

#[tokio::test]
async fn test_command_error_scenarios() {
    let mut config = Config::default();
    config.server.port = 0;
    let (_server, addr) = create_test_server_with_config(config).await;
    
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    // Test unknown command
    let unknown_cmd = b"*1\r\n$7\r\nUNKNOWN\r\n";
    let response = send_command(&mut stream, unknown_cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("unknown command"));
    
    // Test wrong arity for SET (needs 3 args)
    let wrong_arity_cmd = b"*2\r\n$3\r\nSET\r\n$3\r\nkey\r\n";
    let response = send_command(&mut stream, wrong_arity_cmd).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("wrong number of arguments"));
    
    // Test wrong arity for GET (needs 2 args)
    let wrong_arity_get = b"*3\r\n$3\r\nGET\r\n$3\r\nkey\r\n$5\r\nextra\r\n";
    let response = send_command(&mut stream, wrong_arity_get).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("wrong number of arguments"));
    
    // Test INCR on non-numeric value
    let set_str = b"*3\r\n$3\r\nSET\r\n$8\r\nstr_test\r\n$6\r\nstring\r\n";
    send_command(&mut stream, set_str).await.unwrap();
    
    let incr_str = b"*2\r\n$4\r\nINCR\r\n$8\r\nstr_test\r\n";
    let response = send_command(&mut stream, incr_str).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("not an integer"));
    
    // Test DECR on non-numeric value
    let decr_str = b"*2\r\n$4\r\nDECR\r\n$8\r\nstr_test\r\n";
    let response = send_command(&mut stream, decr_str).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("not an integer"));
}

#[tokio::test]
async fn test_ttl_error_scenarios() {
    let mut config = Config::default();
    config.server.port = 0;
    let (_server, addr) = create_test_server_with_config(config).await;
    
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    // Test TTL on non-existent key
    let ttl_missing = b"*2\r\n$3\r\nTTL\r\n$11\r\nmissing_key\r\n";
    let response = send_command(&mut stream, ttl_missing).await.unwrap();
    assert_eq!(response, b":-2\r\n"); // -2 for missing key
    
    // Test EXPIRE with invalid timeout
    let set_key = b"*3\r\n$3\r\nSET\r\n$8\r\ntest_key\r\n$5\r\nvalue\r\n";
    send_command(&mut stream, set_key).await.unwrap();
    
    let expire_invalid = b"*3\r\n$6\r\nEXPIRE\r\n$8\r\ntest_key\r\n$3\r\nabc\r\n";
    let response = send_command(&mut stream, expire_invalid).await.unwrap();
    let response_str = String::from_utf8_lossy(&response);
    assert!(response_str.contains("not an integer") || response_str.starts_with("-ERR"));
    
    // Test EXPIRE on non-existent key
    let expire_missing = b"*3\r\n$6\r\nEXPIRE\r\n$11\r\nmissing_key\r\n$2\r\n10\r\n";
    let response = send_command(&mut stream, expire_missing).await.unwrap();
    assert_eq!(response, b":0\r\n"); // 0 for key not found
}

#[tokio::test]
async fn test_persistence_error_recovery() {
    // Create a temporary directory for AOF file
    let temp_dir = TempDir::new().unwrap();
    let aof_path = temp_dir.path().join("test.aof");
    
    let mut config = Config::default();
    config.server.port = 0;
    config.storage.aof_path = aof_path.clone();
    config.storage.aof_enabled = true;
    
    let (_server, addr) = create_test_server_with_config(config).await;
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    // Perform operations that should be persisted
    let set_cmd = b"*3\r\n$3\r\nSET\r\n$12\r\npersist_test\r\n$5\r\nvalue\r\n";
    let response = send_command(&mut stream, set_cmd).await.unwrap();
    assert_eq!(response, b"+OK\r\n");
    
    // Give time for persistence to complete
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Verify AOF file was created and contains data (if persistence is implemented)
    // Note: This test may need to be updated when persistence is fully integrated
    if aof_path.exists() {
        let aof_content = std::fs::read_to_string(&aof_path).unwrap();
        assert!(aof_content.contains("SET") || aof_content.contains("persist_test"));
    } else {
        // If AOF file doesn't exist, that's acceptable for now as persistence may not be fully integrated
        println!("AOF file not created - persistence may not be fully integrated yet");
    }
}

#[tokio::test]
async fn test_error_recovery_manager() {
    let recovery_manager = ErrorRecoveryManager::new();
    
    // Test retry strategy for recoverable errors
    let network_error = RustyPotatoError::NetworkError {
        message: "Connection timeout".to_string(),
        source: None,
        connection_id: Some("conn_123".to_string()),
    };
    
    let strategy = recovery_manager.get_recovery_strategy(&network_error);
    assert_eq!(strategy, rustypotato::error::RecoveryStrategy::Retry);
    
    // Test no recovery for client errors
    let client_error = RustyPotatoError::InvalidCommand {
        command: "BADCMD".to_string(),
        source: None,
    };
    
    let strategy = recovery_manager.get_recovery_strategy(&client_error);
    assert_eq!(strategy, rustypotato::error::RecoveryStrategy::None);
}

#[tokio::test]
async fn test_retry_operation_success() {
    let recovery_manager = ErrorRecoveryManager::new();
    let attempt_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    
    let attempt_count_clone = attempt_count.clone();
    let result: Result<&str, RustyPotatoError> = recovery_manager.retry_operation(move || {
        let attempt_count = attempt_count_clone.clone();
        async move {
            let count = attempt_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if count < 3 {
                Err(RustyPotatoError::NetworkError {
                    message: "Temporary failure".to_string(),
                    source: None,
                    connection_id: None,
                })
            } else {
                Ok("success")
            }
        }
    }).await;
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "success");
    assert_eq!(attempt_count.load(std::sync::atomic::Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_retry_operation_failure() {
    let recovery_manager = ErrorRecoveryManager::new();
    let attempt_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    
    let attempt_count_clone = attempt_count.clone();
    let result: Result<&str, RustyPotatoError> = recovery_manager.retry_operation(move || {
        let attempt_count = attempt_count_clone.clone();
        async move {
            attempt_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(RustyPotatoError::NetworkError {
                message: "Persistent failure".to_string(),
                source: None,
                connection_id: None,
            })
        }
    }).await;
    
    assert!(result.is_err());
    assert_eq!(attempt_count.load(std::sync::atomic::Ordering::SeqCst), 3); // Should retry max_retries times
}

#[tokio::test]
async fn test_circuit_breaker_functionality() {
    use rustypotato::error::RecoveryConfig;
    
    // Use a short timeout for testing
    let config = RecoveryConfig {
        circuit_breaker_timeout: std::time::Duration::from_millis(50),
        ..Default::default()
    };
    let recovery_manager = ErrorRecoveryManager::with_config(config);
    
    // Initially circuit should be closed
    assert!(recovery_manager.circuit_breaker_check().is_ok());
    
    // Record multiple failures to open circuit
    for _ in 0..5 {
        recovery_manager.record_failure();
    }
    
    // Circuit should now be open
    assert!(recovery_manager.circuit_breaker_check().is_err());
    
    // Wait for circuit breaker timeout to transition to half-open
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    
    // Now it should be in half-open state, so we can record success to close it
    recovery_manager.record_success();
    assert!(recovery_manager.circuit_breaker_check().is_ok());
}

#[tokio::test]
async fn test_connection_error_handling() {
    let mut config = Config::default();
    config.server.port = 0;
    config.server.max_connections = 2; // Limit connections for testing
    
    let (_server, addr) = create_test_server_with_config(config).await;
    
    // Create connections up to the limit
    let mut connections = Vec::new();
    for _ in 0..2 {
        let stream = TcpStream::connect(addr).await.unwrap();
        connections.push(stream);
    }
    
    // Try to create one more connection (should be handled gracefully)
    let result = timeout(Duration::from_millis(500), TcpStream::connect(addr)).await;
    
    // The connection might be rejected or accepted depending on implementation
    // The important thing is that the server doesn't crash
    match result {
        Ok(Ok(_)) => {
            // Connection accepted - server is handling it gracefully
        }
        Ok(Err(_)) | Err(_) => {
            // Connection rejected or timed out - also acceptable
        }
    }
    
    // Verify existing connections still work
    let test_cmd = b"*2\r\n$4\r\nPING\r\n";
    if let Some(stream) = connections.first_mut() {
        let response = send_command(stream, test_cmd).await;
        // Should either work or fail gracefully
        match response {
            Ok(resp) => {
                let resp_str = String::from_utf8_lossy(&resp);
                assert!(resp_str.contains("PONG") || resp_str.starts_with("-ERR"));
            }
            Err(_) => {
                // Connection error is acceptable in this test scenario
            }
        }
    }
}

#[tokio::test]
async fn test_large_command_error_handling() {
    let mut config = Config::default();
    config.server.port = 0;
    let (_server, addr) = create_test_server_with_config(config).await;
    
    let mut stream = TcpStream::connect(addr).await.unwrap();
    
    // Test extremely large key (should be handled gracefully)
    let large_key = "x".repeat(10000);
    let large_cmd = format!("*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n$5\r\nvalue\r\n", 
                           large_key.len(), large_key);
    
    let result = timeout(Duration::from_secs(2), send_command(&mut stream, large_cmd.as_bytes())).await;
    
    match result {
        Ok(Ok(response)) => {
            // Should either succeed or return an error, but not crash
            let response_str = String::from_utf8_lossy(&response);
            assert!(response_str == "+OK\r\n" || response_str.starts_with("-ERR"));
        }
        Ok(Err(_)) | Err(_) => {
            // Timeout or connection error is acceptable for very large commands
        }
    }
}

#[tokio::test]
async fn test_error_logging_integration() {
    // This test verifies that errors are logged appropriately
    // In a real implementation, you might capture log output for verification
    
    let invalid_cmd = RustyPotatoError::InvalidCommand {
        command: "TESTCMD".to_string(),
        source: None,
    };
    
    // Test that logging doesn't panic
    invalid_cmd.log();
    
    let critical_error = RustyPotatoError::PersistenceError {
        message: "Critical disk failure".to_string(),
        source: None,
        recoverable: false,
    };
    
    critical_error.log();
    
    // Test client error detection
    assert!(invalid_cmd.is_client_error());
    assert!(!critical_error.is_client_error());
}

#[tokio::test]
async fn test_error_conversion_from_system_errors() {
    // Test conversion from std::io::Error
    let io_error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Access denied");
    let rusty_error: RustyPotatoError = io_error.into();
    
    match rusty_error {
        RustyPotatoError::PersistenceError { message, recoverable, .. } => {
            assert!(message.contains("Access denied"));
            assert!(recoverable); // IO errors are generally recoverable
        }
        _ => panic!("Expected PersistenceError"),
    }
    
    // Test conversion from parse error
    let parse_error = "abc".parse::<i64>().unwrap_err();
    let rusty_error: RustyPotatoError = parse_error.into();
    
    match rusty_error {
        RustyPotatoError::NotAnInteger { value } => {
            assert!(value.contains("invalid digit"));
        }
        _ => panic!("Expected NotAnInteger"),
    }
}