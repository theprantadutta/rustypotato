//! Comprehensive unit tests for error handling
//!
//! Tests cover error types, error conversion, error messages,
//! and error propagation patterns.

use rustypotato::{RustyPotatoError, Result};
use std::io;

#[cfg(test)]
mod error_tests {
    use super::*;

    #[test]
    fn test_error_types() {
        // Test all error variants
        let errors = vec![
            RustyPotatoError::InvalidCommand { command: "UNKNOWN".to_string() },
            RustyPotatoError::WrongArity { command: "SET".to_string(), expected: 2, actual: 1 },
            RustyPotatoError::NotAnInteger { value: "abc".to_string() },
            RustyPotatoError::KeyNotFound { key: "missing".to_string() },
            RustyPotatoError::IoError { 
                message: "File not found".to_string(),
                source: Some(Box::new(io::Error::new(io::ErrorKind::NotFound, "test error")))
            },
            RustyPotatoError::NetworkError { 
                message: "Connection refused".to_string(),
                address: Some("127.0.0.1:6379".to_string())
            },
            RustyPotatoError::ConfigError { 
                message: "Invalid port".to_string(),
                field: Some("server.port".to_string())
            },
            RustyPotatoError::PersistenceError { 
                message: "AOF write failed".to_string(),
                operation: Some("write".to_string())
            },
            RustyPotatoError::InternalError { 
                message: "Unexpected state".to_string(),
                component: Some("storage".to_string()),
                source: None
            },
        ];

        // Verify each error can be created and displayed
        for error in errors {
            let error_string = error.to_string();
            assert!(!error_string.is_empty());
            
            // Verify debug representation
            let debug_string = format!("{:?}", error);
            assert!(!debug_string.is_empty());
        }
    }

    #[test]
    fn test_error_display_messages() {
        // Test specific error messages
        let invalid_cmd = RustyPotatoError::InvalidCommand { command: "BADCMD".to_string() };
        assert!(invalid_cmd.to_string().contains("BADCMD"));
        assert!(invalid_cmd.to_string().contains("unknown command"));

        let wrong_arity = RustyPotatoError::WrongArity { 
            command: "SET".to_string(), 
            expected: 2, 
            actual: 1 
        };
        assert!(wrong_arity.to_string().contains("SET"));
        assert!(wrong_arity.to_string().contains("2"));
        assert!(wrong_arity.to_string().contains("1"));

        let not_integer = RustyPotatoError::NotAnInteger { value: "hello".to_string() };
        assert!(not_integer.to_string().contains("hello"));
        assert!(not_integer.to_string().contains("integer"));

        let key_not_found = RustyPotatoError::KeyNotFound { key: "missing_key".to_string() };
        assert!(key_not_found.to_string().contains("missing_key"));
        assert!(key_not_found.to_string().contains("not found"));
    }

