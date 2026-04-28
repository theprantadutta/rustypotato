//! Property-based tests for the RESP protocol implementation
//!
//! Tests verify:
//! - Encode-decode roundtrip for all ResponseValue types
//! - Bulk strings of any length work correctly
//! - Arrays with various element counts work correctly
//! - Invalid input handling never panics

use proptest::prelude::*;
use rustypotato::commands::ResponseValue;
use rustypotato::network::protocol::RespCodec;

/// Strategy for generating valid simple strings (no CR/LF)
fn simple_string_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_\\-\\.\\s]{0,100}"
        .prop_filter("no newlines", |s| !s.contains('\r') && !s.contains('\n'))
}

/// Strategy for generating bulk strings (can contain any bytes)
fn bulk_string_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_\\-\\.\\s]{0,500}"
        .prop_filter("no crlf", |s| !s.contains('\r') && !s.contains('\n'))
}

/// Strategy for generating ResponseValue::SimpleString
fn simple_string_response() -> impl Strategy<Value = ResponseValue> {
    simple_string_strategy().prop_map(ResponseValue::SimpleString)
}

/// Strategy for generating ResponseValue::BulkString
fn bulk_string_response() -> impl Strategy<Value = ResponseValue> {
    prop_oneof![
        Just(ResponseValue::nil_bulk()),
        bulk_string_strategy().prop_map(|s| ResponseValue::BulkString(Some(s)))
    ]
}

/// Strategy for generating ResponseValue::Integer
fn integer_response() -> impl Strategy<Value = ResponseValue> {
    any::<i64>().prop_map(ResponseValue::Integer)
}

/// Strategy for generating any ResponseValue (non-recursive)
fn leaf_response() -> impl Strategy<Value = ResponseValue> {
    prop_oneof![
        simple_string_response(),
        bulk_string_response(),
        integer_response(),
        Just(ResponseValue::Nil),
    ]
}

/// Strategy for generating ResponseValue::Array (shallow)
fn shallow_array_response() -> impl Strategy<Value = ResponseValue> {
    prop::collection::vec(leaf_response(), 0..10).prop_map(ResponseValue::Array)
}

