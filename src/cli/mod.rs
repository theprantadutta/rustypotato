//! Command Line Interface for RustyPotato
//! 
//! This module provides the CLI client for interacting with RustyPotato
//! servers, supporting both single-command and interactive modes.

pub mod client;
pub mod commands;
pub mod interactive;

pub use client::CliClient;
pub use commands::{Cli, Commands};