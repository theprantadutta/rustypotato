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
    DbsizeCommand, EchoCommand, FlushdbCommand, InfoCommand, PingCommand, TypeCommand,
};
pub use string::{DelCommand, ExistsCommand, GetCommand, SetCommand};
pub use ttl::{ExpireCommand, TtlCommand};

/// Command execution result types.
#[derive(Debug, Clone, PartialEq)]
pub enum ResponseValue {
    SimpleString(String),
    BulkString(Option<String>),
    Integer(i64),
    Array(Vec<ResponseValue>),
    Nil,
}

impl ResponseValue {
    /// Construct a non-nil bulk string from anything that can be turned
    /// into a `String`. Migration helper for the upcoming binary-safe
    /// rework where this type will become `Option<Bytes>` — written this
    /// way today so call sites can switch to the helper now and the
    /// underlying type swap becomes a one-line change later.
    pub fn bulk(s: impl Into<String>) -> Self {
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
