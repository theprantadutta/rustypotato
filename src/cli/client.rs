//! CLI client implementation

use crate::commands::ResponseValue;
use crate::error::{Result, RustyPotatoError};
use crate::network::protocol::RespCodec;
use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, error, info, warn};

/// CLI client for connecting to RustyPotato server
pub struct CliClient {
    stream: Option<TcpStream>,
    codec: RespCodec,
    address: String,
}

impl CliClient {
    /// Create a new CLI client
    pub fn new() -> Self {
        Self {
            stream: None,
            codec: RespCodec::new(),
            address: "127.0.0.1:6379".to_string(),
        }
    }

    /// Create a new CLI client with custom address
    pub fn with_address(address: String) -> Self {
        Self {
            stream: None,
            codec: RespCodec::new(),
            address,
        }
    }

    /// Connect to the RustyPotato server
    pub async fn connect(&mut self) -> Result<()> {
        info!("Connecting to RustyPotato server at {}", self.address);

        match TcpStream::connect(&self.address).await {
            Ok(stream) => {
                self.stream = Some(stream);
                info!("Successfully connected to server");
                Ok(())
            }
            Err(e) => {
                error!("Failed to connect to server: {}", e);
                Err(RustyPotatoError::ConnectionError {
                    message: format!("Failed to connect to {}: {}", self.address, e),
                    connection_id: None,
                    source: Some(Box::new(e)),
                })
            }
        }
    }

