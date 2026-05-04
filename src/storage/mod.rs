//! Storage layer implementations
//!
//! Provides the core storage primitives:
//! - `MemoryStore` (DashMap-backed, lazy expiration on read).
//! - `PersistenceManager` for the AOF.
//!
//! Expiration is handled inline by `MemoryStore` — every read path
//! checks `is_expired()` and lazily evicts. Active sweep is exposed
//! as `MemoryStore::cleanup_expired()` for callers that want to do
//! periodic GC.

pub mod memory;
pub mod persistence;

pub use memory::{
    ExpireFlag, MemoryStore, SetExistence, SetOutcome, SetTtlOption, StoredValue, ValueType,
};
pub use persistence::{replay_aof_file, snapshot_to_frames, PersistenceManager};

/// Storage operation results
pub type StorageResult<T> = Result<T, crate::error::RustyPotatoError>;
