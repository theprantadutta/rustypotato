//! Integration tests for CLI functionality
//!
//! These tests verify that the CLI client can properly connect to the server,
//! execute commands, and handle various scenarios including error conditions.

use rustypotato::cli::CliClient;
use rustypotato::commands::ResponseValue;
use rustypotato::error::RustyPotatoError;
use std::time::Duration;
use tokio::time::timeout;

/// Test helper to create a CLI client
fn create_test_client() -> CliClient {
    CliClient::with_address("127.0.0.1:6379".to_string())
}

/// Test helper to create a CLI client with custom address
fn create_test_client_with_address(address: &str) -> CliClient {
    CliClient::with_address(address.to_string())
}

#[tokio::test]
async fn test_cli_client_creation() {
    let client = create_test_client();
    assert!(!client.is_connected());
}

#[tokio::test]
async fn test_cli_client_with_custom_address() {
    let client = create_test_client_with_address("192.168.1.100:6380");
    assert!(!client.is_connected());
}

#[tokio::test]
async fn test_cli_client_connection_failure() {
    let mut client = create_test_client_with_address("127.0.0.1:9999"); // Non-existent port

    let result = client.connect().await;
    assert!(result.is_err());
    assert!(!client.is_connected());

    if let Err(RustyPotatoError::ConnectionError { message, .. }) = result {
        assert!(message.contains("Failed to connect"));
    } else {
        panic!("Expected ConnectionError");
    }
}

#[tokio::test]
async fn test_cli_client_execute_without_connection() {
    let mut client = create_test_client();

    let result = client.execute_command("GET", &["test".to_string()]).await;
    assert!(result.is_err());

    if let Err(RustyPotatoError::ConnectionError { message, .. }) = result {
        assert!(message.contains("Not connected"));
    } else {
        panic!("Expected ConnectionError");
    }
}

