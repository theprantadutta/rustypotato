//! Command implementations and execution framework
//!
//! This module contains all command implementations for RustyPotato,
//! including basic operations (SET, GET, DEL), TTL management, and
//! atomic operations.

pub mod atomic;
pub mod auth;
pub mod hash;
pub mod list;
pub mod pubsub;
pub mod registry;
pub mod set;
pub mod string;
pub mod ttl;

#[cfg(test)]
mod integration_tests;

pub use atomic::{DecrCommand, IncrCommand};
pub use auth::{AuthCommand, is_auth_exempt};
pub use hash::{HdelCommand, HgetCommand, HgetallCommand, HexistsCommand, HsetCommand};
pub use list::{LlenCommand, LpopCommand, LpushCommand, LrangeCommand, RpopCommand, RpushCommand};
pub use set::{
    SaddCommand, ScardCommand, SismemberCommand, SmembersCommand, SpopCommand, SrandmemberCommand,
    SremCommand,
};
pub use pubsub::{
    PsubscribeCommand, PublishCommand, PunsubscribeCommand, SubscribeCommand, UnsubscribeCommand,
};
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
