//! Redis RESP protocol implementation
//!
//! This module implements the Redis Serialization Protocol (RESP) for
//! encoding responses and decoding client commands.

use crate::commands::{ParsedCommand, ResponseValue};
use crate::error::{Result, RustyPotatoError};
use bytes::{Buf, BufMut, BytesMut};
use std::io::Cursor;

/// Redis RESP protocol codec
#[derive(Debug, Clone)]
pub struct RespCodec {
    buffer: BytesMut,
}

impl RespCodec {
    /// Create a new RESP codec
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::with_capacity(4096),
        }
    }

    /// Encode a ResponseValue into RESP format
    pub fn encode(&mut self, value: &ResponseValue) -> Result<Vec<u8>> {
        self.buffer.clear();
        self.encode_value(value)?;
        Ok(self.buffer.to_vec())
    }

    /// Decode RESP data into a ParsedCommand
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<ParsedCommand>> {
        self.buffer.extend_from_slice(data);

        if let Some(command) = self.try_parse_command()? {
            Ok(Some(command))
        } else {
            Ok(None) // Need more data
        }
    }

    /// Encode a single ResponseValue
    fn encode_value(&mut self, value: &ResponseValue) -> Result<()> {
        match value {
            ResponseValue::SimpleString(s) => {
                self.buffer.put_u8(b'+');
                self.buffer.extend_from_slice(s.as_bytes());
                self.buffer.extend_from_slice(b"\r\n");
            }
            ResponseValue::BulkString(Some(s)) => {
                self.buffer.put_u8(b'$');
                self.buffer
                    .extend_from_slice(s.len().to_string().as_bytes());
                self.buffer.extend_from_slice(b"\r\n");
                self.buffer.extend_from_slice(s.as_bytes());
                self.buffer.extend_from_slice(b"\r\n");
            }
            ResponseValue::BulkString(None) | ResponseValue::Nil => {
                self.buffer.extend_from_slice(b"$-1\r\n");
            }
            ResponseValue::Integer(i) => {
                self.buffer.put_u8(b':');
                self.buffer.extend_from_slice(i.to_string().as_bytes());
                self.buffer.extend_from_slice(b"\r\n");
            }
            ResponseValue::Array(arr) => {
                self.buffer.put_u8(b'*');
                self.buffer
                    .extend_from_slice(arr.len().to_string().as_bytes());
                self.buffer.extend_from_slice(b"\r\n");
                for item in arr {
                    self.encode_value(item)?;
                }
            }
        }
        Ok(())
    }

    /// Try to parse a complete command from the buffer
    fn try_parse_command(&mut self) -> Result<Option<ParsedCommand>> {
        if self.buffer.is_empty() {
            return Ok(None);
        }

        let mut cursor = Cursor::new(&self.buffer[..]);

        // Parse the command array
        match self.parse_resp_value(&mut cursor)? {
            Some(RespValue::Array(elements)) => {
                let consumed = cursor.position() as usize;

                // Convert RESP array to command
                let mut args = Vec::new();
                for element in elements {
                    match element {
                        RespValue::BulkString(Some(s)) => args.push(s),
                        RespValue::SimpleString(s) => args.push(s),
                        _ => {
                            return Err(RustyPotatoError::ProtocolError {
                                message: "Command arguments must be strings".to_string(),
                                command: None,
                                source: None,
                            })
                        }
                    }
                }

                if args.is_empty() {
                    return Err(RustyPotatoError::ProtocolError {
                        message: "Empty command array".to_string(),
                        command: None,
                        source: None,
                    });
                }

                let command_name = args.remove(0).to_uppercase();
                // Use a placeholder UUID for now - this will be set by the connection handler
                let parsed_command = ParsedCommand::new(command_name, args, uuid::Uuid::nil());

                // Remove consumed bytes from buffer
                self.buffer.advance(consumed);

                Ok(Some(parsed_command))
            }
            Some(_) => Err(RustyPotatoError::ProtocolError {
                message: "Expected array for command".to_string(),
                command: None,
                source: None,
            }),
            None => Ok(None), // Need more data
        }
    }

    /// Parse a single RESP value from the cursor
    fn parse_resp_value(&self, cursor: &mut Cursor<&[u8]>) -> Result<Option<RespValue>> {
        if !cursor.has_remaining() {
            return Ok(None);
        }

        let type_byte = cursor.get_u8();

        match type_byte {
            b'+' => self.parse_simple_string(cursor),
            b'-' => self.parse_error(cursor),
            b':' => self.parse_integer(cursor),
            b'$' => self.parse_bulk_string(cursor),
            b'*' => self.parse_array(cursor),
            _ => Err(RustyPotatoError::ProtocolError {
                message: format!("Unknown RESP type: {}", type_byte as char),
                command: None,
                source: None,
            }),
        }
    }

    /// Parse a simple string (+OK\r\n)
    fn parse_simple_string(&self, cursor: &mut Cursor<&[u8]>) -> Result<Option<RespValue>> {
        if let Some(line) = self.read_line(cursor)? {
            Ok(Some(RespValue::SimpleString(line)))
        } else {
            Ok(None)
        }
    }

    /// Parse an error (-ERR message\r\n)
    fn parse_error(&self, cursor: &mut Cursor<&[u8]>) -> Result<Option<RespValue>> {
        if let Some(line) = self.read_line(cursor)? {
            Ok(Some(RespValue::Error(line)))
        } else {
            Ok(None)
        }
    }

    /// Parse an integer (:123\r\n)
    fn parse_integer(&self, cursor: &mut Cursor<&[u8]>) -> Result<Option<RespValue>> {
        if let Some(line) = self.read_line(cursor)? {
            let value = line
                .parse::<i64>()
                .map_err(|_| RustyPotatoError::ProtocolError {
                    message: format!("Invalid integer: {line}"),
                    command: None,
                    source: None,
                })?;
            Ok(Some(RespValue::Integer(value)))
        } else {
            Ok(None)
        }
    }

    /// Parse a bulk string ($5\r\nhello\r\n or $-1\r\n for null)
    fn parse_bulk_string(&self, cursor: &mut Cursor<&[u8]>) -> Result<Option<RespValue>> {
        if let Some(length_str) = self.read_line(cursor)? {
            let length =
                length_str
                    .parse::<i32>()
                    .map_err(|_| RustyPotatoError::ProtocolError {
                        message: format!("Invalid bulk string length: {length_str}"),
                        command: None,
                        source: None,
                    })?;

            if length == -1 {
                return Ok(Some(RespValue::BulkString(None)));
            }

            if length < 0 {
                return Err(RustyPotatoError::ProtocolError {
                    message: format!("Invalid bulk string length: {length}"),
                    command: None,
                    source: None,
                });
            }

            let length = length as usize;

            // Check if we have enough data for the string + \r\n
            if cursor.remaining() < length + 2 {
                return Ok(None); // Need more data
            }

            let mut string_data = vec![0u8; length];
            cursor.copy_to_slice(&mut string_data);

            // Verify \r\n terminator
            if cursor.remaining() < 2 || cursor.get_u8() != b'\r' || cursor.get_u8() != b'\n' {
                return Err(RustyPotatoError::ProtocolError {
                    message: "Missing \\r\\n terminator for bulk string".to_string(),
                    command: None,
                    source: None,
                });
            }

            let string =
                String::from_utf8(string_data).map_err(|_| RustyPotatoError::ProtocolError {
                    message: "Invalid UTF-8 in bulk string".to_string(),
                    command: None,
                    source: None,
                })?;

            Ok(Some(RespValue::BulkString(Some(string))))
        } else {
            Ok(None)
        }
    }

    /// Parse an array (*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n)
    fn parse_array(&self, cursor: &mut Cursor<&[u8]>) -> Result<Option<RespValue>> {
        if let Some(length_str) = self.read_line(cursor)? {
            let length =
                length_str
                    .parse::<i32>()
                    .map_err(|_| RustyPotatoError::ProtocolError {
                        message: format!("Invalid array length: {length_str}"),
                        command: None,
                        source: None,
                    })?;

            if length == -1 {
                return Ok(Some(RespValue::Array(vec![])));
            }

            if length < 0 {
                return Err(RustyPotatoError::ProtocolError {
                    message: format!("Invalid array length: {length}"),
                    command: None,
                    source: None,
                });
            }

            let length = length as usize;
            let mut elements = Vec::with_capacity(length);

            for _ in 0..length {
                if let Some(element) = self.parse_resp_value(cursor)? {
                    elements.push(element);
                } else {
                    return Ok(None); // Need more data
                }
            }

            Ok(Some(RespValue::Array(elements)))
        } else {
            Ok(None)
        }
    }

    /// Read a line terminated by \r\n
    fn read_line(&self, cursor: &mut Cursor<&[u8]>) -> Result<Option<String>> {
        let start_pos = cursor.position() as usize;
        let data = cursor.get_ref();

        // Look for \r\n
        for i in start_pos..data.len().saturating_sub(1) {
            if data[i] == b'\r' && data[i + 1] == b'\n' {
                let line_data = &data[start_pos..i];
                cursor.set_position((i + 2) as u64);

                let line = String::from_utf8(line_data.to_vec()).map_err(|_| {
                    RustyPotatoError::ProtocolError {
                        message: "Invalid UTF-8 in line".to_string(),
                        command: None,
                        source: None,
                    }
                })?;

                return Ok(Some(line));
            }
        }

        Ok(None) // No complete line found
    }
}

