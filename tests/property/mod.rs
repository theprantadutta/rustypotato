//! Property-based tests using proptest
//!
//! These tests verify invariants and properties that should hold for all inputs,
//! providing stronger guarantees than example-based tests.

pub mod storage_properties;
pub mod protocol_properties;
pub mod command_properties;
