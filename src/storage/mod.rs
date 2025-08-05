//! Storage layer implementations
//!
//! This module provides the core storage functionality including
//! in-memory storage, persistence, and expiration management.

pub mod expiration;
pub mod memory;
pub mod persistence;

pub use expiration::{ExpirationEntry, ExpirationManager};
pub use memory::{MemoryStore, StoredValue, ValueType};
pub use persistence::{AofWriter, PersistenceManager};

/// Storage operation results
pub type StorageResult<T> = Result<T, crate::error::RustyPotatoError>;