    /// Disconnect from the server
    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(mut stream) = self.stream.take() {
            debug!("Disconnecting from server");
            if let Err(e) = stream.shutdown().await {
                warn!("Error during disconnect: {}", e);
            }
        }
        Ok(())
    }

    /// Check if client is connected
    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    /// Execute a command and return the response
    pub async fn execute_command(
        &mut self,
        command: &str,
        args: &[String],
    ) -> Result<ResponseValue> {
        if self.stream.is_none() {
            return Err(RustyPotatoError::ConnectionError {
                message: "Not connected to server".to_string(),
                connection_id: None,
                source: None,
            });
        }

        // Build command array for RESP protocol
        let mut command_parts = vec![command.to_string()];
        command_parts.extend_from_slice(args);

        debug!("Executing command: {} with args: {:?}", command, args);

        // Encode command as RESP array
        let encoded_command = self.encode_command_array(&command_parts)?;

        // Send command
        let stream = self.stream.as_mut().unwrap();
        if let Err(e) = stream.write_all(&encoded_command).await {
            error!("Failed to send command: {}", e);
            return Err(RustyPotatoError::NetworkError {
                message: format!("Failed to send command: {e}"),
                source: Some(Box::new(e)),
                connection_id: None,
            });
        }

        // Read response
        self.read_response().await
    }

    /// Encode command parts as RESP array
    pub fn encode_command_array(&mut self, parts: &[String]) -> Result<Vec<u8>> {
        // Create ResponseValue array from command parts
        let array_elements: Vec<ResponseValue> = parts
            .iter()
            .map(|part| ResponseValue::BulkString(Some(part.clone())))
            .collect();

        let response_value = ResponseValue::Array(array_elements);

        self.codec
            .encode(&response_value)
            .map_err(|e| RustyPotatoError::ProtocolError {
                message: format!("Failed to encode command: {e}"),
                command: Some(parts.join(" ")),
                source: Some(Box::new(e)),
            })
    }

    /// Read and parse response from server
    async fn read_response(&mut self) -> Result<ResponseValue> {
        let mut buffer = BytesMut::with_capacity(4096);

        loop {
            // Read data from stream
            let mut temp_buffer = [0u8; 1024];
            let bytes_read = {
                let stream = self.stream.as_mut().unwrap();
                match stream.read(&mut temp_buffer).await {
                    Ok(0) => {
                        return Err(RustyPotatoError::ConnectionError {
                            message: "Connection closed by server".to_string(),
                            connection_id: None,
                            source: None,
                        });
                    }
                    Ok(n) => n,
                    Err(e) => {
                        error!("Failed to read response: {}", e);
                        return Err(RustyPotatoError::NetworkError {
                            message: format!("Failed to read response: {e}"),
                            source: Some(Box::new(e)),
                            connection_id: None,
                        });
                    }
                }
            };

            buffer.extend_from_slice(&temp_buffer[..bytes_read]);

            // Try to parse response
            match self.parse_response(&buffer) {
                Ok(Some(response)) => {
                    debug!("Received response: {:?}", response);
                    return Ok(response);
                }
                Ok(None) => {
                    // Need more data, continue reading
                    continue;
                }
                Err(e) => {
                    error!("Failed to parse response: {}", e);
                    return Err(e);
                }
            }
        }
    }

    /// Parse RESP response from buffer
    fn parse_response(&self, buffer: &[u8]) -> Result<Option<ResponseValue>> {
        if buffer.is_empty() {
            return Ok(None);
        }

        let mut cursor = std::io::Cursor::new(buffer);

        match self.parse_resp_value(&mut cursor)? {
            Some(resp_value) => Ok(Some(Self::convert_resp_to_response(resp_value))),
            None => Ok(None),
        }
    }

    /// Parse a single RESP value from cursor
    fn parse_resp_value(
        &self,
        cursor: &mut std::io::Cursor<&[u8]>,
    ) -> Result<Option<crate::network::protocol::RespValue>> {
        if cursor.position() >= cursor.get_ref().len() as u64 {
            return Ok(None);
        }

        let mut type_byte = [0u8; 1];
        if std::io::Read::read_exact(cursor, &mut type_byte).is_err() {
            return Ok(None);
        }

        match type_byte[0] {
            b'+' => self.parse_simple_string(cursor),
            b'-' => self.parse_error_response(cursor),
            b':' => self.parse_integer_response(cursor),
            b'$' => self.parse_bulk_string_response(cursor),
            b'*' => self.parse_array_response(cursor),
            _ => Err(RustyPotatoError::ProtocolError {
                message: format!("Unknown RESP type: {}", type_byte[0] as char),
                command: None,
                source: None,
            }),
        }
    }

    /// Parse simple string response (+OK\r\n)
    fn parse_simple_string(
        &self,
        cursor: &mut std::io::Cursor<&[u8]>,
    ) -> Result<Option<crate::network::protocol::RespValue>> {
        if let Some(line) = self.read_line(cursor)? {
            Ok(Some(crate::network::protocol::RespValue::SimpleString(
                line,
            )))
        } else {
            Ok(None)
        }
    }

    /// Parse error response (-ERR message\r\n)
    fn parse_error_response(
        &self,
        cursor: &mut std::io::Cursor<&[u8]>,
    ) -> Result<Option<crate::network::protocol::RespValue>> {
        if let Some(line) = self.read_line(cursor)? {
            Ok(Some(crate::network::protocol::RespValue::Error(line)))
        } else {
            Ok(None)
        }
    }

    /// Parse integer response (:123\r\n)
    fn parse_integer_response(
        &self,
        cursor: &mut std::io::Cursor<&[u8]>,
    ) -> Result<Option<crate::network::protocol::RespValue>> {
        if let Some(line) = self.read_line(cursor)? {
            let value = line
                .parse::<i64>()
                .map_err(|_| RustyPotatoError::ProtocolError {
                    message: format!("Invalid integer: {line}"),
                    command: None,
                    source: None,
                })?;
            Ok(Some(crate::network::protocol::RespValue::Integer(value)))
        } else {
            Ok(None)
        }
    }

    /// Parse bulk string response ($5\r\nhello\r\n or $-1\r\n for null)
    fn parse_bulk_string_response(
        &self,
        cursor: &mut std::io::Cursor<&[u8]>,
    ) -> Result<Option<crate::network::protocol::RespValue>> {
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
                return Ok(Some(crate::network::protocol::RespValue::BulkString(None)));
            }

            if length < 0 {
                return Err(RustyPotatoError::ProtocolError {
                    message: format!("Invalid bulk string length: {length}"),
                    command: None,
                    source: None,
                });
            }

            let length = length as usize;

            // Check if we have enough data
            let remaining_data = &cursor.get_ref()[(cursor.position() as usize)..];
            if remaining_data.len() < length + 2 {
                return Ok(None); // Need more data
            }

            let mut string_data = vec![0u8; length];
            std::io::Read::read_exact(cursor, &mut string_data).map_err(|_| {
                RustyPotatoError::ProtocolError {
                    message: "Failed to read bulk string data".to_string(),
                    command: None,
                    source: None,
                }
            })?;

            // Verify \r\n terminator
            let mut terminator = [0u8; 2];
            std::io::Read::read_exact(cursor, &mut terminator).map_err(|_| {
                RustyPotatoError::ProtocolError {
                    message: "Failed to read bulk string terminator".to_string(),
                    command: None,
                    source: None,
                }
            })?;

            if terminator != [b'\r', b'\n'] {
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

            Ok(Some(crate::network::protocol::RespValue::BulkString(Some(
                string,
            ))))
        } else {
            Ok(None)
        }
    }

    /// Parse array response (*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n)
    fn parse_array_response(
        &self,
        cursor: &mut std::io::Cursor<&[u8]>,
    ) -> Result<Option<crate::network::protocol::RespValue>> {
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
                return Ok(Some(crate::network::protocol::RespValue::Array(vec![])));
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

            Ok(Some(crate::network::protocol::RespValue::Array(elements)))
        } else {
            Ok(None)
        }
    }

    /// Read a line terminated by \r\n
    fn read_line(&self, cursor: &mut std::io::Cursor<&[u8]>) -> Result<Option<String>> {
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

    /// Convert RespValue to ResponseValue
    fn convert_resp_to_response(
        resp_value: crate::network::protocol::RespValue,
    ) -> ResponseValue {
        match resp_value {
            crate::network::protocol::RespValue::SimpleString(s) => ResponseValue::SimpleString(s),
            crate::network::protocol::RespValue::BulkString(s) => ResponseValue::BulkString(s),
            crate::network::protocol::RespValue::Integer(i) => ResponseValue::Integer(i),
            crate::network::protocol::RespValue::Array(arr) => {
                let converted_arr: Vec<ResponseValue> = arr
                    .into_iter()
                    .map(Self::convert_resp_to_response)
                    .collect();
                ResponseValue::Array(converted_arr)
            }
            crate::network::protocol::RespValue::Error(msg) => {
                // Convert error to a simple string for display
                ResponseValue::SimpleString(format!("ERR {msg}"))
            }
        }
    }

    /// Format response for display
    pub fn format_response(response: &ResponseValue) -> String {
        match response {
            ResponseValue::SimpleString(s) => s.clone(),
            ResponseValue::BulkString(Some(s)) => s.clone(),
            ResponseValue::BulkString(None) | ResponseValue::Nil => "(nil)".to_string(),
            ResponseValue::Integer(i) => format!("(integer) {i}"),
            ResponseValue::Array(arr) => {
                if arr.is_empty() {
                    "(empty array)".to_string()
                } else {
                    let formatted_items: Vec<String> = arr
                        .iter()
                        .enumerate()
                        .map(|(i, item)| format!("{}) {}", i + 1, Self::format_response(item)))
                        .collect();
                    formatted_items.join("\n")
                }
            }
        }
    }
}

impl Default for CliClient {
    fn default() -> Self {
        Self::new()
    }
}