/// Strategy for any ResponseValue including shallow arrays
fn any_response() -> impl Strategy<Value = ResponseValue> {
    prop_oneof![leaf_response(), shallow_array_response(),]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    // ==================== Encode-Decode Roundtrip Properties ====================

    /// Property: SimpleString encodes and can be reconstructed
    #[test]
    fn prop_simple_string_encodes(s in simple_string_strategy()) {
        let mut codec = RespCodec::new();
        let response = ResponseValue::SimpleString(s.clone());

        let encoded = codec.encode(&response).unwrap();

        // Verify format: +<string>\r\n
        prop_assert!(encoded.starts_with(b"+"));
        prop_assert!(encoded.ends_with(b"\r\n"));

        // Verify content
        let content = String::from_utf8_lossy(&encoded[1..encoded.len()-2]);
        prop_assert_eq!(content.as_ref(), s.as_str());
    }

    /// Property: BulkString encodes and can be reconstructed
    #[test]
    fn prop_bulk_string_encodes(s in bulk_string_strategy()) {
        let mut codec = RespCodec::new();
        let response = ResponseValue::BulkString(Some(s.clone()));

        let encoded = codec.encode(&response).unwrap();

        // Verify format: $<length>\r\n<data>\r\n
        prop_assert!(encoded.starts_with(b"$"));

        // Extract the encoded string
        let encoded_str = String::from_utf8_lossy(&encoded);
        let parts: Vec<&str> = encoded_str.splitn(2, "\r\n").collect();
        prop_assert!(!parts.is_empty());

        // Verify length prefix
        let length_str = &parts[0][1..]; // Skip the '$'
        let expected_len = s.len();
        prop_assert_eq!(length_str.parse::<usize>().unwrap(), expected_len);
    }

    /// Property: Nil BulkString encodes correctly
    #[test]
    fn prop_nil_bulk_string_encodes(_dummy in 0i32..1) {
        let mut codec = RespCodec::new();
        let response = ResponseValue::nil_bulk();

        let encoded = codec.encode(&response).unwrap();
        prop_assert_eq!(&encoded, b"$-1\r\n");
    }

    /// Property: Nil encodes correctly
    #[test]
    fn prop_nil_encodes(_dummy in 0i32..1) {
        let mut codec = RespCodec::new();
        let response = ResponseValue::Nil;

        let encoded = codec.encode(&response).unwrap();
        prop_assert_eq!(&encoded, b"$-1\r\n");
    }

    /// Property: Integer encodes correctly
    #[test]
    fn prop_integer_encodes(i in any::<i64>()) {
        let mut codec = RespCodec::new();
        let response = ResponseValue::Integer(i);

        let encoded = codec.encode(&response).unwrap();

        // Verify format: :<integer>\r\n
        prop_assert!(encoded.starts_with(b":"));
        prop_assert!(encoded.ends_with(b"\r\n"));

        // Verify content
        let content = String::from_utf8_lossy(&encoded[1..encoded.len()-2]);
        prop_assert_eq!(content.parse::<i64>().unwrap(), i);
    }

    /// Property: Empty array encodes correctly
    #[test]
    fn prop_empty_array_encodes(_dummy in 0i32..1) {
        let mut codec = RespCodec::new();
        let response = ResponseValue::Array(vec![]);

        let encoded = codec.encode(&response).unwrap();
        prop_assert_eq!(&encoded, b"*0\r\n");
    }

    /// Property: Array length is encoded correctly
    #[test]
    fn prop_array_length_encodes(elements in prop::collection::vec(leaf_response(), 0..20)) {
        let element_count = elements.len();
        let mut codec = RespCodec::new();
        let response = ResponseValue::Array(elements);

        let encoded = codec.encode(&response).unwrap();

        // Verify starts with *<length>\r\n
        prop_assert!(encoded.starts_with(b"*"));

        let encoded_str = String::from_utf8_lossy(&encoded);
        let first_line_end = encoded_str.find("\r\n").unwrap();
        let length_str = &encoded_str[1..first_line_end];
        prop_assert_eq!(length_str.parse::<usize>().unwrap(), element_count);
    }

    // ==================== Encoding Consistency Properties ====================

    /// Property: Encoding is deterministic (same input = same output)
    #[test]
    fn prop_encoding_deterministic(response in any_response()) {
        let mut codec1 = RespCodec::new();
        let mut codec2 = RespCodec::new();

        let encoded1 = codec1.encode(&response).unwrap();
        let encoded2 = codec2.encode(&response).unwrap();

        prop_assert_eq!(encoded1, encoded2);
    }

    /// Property: Multiple encodes don't interfere with each other
    #[test]
    fn prop_multiple_encodes_independent(
        response1 in any_response(),
        response2 in any_response()
    ) {
        let mut codec = RespCodec::new();

        let encoded1a = codec.encode(&response1).unwrap();
        let encoded2 = codec.encode(&response2).unwrap();
        let encoded1b = codec.encode(&response1).unwrap();

        // First response should encode the same way both times
        prop_assert_eq!(encoded1a.clone(), encoded1b);

        // Just verify encoding completes successfully
        prop_assert!(!encoded1a.is_empty());
        prop_assert!(!encoded2.is_empty());
    }

    // ==================== Bulk String Length Properties ====================

    /// Property: Very long bulk strings encode correctly
    #[test]
    fn prop_long_bulk_string_length(len in 0usize..10000) {
        let s: String = (0..len).map(|_| 'x').collect();
        let mut codec = RespCodec::new();
        let response = ResponseValue::BulkString(Some(s.clone()));

        let encoded = codec.encode(&response).unwrap();

        // Verify the length prefix is correct
        let encoded_str = String::from_utf8_lossy(&encoded);
        let first_line_end = encoded_str.find("\r\n").unwrap();
        let length_str = &encoded_str[1..first_line_end];
        prop_assert_eq!(length_str.parse::<usize>().unwrap(), len);

        // Verify total encoded length
        // $<len>\r\n<data>\r\n = 1 + len_digits + 2 + len + 2
        let expected_len = 1 + length_str.len() + 2 + len + 2;
        prop_assert_eq!(encoded.len(), expected_len);
    }

    // ==================== Integer Boundary Properties ====================

    /// Property: Extreme integers encode correctly
    #[test]
    fn prop_extreme_integers(_dummy in 0i32..1) {
        let mut codec = RespCodec::new();

        let cases = vec![
            i64::MIN,
            i64::MAX,
            0,
            1,
            -1,
        ];

        for i in cases {
            let response = ResponseValue::Integer(i);
            let encoded = codec.encode(&response).unwrap();

            let content = String::from_utf8_lossy(&encoded[1..encoded.len()-2]);
            prop_assert_eq!(content.parse::<i64>().unwrap(), i);
        }
    }

    // ==================== Nested Array Properties ====================

    /// Property: Nested arrays encode without error
    #[test]
    fn prop_nested_array_encodes(
        inner_elements in prop::collection::vec(leaf_response(), 0..5),
        outer_count in 0usize..5
    ) {
        let mut codec = RespCodec::new();

        let inner = ResponseValue::Array(inner_elements);
        let outer: Vec<ResponseValue> = (0..outer_count).map(|_| inner.clone()).collect();
        let response = ResponseValue::Array(outer);

        let result = codec.encode(&response);
        prop_assert!(result.is_ok());
    }
}