impl Default for RespCodec {
    fn default() -> Self {
        Self::new()
    }
}

/// RESP value types for internal parsing
#[derive(Debug, Clone, PartialEq)]
pub enum RespValue {
    SimpleString(String),
    BulkString(Option<String>),
    Integer(i64),
    Array(Vec<RespValue>),
    Error(String),
}

/// Encode an error response
pub fn encode_error(message: &str) -> Vec<u8> {
    let mut buffer = BytesMut::new();
    buffer.put_u8(b'-');
    buffer.extend_from_slice(message.as_bytes());
    buffer.extend_from_slice(b"\r\n");
    buffer.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::ResponseValue;

    #[test]
    fn test_encode_simple_string() {
        let mut codec = RespCodec::new();
        let value = ResponseValue::SimpleString("OK".to_string());
        let result = codec.encode(&value).unwrap();
        assert_eq!(result, b"+OK\r\n");
    }

    #[test]
    fn test_encode_bulk_string() {
        let mut codec = RespCodec::new();
        let value = ResponseValue::BulkString(Some("hello".to_string()));
        let result = codec.encode(&value).unwrap();
        assert_eq!(result, b"$5\r\nhello\r\n");
    }

    #[test]
    fn test_encode_bulk_string_nil() {
        let mut codec = RespCodec::new();
        let value = ResponseValue::BulkString(None);
        let result = codec.encode(&value).unwrap();
        assert_eq!(result, b"$-1\r\n");
    }

    #[test]
    fn test_encode_nil() {
        let mut codec = RespCodec::new();
        let value = ResponseValue::Nil;
        let result = codec.encode(&value).unwrap();
        assert_eq!(result, b"$-1\r\n");
    }

    #[test]
    fn test_encode_integer() {
        let mut codec = RespCodec::new();
        let value = ResponseValue::Integer(42);
        let result = codec.encode(&value).unwrap();
        assert_eq!(result, b":42\r\n");
    }

    #[test]
    fn test_encode_negative_integer() {
        let mut codec = RespCodec::new();
        let value = ResponseValue::Integer(-123);
        let result = codec.encode(&value).unwrap();
        assert_eq!(result, b":-123\r\n");
    }

    #[test]
    fn test_encode_array() {
        let mut codec = RespCodec::new();
        let value = ResponseValue::Array(vec![
            ResponseValue::SimpleString("OK".to_string()),
            ResponseValue::Integer(42),
        ]);
        let result = codec.encode(&value).unwrap();
        assert_eq!(result, b"*2\r\n+OK\r\n:42\r\n");
    }

    #[test]
    fn test_encode_empty_array() {
        let mut codec = RespCodec::new();
        let value = ResponseValue::Array(vec![]);
        let result = codec.encode(&value).unwrap();
        assert_eq!(result, b"*0\r\n");
    }

    #[test]
    fn test_encode_nested_array() {
        let mut codec = RespCodec::new();
        let value = ResponseValue::Array(vec![
            ResponseValue::Array(vec![
                ResponseValue::SimpleString("foo".to_string()),
                ResponseValue::SimpleString("bar".to_string()),
            ]),
            ResponseValue::Integer(123),
        ]);
        let result = codec.encode(&value).unwrap();
        assert_eq!(result, b"*2\r\n*2\r\n+foo\r\n+bar\r\n:123\r\n");
    }

    #[test]
    fn test_decode_simple_command() {
        let mut codec = RespCodec::new();
        let data = b"*2\r\n$3\r\nGET\r\n$3\r\nkey\r\n";
        let result = codec.decode(data).unwrap();

        assert!(result.is_some());
        let cmd = result.unwrap();
        assert_eq!(cmd.name, "GET");
        assert_eq!(cmd.args, vec!["key"]);
    }

    #[test]
    fn test_decode_set_command() {
        let mut codec = RespCodec::new();
        let data = b"*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n";
        let result = codec.decode(data).unwrap();

        assert!(result.is_some());
        let cmd = result.unwrap();
        assert_eq!(cmd.name, "SET");
        assert_eq!(cmd.args, vec!["key", "value"]);
    }

    #[test]
    fn test_decode_incomplete_command() {
        let mut codec = RespCodec::new();
        let data = b"*2\r\n$3\r\nGET\r\n$3\r\nk"; // Incomplete
        let result = codec.decode(data).unwrap();

        assert!(result.is_none()); // Need more data
    }

    #[test]
    fn test_decode_multiple_commands() {
        let mut codec = RespCodec::new();

        // First command
        let data1 = b"*2\r\n$3\r\nGET\r\n$3\r\nkey\r\n";
        let result1 = codec.decode(data1).unwrap();
        assert!(result1.is_some());
        let cmd1 = result1.unwrap();
        assert_eq!(cmd1.name, "GET");

        // Second command
        let data2 = b"*3\r\n$3\r\nSET\r\n$4\r\nkey2\r\n$6\r\nvalue2\r\n";
        let result2 = codec.decode(data2).unwrap();
        assert!(result2.is_some());
        let cmd2 = result2.unwrap();
        assert_eq!(cmd2.name, "SET");
        assert_eq!(cmd2.args, vec!["key2", "value2"]);
    }

    #[test]
    fn test_decode_command_with_empty_string() {
        let mut codec = RespCodec::new();
        let data = b"*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$0\r\n\r\n";
        let result = codec.decode(data).unwrap();

        assert!(result.is_some());
        let cmd = result.unwrap();
        assert_eq!(cmd.name, "SET");
        assert_eq!(cmd.args, vec!["key", ""]);
    }

    #[test]
    fn test_decode_invalid_array_length() {
        let mut codec = RespCodec::new();
        let data = b"*abc\r\n$3\r\nGET\r\n";
        let result = codec.decode(data);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid array length"));
    }

    #[test]
    fn test_decode_invalid_bulk_string_length() {
        let mut codec = RespCodec::new();
        let data = b"*2\r\n$3\r\nGET\r\n$abc\r\nkey\r\n";
        let result = codec.decode(data);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid bulk string length"));
    }

    #[test]
    fn test_decode_negative_bulk_string_length() {
        let mut codec = RespCodec::new();
        let data = b"*2\r\n$3\r\nGET\r\n$-5\r\n";
        let result = codec.decode(data);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid bulk string length"));
    }

    #[test]
    fn test_decode_missing_crlf() {
        let mut codec = RespCodec::new();
        let data = b"*2\r\n$3\r\nGET$3\r\nkey\r\n"; // Missing \r\n after GET
        let result = codec.decode(data);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Missing \\r\\n terminator"));
    }

    #[test]
    fn test_decode_invalid_utf8() {
        let mut codec = RespCodec::new();
        let mut data = b"*2\r\n$3\r\nGET\r\n$3\r\n".to_vec();
        data.extend_from_slice(&[0xFF, 0xFE, 0xFD]); // Invalid UTF-8
        data.extend_from_slice(b"\r\n");

        let result = codec.decode(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid UTF-8"));
    }

    #[test]
    fn test_decode_empty_command_array() {
        let mut codec = RespCodec::new();
        let data = b"*0\r\n";
        let result = codec.decode(data);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Empty command array"));
    }

    #[test]
    fn test_decode_non_string_command_args() {
        let mut codec = RespCodec::new();
        // This would be parsed as RespValue but contains an integer instead of string
        // We need to test this at the RespValue parsing level
        let data = b"*2\r\n$3\r\nGET\r\n:42\r\n";
        let result = codec.decode(data);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Command arguments must be strings"));
    }

    #[test]
    fn test_decode_unknown_resp_type() {
        let mut codec = RespCodec::new();
        let data = b"@invalid\r\n"; // @ is not a valid RESP type
        let result = codec.decode(data);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unknown RESP type"));
    }

    #[test]
    fn test_parse_simple_string() {
        let codec = RespCodec::new();
        let data: &[u8] = b"+OK\r\n";
        let mut cursor = Cursor::new(data);
        cursor.advance(1); // Skip the '+'

        let result = codec.parse_simple_string(&mut cursor).unwrap();
        assert_eq!(result, Some(RespValue::SimpleString("OK".to_string())));
    }

    #[test]
    fn test_parse_error() {
        let codec = RespCodec::new();
        let data: &[u8] = b"-ERR message\r\n";
        let mut cursor = Cursor::new(data);
        cursor.advance(1); // Skip the '-'

        let result = codec.parse_error(&mut cursor).unwrap();
        assert_eq!(result, Some(RespValue::Error("ERR message".to_string())));
    }

    #[test]
    fn test_parse_integer() {
        let codec = RespCodec::new();
        let data: &[u8] = b":42\r\n";
        let mut cursor = Cursor::new(data);
        cursor.advance(1); // Skip the ':'

        let result = codec.parse_integer(&mut cursor).unwrap();
        assert_eq!(result, Some(RespValue::Integer(42)));
    }

    #[test]
    fn test_parse_integer_negative() {
        let codec = RespCodec::new();
        let data: &[u8] = b":-123\r\n";
        let mut cursor = Cursor::new(data);
        cursor.advance(1); // Skip the ':'

        let result = codec.parse_integer(&mut cursor).unwrap();
        assert_eq!(result, Some(RespValue::Integer(-123)));
    }

    #[test]
    fn test_parse_bulk_string() {
        let codec = RespCodec::new();
        let data: &[u8] = b"$5\r\nhello\r\n";
        let mut cursor = Cursor::new(data);
        cursor.advance(1); // Skip the '$'

        let result = codec.parse_bulk_string(&mut cursor).unwrap();
        assert_eq!(
            result,
            Some(RespValue::BulkString(Some("hello".to_string())))
        );
    }

    #[test]
    fn test_parse_bulk_string_null() {
        let codec = RespCodec::new();
        let data: &[u8] = b"$-1\r\n";
        let mut cursor = Cursor::new(data);
        cursor.advance(1); // Skip the '$'

        let result = codec.parse_bulk_string(&mut cursor).unwrap();
        assert_eq!(result, Some(RespValue::BulkString(None)));
    }

    #[test]
    fn test_parse_bulk_string_empty() {
        let codec = RespCodec::new();
        let data: &[u8] = b"$0\r\n\r\n";
        let mut cursor = Cursor::new(data);
        cursor.advance(1); // Skip the '$'

        let result = codec.parse_bulk_string(&mut cursor).unwrap();
        assert_eq!(result, Some(RespValue::BulkString(Some("".to_string()))));
    }

    #[test]
    fn test_parse_array() {
        let codec = RespCodec::new();
        let data: &[u8] = b"*2\r\n+OK\r\n:42\r\n";
        let mut cursor = Cursor::new(data);
        cursor.advance(1); // Skip the '*'

        let result = codec.parse_array(&mut cursor).unwrap();
        assert_eq!(
            result,
            Some(RespValue::Array(vec![
                RespValue::SimpleString("OK".to_string()),
                RespValue::Integer(42),
            ]))
        );
    }

    #[test]
    fn test_parse_array_empty() {
        let codec = RespCodec::new();
        let data: &[u8] = b"*0\r\n";
        let mut cursor = Cursor::new(data);
        cursor.advance(1); // Skip the '*'

        let result = codec.parse_array(&mut cursor).unwrap();
        assert_eq!(result, Some(RespValue::Array(vec![])));
    }

    #[test]
    fn test_parse_array_null() {
        let codec = RespCodec::new();
        let data: &[u8] = b"*-1\r\n";
        let mut cursor = Cursor::new(data);
        cursor.advance(1); // Skip the '*'

        let result = codec.parse_array(&mut cursor).unwrap();
        assert_eq!(result, Some(RespValue::Array(vec![])));
    }

    #[test]
    fn test_read_line() {
        let codec = RespCodec::new();
        let data: &[u8] = b"hello world\r\nmore data";
        let mut cursor = Cursor::new(data);

        let result = codec.read_line(&mut cursor).unwrap();
        assert_eq!(result, Some("hello world".to_string()));
        assert_eq!(cursor.position(), 13); // After \r\n
    }

    #[test]
    fn test_read_line_incomplete() {
        let codec = RespCodec::new();
        let data: &[u8] = b"hello world";
        let mut cursor = Cursor::new(data);

        let result = codec.read_line(&mut cursor).unwrap();
        assert_eq!(result, None); // No \r\n found
    }

    #[test]
    fn test_read_line_only_cr() {
        let codec = RespCodec::new();
        let data: &[u8] = b"hello\r";
        let mut cursor = Cursor::new(data);

        let result = codec.read_line(&mut cursor).unwrap();
        assert_eq!(result, None); // No complete \r\n
    }

    #[test]
    fn test_encode_error_function() {
        let result = encode_error("Test error message");
        assert_eq!(result, b"-Test error message\r\n");
    }

    #[test]
    fn test_encode_error_empty() {
        let result = encode_error("");
        assert_eq!(result, b"-\r\n");
    }

    #[test]
    fn test_codec_buffer_reuse() {
        let mut codec = RespCodec::new();

        // First encoding
        let value1 = ResponseValue::SimpleString("first".to_string());
        let result1 = codec.encode(&value1).unwrap();
        assert_eq!(result1, b"+first\r\n");

        // Second encoding should reuse buffer
        let value2 = ResponseValue::SimpleString("second".to_string());
        let result2 = codec.encode(&value2).unwrap();
        assert_eq!(result2, b"+second\r\n");
    }

    #[test]
    fn test_decode_partial_then_complete() {
        let mut codec = RespCodec::new();

        // Send partial data
        let partial = b"*2\r\n$3\r\nGET\r\n$3\r\nk";
        let result1 = codec.decode(partial).unwrap();
        assert!(result1.is_none());

        // Send remaining data
        let remaining = b"ey\r\n";
        let result2 = codec.decode(remaining).unwrap();
        assert!(result2.is_some());

        let cmd = result2.unwrap();
        assert_eq!(cmd.name, "GET");
        assert_eq!(cmd.args, vec!["key"]);
    }

    #[test]
    fn test_decode_command_case_insensitive() {
        let mut codec = RespCodec::new();
        let data = b"*2\r\n$3\r\nget\r\n$3\r\nkey\r\n";
        let result = codec.decode(data).unwrap();

        assert!(result.is_some());
        let cmd = result.unwrap();
        assert_eq!(cmd.name, "GET"); // Should be uppercase
        assert_eq!(cmd.args, vec!["key"]);
    }

    #[test]
    fn test_resp_value_equality() {
        let val1 = RespValue::SimpleString("test".to_string());
        let val2 = RespValue::SimpleString("test".to_string());
        let val3 = RespValue::SimpleString("different".to_string());

        assert_eq!(val1, val2);
        assert_ne!(val1, val3);
    }

    #[test]
    fn test_resp_value_debug() {
        let val = RespValue::Array(vec![
            RespValue::SimpleString("test".to_string()),
            RespValue::Integer(42),
        ]);

        let debug_str = format!("{val:?}");
        assert!(debug_str.contains("Array"));
        assert!(debug_str.contains("SimpleString"));
        assert!(debug_str.contains("Integer"));
    }
}
