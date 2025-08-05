//! Command implementations and execution framework
//!
//! This module contains all command implementations for RustyPotato,
//! including basic operations (SET, GET, DEL), TTL management, and
//! atomic operations.

pub mod atomic;
pub mod registry;
pub mod string;
pub mod ttl;

#[cfg(test)]
mod integration_tests;

pub use atomic::{DecrCommand, IncrCommand};
pub use registry::{Command, CommandRegistry, CommandResult, ParsedCommand};
pub use string::{DelCommand, ExistsCommand, GetCommand, SetCommand};
pub use ttl::{ExpireCommand, TtlCommand};

/// Command execution result types
#[derive(Debug, Clone, PartialEq)]
pub enum ResponseValue {
    SimpleString(String),
    BulkString(Option<String>),
    Integer(i64),
    Array(Vec<ResponseValue>),
    Nil,
}

/// Command arity specification
#[derive(Debug, Clone, PartialEq)]
pub enum CommandArity {
    Fixed(usize),
    Range(usize, usize),
    AtLeast(usize),
}
