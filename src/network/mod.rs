//! Network layer for TCP server and client connections
//!
//! This module handles all network communication including the TCP server,
//! Redis RESP protocol implementation, and connection management.

pub mod connection;
pub mod protocol;
pub mod server;

pub use connection::{ClientConnection, ConnectionPool};
pub use protocol::{encode_error, RespCodec, RespValue};
pub use server::TcpServer;
