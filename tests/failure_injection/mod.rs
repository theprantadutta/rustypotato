//! Failure injection tests for RustyPotato
//!
//! These tests verify the system's resilience under various failure conditions:
//! - Network failures (connection drops, partial data, timeouts)
//! - Persistence failures (disk errors, corrupted files)
//! - Resource exhaustion (memory pressure, connection limits)

pub mod dos_hardening;
pub mod network_failures;
pub mod persistence_failures;
pub mod resource_exhaustion;
