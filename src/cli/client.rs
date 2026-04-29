//! CLI client implementation

use crate::commands::ResponseValue;
use crate::error::{Result, RustyPotatoError};
use crate::network::protocol::RespCodec;
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
            .map(|part| ResponseValue::bulk(part.clone()))
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

    /// Read and parse response from server using the canonical
    /// `RespCodec`. The previous implementation hand-rolled a parallel
    /// RESP parser (~200 lines) which is now collapsed into the same
    /// codec the server uses for command parsing.
    async fn read_response(&mut self) -> Result<ResponseValue> {
        let mut chunk = [0u8; 1024];
        loop {
            // Try to drain any frame already buffered in the codec
            // before issuing another read — handles the pipelined case
            // where one syscall returned multiple responses.
            if let Some(value) = self.codec.decode_resp_value(&[])? {
                debug!("Received response: {:?}", value);
                return Ok(Self::convert_resp_to_response(value));
            }

            let stream = self.stream.as_mut().unwrap();
            let n = match stream.read(&mut chunk).await {
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
            };

            match self.codec.decode_resp_value(&chunk[..n])? {
                Some(value) => {
                    debug!("Received response: {:?}", value);
                    return Ok(Self::convert_resp_to_response(value));
                }
                None => continue, // need more data
            }
        }
    }

    /// Convert RespValue to ResponseValue
    fn convert_resp_to_response(resp_value: crate::network::protocol::RespValue) -> ResponseValue {
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
