//! Command implementations and execution framework
//!
//! This module contains all command implementations for RustyPotato,
//! including basic operations (SET, GET, DEL), TTL management, and
//! atomic operations.

pub mod atomic;
pub mod hash;
pub mod registry;
pub mod server;
pub mod string;
pub mod ttl;

#[cfg(test)]
mod integration_tests;

pub use atomic::{DecrCommand, IncrCommand};
pub use hash::{HdelCommand, HexistsCommand, HgetCommand, HgetallCommand, HsetCommand};
pub use registry::{Command, CommandRegistry, CommandResult, ParsedCommand};
pub use server::{
    DbsizeCommand, EchoCommand, FlushdbCommand, InfoCommand, KeysCommand, PingCommand, TypeCommand,
};
pub use string::{DelCommand, ExistsCommand, GetCommand, SetCommand};
pub use ttl::{ExpireCommand, TtlCommand};

use bytes::Bytes;

/// Command execution result types.
///
/// `BulkString` carries `Bytes` (not `String`) so that arbitrary-byte
/// values flow through the RESP write path without being squeezed
/// through UTF-8. Construct via `ResponseValue::bulk(s)` (which accepts
/// `&str`, `String`, `Vec<u8>`, or `Bytes`) and `nil_bulk()` for `$-1`.
#[derive(Debug, Clone, PartialEq)]
pub enum ResponseValue {
    SimpleString(String),
    BulkString(Option<Bytes>),
    Integer(i64),
    Array(Vec<ResponseValue>),
    Nil,
}

impl ResponseValue {
    /// Construct a non-nil bulk string from anything convertible into
    /// `Bytes`. Accepts `&str`, `String`, `Vec<u8>`, `&[u8]`, `Bytes`.
    pub fn bulk(s: impl Into<Bytes>) -> Self {
        Self::BulkString(Some(s.into()))
    }

    /// The RESP `$-1\r\n` nil response.
    pub fn nil_bulk() -> Self {
        Self::BulkString(None)
    }
}

/// Command arity specification
#[derive(Debug, Clone, PartialEq)]
pub enum CommandArity {
    Fixed(usize),
    Range(usize, usize),
    AtLeast(usize),
}