#[tokio::test]
async fn test_cli_client_disconnect_without_connection() {
    let mut client = create_test_client();

    let result = client.disconnect().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_response_formatting() {
    let client = create_test_client();

    // Test simple string
    let response = ResponseValue::SimpleString("OK".to_string());
    assert_eq!(client.format_response(&response), "OK");

    // Test bulk string
    let response = ResponseValue::BulkString(Some("hello".to_string()));
    assert_eq!(client.format_response(&response), "hello");

    // Test nil
    let response = ResponseValue::BulkString(None);
    assert_eq!(client.format_response(&response), "(nil)");

    let response = ResponseValue::Nil;
    assert_eq!(client.format_response(&response), "(nil)");

    // Test integer
    let response = ResponseValue::Integer(42);
    assert_eq!(client.format_response(&response), "(integer) 42");

    // Test empty array
    let response = ResponseValue::Array(vec![]);
    assert_eq!(client.format_response(&response), "(empty array)");

    // Test array with elements
    let response = ResponseValue::Array(vec![
        ResponseValue::SimpleString("first".to_string()),
        ResponseValue::Integer(2),
        ResponseValue::BulkString(Some("third".to_string())),
    ]);
    let formatted = client.format_response(&response);
    assert!(formatted.contains("1) first"));
    assert!(formatted.contains("2) (integer) 2"));
    assert!(formatted.contains("3) third"));
}

#[tokio::test]
async fn test_command_array_encoding() {
    let mut client = create_test_client();

    // Test encoding a simple command
    let encoded = client
        .encode_command_array(&["GET".to_string(), "key".to_string()])
        .unwrap();

    // Should be RESP array format: *2\r\n$3\r\nGET\r\n$3\r\nkey\r\n
    let expected = b"*2\r\n$3\r\nGET\r\n$3\r\nkey\r\n";
    assert_eq!(encoded, expected);
}

#[tokio::test]
async fn test_command_array_encoding_with_empty_string() {
    let mut client = create_test_client();

    let encoded = client
        .encode_command_array(&["SET".to_string(), "key".to_string(), "".to_string()])
        .unwrap();

    // Should handle empty string properly
    let expected = b"*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$0\r\n\r\n";
    assert_eq!(encoded, expected);
}

#[tokio::test]
async fn test_command_array_encoding_single_command() {
    let mut client = create_test_client();

    let encoded = client.encode_command_array(&["PING".to_string()]).unwrap();

    let expected = b"*1\r\n$4\r\nPING\r\n";
    assert_eq!(encoded, expected);
}

// Note: The following tests would require a running server
// They are marked as ignored by default and can be run with --ignored flag

#[tokio::test]
#[ignore = "requires running server"]
async fn test_cli_client_basic_operations() {
    let mut client = create_test_client();

    // Connect to server
    client.connect().await.expect("Failed to connect to server");
    assert!(client.is_connected());

    // Test SET command
    let result = client
        .execute_command("SET", &["test_key".to_string(), "test_value".to_string()])
        .await;
    assert!(result.is_ok());

    // Test GET command
    let result = client
        .execute_command("GET", &["test_key".to_string()])
        .await;
    assert!(result.is_ok());
    if let Ok(ResponseValue::BulkString(Some(value))) = result {
        assert_eq!(value, "test_value");
    }

    // Test EXISTS command
    let result = client
        .execute_command("EXISTS", &["test_key".to_string()])
        .await;
    assert!(result.is_ok());
    if let Ok(ResponseValue::Integer(exists)) = result {
        assert_eq!(exists, 1);
    }

    // Test DEL command
    let result = client
        .execute_command("DEL", &["test_key".to_string()])
        .await;
    assert!(result.is_ok());

    // Verify key is deleted
    let result = client
        .execute_command("GET", &["test_key".to_string()])
        .await;
    assert!(result.is_ok());
    if let Ok(ResponseValue::BulkString(None)) = result {
        // Expected - key should be gone
    } else {
        panic!("Expected nil response for deleted key");
    }

    // Disconnect
    client.disconnect().await.expect("Failed to disconnect");
    assert!(!client.is_connected());
}

#[tokio::test]
#[ignore = "requires running server"]
async fn test_cli_client_ttl_operations() {
    let mut client = create_test_client();

    client.connect().await.expect("Failed to connect to server");

    // Set a key
    client
        .execute_command("SET", &["ttl_key".to_string(), "ttl_value".to_string()])
        .await
        .unwrap();

    // Set expiration
    let result = client
        .execute_command("EXPIRE", &["ttl_key".to_string(), "60".to_string()])
        .await;
    assert!(result.is_ok());

    // Check TTL
    let result = client
        .execute_command("TTL", &["ttl_key".to_string()])
        .await;
    assert!(result.is_ok());
    if let Ok(ResponseValue::Integer(ttl)) = result {
        assert!(ttl > 0 && ttl <= 60);
    }

    // Clean up
    client
        .execute_command("DEL", &["ttl_key".to_string()])
        .await
        .unwrap();
    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore = "requires running server"]
async fn test_cli_client_atomic_operations() {
    let mut client = create_test_client();

    client.connect().await.expect("Failed to connect to server");

    // Test INCR on non-existent key
    let result = client
        .execute_command("INCR", &["counter".to_string()])
        .await;
    assert!(result.is_ok());
    if let Ok(ResponseValue::Integer(value)) = result {
        assert_eq!(value, 1);
    }

    // Test INCR on existing key
    let result = client
        .execute_command("INCR", &["counter".to_string()])
        .await;
    assert!(result.is_ok());
    if let Ok(ResponseValue::Integer(value)) = result {
        assert_eq!(value, 2);
    }

    // Test DECR
    let result = client
        .execute_command("DECR", &["counter".to_string()])
        .await;
    assert!(result.is_ok());
    if let Ok(ResponseValue::Integer(value)) = result {
        assert_eq!(value, 1);
    }

    // Clean up
    client
        .execute_command("DEL", &["counter".to_string()])
        .await
        .unwrap();
    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore = "requires running server"]
async fn test_cli_client_error_handling() {
    let mut client = create_test_client();

    client.connect().await.expect("Failed to connect to server");

    // Test invalid command
    let result = client.execute_command("INVALID", &[]).await;
    // Should get an error response, not a Rust error
    assert!(result.is_ok());
    if let Ok(ResponseValue::SimpleString(msg)) = result {
        assert!(msg.contains("ERR"));
    }

    // Test wrong arity
    let result = client
        .execute_command("SET", &["only_key".to_string()])
        .await;
    assert!(result.is_ok());
    if let Ok(ResponseValue::SimpleString(msg)) = result {
        assert!(msg.contains("ERR"));
    }

    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore = "requires running server"]
async fn test_cli_client_connection_timeout() {
    let mut client = create_test_client();

    client.connect().await.expect("Failed to connect to server");

    // Execute a command with timeout
    let result = timeout(
        Duration::from_secs(5),
        client.execute_command("GET", &["test".to_string()]),
    )
    .await;

    assert!(result.is_ok(), "Command should complete within timeout");

    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore = "requires running server"]
async fn test_cli_client_large_value() {
    let mut client = create_test_client();

    client.connect().await.expect("Failed to connect to server");

    // Test with a large value
    let large_value = "x".repeat(10000);
    let result = client
        .execute_command("SET", &["large_key".to_string(), large_value.clone()])
        .await;
    assert!(result.is_ok());

    let result = client
        .execute_command("GET", &["large_key".to_string()])
        .await;
    assert!(result.is_ok());
    if let Ok(ResponseValue::BulkString(Some(value))) = result {
        assert_eq!(value, large_value);
    }

    // Clean up
    client
        .execute_command("DEL", &["large_key".to_string()])
        .await
        .unwrap();
    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore = "requires running server"]
async fn test_cli_client_special_characters() {
    let mut client = create_test_client();

    client.connect().await.expect("Failed to connect to server");

    // Test with special characters
    let special_value = "Hello\nWorld\r\n\t\"quoted\"\\backslash";
    let result = client
        .execute_command(
            "SET",
            &["special_key".to_string(), special_value.to_string()],
        )
        .await;
    assert!(result.is_ok());

    let result = client
        .execute_command("GET", &["special_key".to_string()])
        .await;
    assert!(result.is_ok());
    if let Ok(ResponseValue::BulkString(Some(value))) = result {
        assert_eq!(value, special_value);
    }

    // Clean up
    client
        .execute_command("DEL", &["special_key".to_string()])
        .await
        .unwrap();
    client.disconnect().await.unwrap();
}

#[tokio::test]
#[ignore = "requires running server"]
async fn test_cli_client_unicode() {
    let mut client = create_test_client();

    client.connect().await.expect("Failed to connect to server");

    // Test with Unicode characters
    let unicode_value = "Hello 世界 🌍 Здравствуй мир";
    let result = client
        .execute_command(
            "SET",
            &["unicode_key".to_string(), unicode_value.to_string()],
        )
        .await;
    assert!(result.is_ok());

    let result = client
        .execute_command("GET", &["unicode_key".to_string()])
        .await;
    assert!(result.is_ok());
    if let Ok(ResponseValue::BulkString(Some(value))) = result {
        assert_eq!(value, unicode_value);
    }

    // Clean up
    client
        .execute_command("DEL", &["unicode_key".to_string()])
        .await
        .unwrap();
    client.disconnect().await.unwrap();
}

// Test command parser functionality
mod command_parser_tests {
    use rustypotato::cli::interactive::CommandParser;

    #[test]
    fn test_parse_simple_command() {
        let result = CommandParser::parse("SET key value").unwrap();
        assert_eq!(result, vec!["SET", "key", "value"]);
    }

    #[test]
    fn test_parse_quoted_string() {
        let result = CommandParser::parse("SET key \"hello world\"").unwrap();
        assert_eq!(result, vec!["SET", "key", "hello world"]);
    }

    #[test]
    fn test_parse_escaped_quotes() {
        let result = CommandParser::parse("SET key \"hello \\\"world\\\"\"").unwrap();
        assert_eq!(result, vec!["SET", "key", "hello \"world\""]);
    }

    #[test]
    fn test_parse_multiple_spaces() {
        let result = CommandParser::parse("SET    key     value").unwrap();
        assert_eq!(result, vec!["SET", "key", "value"]);
    }

    #[test]
    fn test_parse_tabs() {
        let result = CommandParser::parse("SET\tkey\tvalue").unwrap();
        assert_eq!(result, vec!["SET", "key", "value"]);
    }

    #[test]
    fn test_parse_empty_string() {
        let result = CommandParser::parse("").unwrap();
        assert_eq!(result, Vec::<String>::new());
    }

    #[test]
    fn test_parse_whitespace_only() {
        let result = CommandParser::parse("   \t  ").unwrap();
        assert_eq!(result, Vec::<String>::new());
    }

    #[test]
    fn test_parse_unterminated_quote() {
        let result = CommandParser::parse("SET key \"unterminated");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_escaped_backslash() {
        let result = CommandParser::parse("SET key \"path\\\\to\\\\file\"").unwrap();
        assert_eq!(result, vec!["SET", "key", "path\\to\\file"]);
    }

    #[test]
    fn test_parse_empty_quotes() {
        let result = CommandParser::parse("SET key \"\"").unwrap();
        assert_eq!(result, vec!["SET", "key", ""]);
    }

    #[test]
    fn test_parse_mixed_quotes_and_spaces() {
        let result = CommandParser::parse("SET \"key with spaces\" value").unwrap();
        assert_eq!(result, vec!["SET", "key with spaces", "value"]);
    }

    #[test]
    fn test_parse_special_characters() {
        let result = CommandParser::parse("SET key \"line1\\nline2\\ttab\"").unwrap();
        assert_eq!(result, vec!["SET", "key", "line1\nline2\ttab"]);
    }
}

// Performance tests
#[cfg(test)]
mod performance_tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    #[ignore = "performance test"]
    async fn test_cli_client_performance() {
        let mut client = create_test_client();
        client.connect().await.expect("Failed to connect to server");

        let start = Instant::now();
        let num_operations = 1000;

        for i in 0..num_operations {
            let key = format!("perf_key_{}", i);
            let value = format!("perf_value_{}", i);

            client
                .execute_command("SET", &[key.clone(), value])
                .await
                .unwrap();
            client.execute_command("GET", &[key.clone()]).await.unwrap();
            client.execute_command("DEL", &[key]).await.unwrap();
        }

        let duration = start.elapsed();
        let ops_per_sec = (num_operations * 3) as f64 / duration.as_secs_f64();

        println!(
            "Performed {} operations in {:?} ({:.2} ops/sec)",
            num_operations * 3,
            duration,
            ops_per_sec
        );

        // Should be able to perform at least 100 ops/sec
        assert!(
            ops_per_sec > 100.0,
            "Performance too low: {:.2} ops/sec",
            ops_per_sec
        );

        client.disconnect().await.unwrap();
    }
}