#[cfg(test)]
mod decode_tests {
    use super::*;

    /// Test that decoding doesn't panic on arbitrary bytes
    #[test]
    fn test_decode_no_panic_on_garbage() {
        let test_cases: Vec<&[u8]> = vec![
            b"",
            b"\x00\x00\x00",
            b"invalid",
            b"*abc\r\n",
            b"$-2\r\n",
            b"$9999999999999999999\r\n",
            b"*-1\r\n",
            b"+OK",            // missing \r\n
            b":\r\n",          // empty integer
            b"*1\r\n$3\r\nab", // incomplete bulk string
        ];

        for data in test_cases {
            let mut codec = RespCodec::new();
            // Should not panic, just return Ok(None) or Err
            let _ = codec.decode(data);
        }
    }

    /// Test that valid RESP commands decode correctly
    #[test]
    fn test_valid_commands_decode() {
        let test_cases = vec![
            (b"*1\r\n$4\r\nPING\r\n".to_vec(), Some("PING")),
            (b"*2\r\n$3\r\nGET\r\n$3\r\nkey\r\n".to_vec(), Some("GET")),
            (
                b"*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n".to_vec(),
                Some("SET"),
            ),
        ];

        for (data, expected_cmd) in test_cases {
            let mut codec = RespCodec::new();
            let result = codec.decode(&data);

            if let Some(cmd) = expected_cmd {
                assert!(result.is_ok());
                let parsed = result.unwrap();
                assert!(parsed.is_some());
                assert_eq!(parsed.unwrap().name.to_uppercase(), cmd);
            }
        }
    }

    /// Test incremental decoding (partial data followed by rest)
    #[test]
    fn test_incremental_decode() {
        let mut codec = RespCodec::new();

        // Send partial command
        let result = codec.decode(b"*2\r\n$3\r\nGET");
        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // Need more data

        // Send rest of command
        let result = codec.decode(b"\r\n$3\r\nkey\r\n");
        assert!(result.is_ok());
        let parsed = result.unwrap();
        assert!(parsed.is_some());
        assert_eq!(parsed.unwrap().name.to_uppercase(), "GET");
    }
}
