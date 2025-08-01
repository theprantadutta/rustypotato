//! Redis RESP protocol implementation

/// Redis RESP protocol codec
pub struct RespCodec {
    // Implementation will be added in later tasks
}

impl RespCodec {
    /// Create a new RESP codec
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for RespCodec {
    fn default() -> Self {
        Self::new()
    }
}

/// RESP value types
#[derive(Debug, Clone, PartialEq)]
pub enum RespValue {
    SimpleString(String),
    BulkString(Option<String>),
    Integer(i64),
    Array(Vec<RespValue>),
    Error(String),
}