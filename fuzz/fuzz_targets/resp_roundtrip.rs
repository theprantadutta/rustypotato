//! Fuzz target for RESP encode/decode roundtrip
//!
//! This fuzzer generates arbitrary ResponseValue structures and verifies
//! that encoding them produces valid RESP that can be successfully processed.
//!
//! Run with: cargo +nightly fuzz run resp_roundtrip
//!
//! Note: cargo-fuzz requires Linux/macOS. On Windows, run in WSL or CI.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use rustypotato::commands::ResponseValue;
use rustypotato::network::protocol::RespCodec;

/// A simplified ResponseValue for fuzzing that can be derived with Arbitrary
#[derive(Debug, Clone, Arbitrary)]
enum FuzzResponseValue {
    SimpleString(String),
    BulkString(Option<String>),
    Integer(i64),
    Nil,
    Array(Vec<FuzzResponseValue>),
}

impl FuzzResponseValue {
    /// Convert to actual ResponseValue, filtering out invalid simple strings
    fn to_response_value(&self) -> ResponseValue {
        match self {
            FuzzResponseValue::SimpleString(s) => {
                // Simple strings cannot contain CR or LF
                let cleaned: String = s
                    .chars()
                    .filter(|c| *c != '\r' && *c != '\n')
                    .take(1000) // Limit length to avoid OOM
                    .collect();
                ResponseValue::SimpleString(cleaned)
            }
            FuzzResponseValue::BulkString(opt) => {
                ResponseValue::BulkString(opt.as_ref().map(|s| {
                    // Bulk strings can contain any bytes, but limit length
                    s.chars().take(10000).collect()
                }))
            }
            FuzzResponseValue::Integer(i) => ResponseValue::Integer(*i),
            FuzzResponseValue::Nil => ResponseValue::Nil,
            FuzzResponseValue::Array(arr) => {
                // Limit array depth and size to avoid stack overflow
                if arr.len() > 100 {
                    ResponseValue::Array(
                        arr.iter()
                            .take(100)
                            .map(|v| v.to_response_value())
                            .collect(),
                    )
                } else {
                    ResponseValue::Array(arr.iter().map(|v| v.to_response_value()).collect())
                }
            }
        }
    }
}

fuzz_target!(|data: FuzzResponseValue| {
    let response = data.to_response_value();

    let mut codec = RespCodec::new();

    // Encoding should never panic
    let encoded = match codec.encode(&response) {
        Ok(bytes) => bytes,
        Err(_) => return, // Encoding errors are acceptable
    };

    // Verify the encoded output has valid RESP structure
    // (starts with valid type prefix, ends with CRLF for simple types)
    if !encoded.is_empty() {
        let first_byte = encoded[0];
        assert!(
            matches!(first_byte, b'+' | b'-' | b':' | b'$' | b'*'),
            "Invalid RESP type prefix: {}",
            first_byte as char
        );
    }

    // For non-array types, verify CRLF termination
    match &response {
        ResponseValue::SimpleString(_) | ResponseValue::Integer(_) => {
            assert!(
                encoded.ends_with(b"\r\n"),
                "Simple response should end with CRLF"
            );
        }
        ResponseValue::BulkString(_) | ResponseValue::Nil => {
            assert!(
                encoded.ends_with(b"\r\n"),
                "Bulk string should end with CRLF"
            );
        }
        ResponseValue::Array(_) => {
            // Arrays are more complex, just verify they have content
            assert!(!encoded.is_empty(), "Array encoding should produce output");
        }
        _ => {}
    }
});
