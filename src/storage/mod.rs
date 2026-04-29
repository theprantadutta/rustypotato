//! Storage layer implementations
//!
//! This module provides the core storage functionality including
//! in-memory storage, persistence, and expiration management.

pub mod expiration;
pub mod memory;
pub mod persistence;

pub use expiration::{ExpirationEntry, ExpirationManager};
pub use memory::{
    ExpireFlag, MemoryStore, SetExistence, SetOutcome, SetTtlOption, StoredValue, ValueType,
};
pub use persistence::{replay_aof_file, snapshot_to_frames, PersistenceManager};

/// Storage operation results
pub type StorageResult<T> = Result<T, crate::error::RustyPotatoError>;
