//! Command implementations and execution framework
//! 
//! This module contains all command implementations for RustyPotato,
//! including basic operations (SET, GET, DEL), TTL management, and
//! atomic operations.

pub mod registry;
pub mod string;
pub mod ttl;
pub mod atomic;

#[cfg(test)]
mod integration_tests;

pub use registry::{Command, CommandRegistry, CommandResult, ParsedCommand};
pub use string::{SetCommand, GetCommand, DelCommand, ExistsCommand};

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