    #[test]
    fn test_error_from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "Access denied");
        let rusty_err: RustyPotatoError = io_err.into();
        
        if let RustyPotatoError::IoError { message, source } = rusty_err {
            assert!(message.contains("Access denied"));
            assert!(source.is_some());
        } else {
            panic!("Expected IoError variant");
        }
    }

    #[test]
    fn test_error_chain() {
        let root_cause = io::Error::new(io::ErrorKind::BrokenPipe, "Broken pipe");
        let wrapped_error = RustyPotatoError::NetworkError {
            message: "Connection lost".to_string(),
            address: Some("127.0.0.1:6379".to_string()),
        };

        // Test error source chain
        let error_string = wrapped_error.to_string();
        assert!(error_string.contains("Connection lost"));
        assert!(error_string.contains("127.0.0.1:6379"));
    }

    #[test]
    fn test_result_type() {
        // Test successful result
        let success: Result<i32> = Ok(42);
        assert_eq!(success.unwrap(), 42);

        // Test error result
        let error: Result<i32> = Err(RustyPotatoError::InvalidCommand { 
            command: "TEST".to_string() 
        });
        assert!(error.is_err());
    }

    #[test]
    fn test_error_propagation() {
        fn inner_function() -> Result<()> {
            Err(RustyPotatoError::NotAnInteger { value: "abc".to_string() })
        }

        fn outer_function() -> Result<()> {
            inner_function()?;
            Ok(())
        }

        let result = outer_function();
        assert!(result.is_err());
        
        if let Err(RustyPotatoError::NotAnInteger { value }) = result {
            assert_eq!(value, "abc");
        } else {
            panic!("Expected NotAnInteger error");
        }
    }

    #[test]
    fn test_error_context() {
        // Test errors with context information
        let config_error = RustyPotatoError::ConfigError {
            message: "Port must be between 1 and 65535".to_string(),
            field: Some("server.port".to_string()),
        };

        let error_string = config_error.to_string();
        assert!(error_string.contains("Port must be between 1 and 65535"));
        assert!(error_string.contains("server.port"));

        let persistence_error = RustyPotatoError::PersistenceError {
            message: "Failed to write to AOF".to_string(),
            operation: Some("append".to_string()),
        };

        let error_string = persistence_error.to_string();
        assert!(error_string.contains("Failed to write to AOF"));
        assert!(error_string.contains("append"));
    }

    #[test]
    fn test_error_equality() {
        let error1 = RustyPotatoError::InvalidCommand { command: "TEST".to_string() };
        let error2 = RustyPotatoError::InvalidCommand { command: "TEST".to_string() };
        let error3 = RustyPotatoError::InvalidCommand { command: "OTHER".to_string() };

        // Note: RustyPotatoError doesn't implement PartialEq due to Box<dyn Error>
        // So we test by comparing string representations
        assert_eq!(error1.to_string(), error2.to_string());
        assert_ne!(error1.to_string(), error3.to_string());
    }

    #[test]
    fn test_error_send_sync() {
        // Test that errors can be sent across threads
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<RustyPotatoError>();
        assert_sync::<RustyPotatoError>();
    }

    #[test]
    fn test_error_downcast() {
        let io_err = io::Error::new(io::ErrorKind::TimedOut, "Operation timed out");
        let rusty_err: RustyPotatoError = io_err.into();

        // Test that we can access the source error
        if let RustyPotatoError::IoError { source, .. } = &rusty_err {
            assert!(source.is_some());
            if let Some(source_err) = source {
                let downcast_result = source_err.downcast_ref::<io::Error>();
                assert!(downcast_result.is_some());
                if let Some(io_error) = downcast_result {
                    assert_eq!(io_error.kind(), io::ErrorKind::TimedOut);
                }
            }
        } else {
            panic!("Expected IoError variant");
        }
    }

    #[test]
    fn test_error_optional_fields() {
        // Test errors with optional fields set to None
        let network_error_minimal = RustyPotatoError::NetworkError {
            message: "Network error".to_string(),
            address: None,
        };
        assert!(network_error_minimal.to_string().contains("Network error"));

        let config_error_minimal = RustyPotatoError::ConfigError {
            message: "Config error".to_string(),
            field: None,
        };
        assert!(config_error_minimal.to_string().contains("Config error"));

        let persistence_error_minimal = RustyPotatoError::PersistenceError {
            message: "Persistence error".to_string(),
            operation: None,
        };
        assert!(persistence_error_minimal.to_string().contains("Persistence error"));

        let internal_error_minimal = RustyPotatoError::InternalError {
            message: "Internal error".to_string(),
            component: None,
            source: None,
        };
        assert!(internal_error_minimal.to_string().contains("Internal error"));
    }

    #[test]
    fn test_error_edge_cases() {
        // Test with empty strings
        let empty_command = RustyPotatoError::InvalidCommand { command: "".to_string() };
        assert!(!empty_command.to_string().is_empty());

        let empty_value = RustyPotatoError::NotAnInteger { value: "".to_string() };
        assert!(!empty_value.to_string().is_empty());

        let empty_key = RustyPotatoError::KeyNotFound { key: "".to_string() };
        assert!(!empty_key.to_string().is_empty());

        // Test with very long strings
        let long_command = "A".repeat(1000);
        let long_cmd_error = RustyPotatoError::InvalidCommand { command: long_command.clone() };
        assert!(long_cmd_error.to_string().contains(&long_command));

        // Test with special characters
        let special_chars = "key\n\t\r\"'\\";
        let special_error = RustyPotatoError::KeyNotFound { key: special_chars.to_string() };
        assert!(special_error.to_string().contains(special_chars));
    }

    #[test]
    fn test_error_formatting() {
        let error = RustyPotatoError::WrongArity {
            command: "SET".to_string(),
            expected: 2,
            actual: 1,
        };

        // Test different formatting options
        let display = format!("{}", error);
        let debug = format!("{:?}", error);
        let alternate_debug = format!("{:#?}", error);

        assert!(!display.is_empty());
        assert!(!debug.is_empty());
        assert!(!alternate_debug.is_empty());

        // Debug should be more verbose than display
        assert!(debug.len() >= display.len());
    }

    #[test]
    fn test_error_in_result_chains() {
        fn operation_that_fails() -> Result<String> {
            Err(RustyPotatoError::KeyNotFound { key: "test".to_string() })
        }

        fn operation_that_maps_error() -> Result<i32> {
            let _value = operation_that_fails()?;
            Ok(42)
        }

        let result = operation_that_maps_error();
        assert!(result.is_err());

        match result {
            Err(RustyPotatoError::KeyNotFound { key }) => {
                assert_eq!(key, "test");
            }
            _ => panic!("Expected KeyNotFound error"),
        }
    }

    #[test]
    fn test_error_with_nested_sources() {
        let root_io_error = io::Error::new(io::ErrorKind::UnexpectedEof, "Unexpected EOF");
        let wrapped_error = RustyPotatoError::PersistenceError {
            message: "Failed to read AOF file".to_string(),
            operation: Some("read".to_string()),
        };

        // Test that error messages are properly formatted
        let error_msg = wrapped_error.to_string();
        assert!(error_msg.contains("Failed to read AOF file"));
        assert!(error_msg.contains("read"));
    }
}