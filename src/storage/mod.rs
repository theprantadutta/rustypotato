//! Storage layer implementations
//! 
//! This module provides the core storage functionality including
//! in-memory storage, persistence, and expiration management.

pub mod memory;
pub mod persistence;
pub mod expiration;

pub use memory::{MemoryStore, StoredValue, ValueType};
pub use persistence::{AofWriter, PersistenceManager};
pub use expiration::{ExpirationManager, ExpirationEntry};

/// Storage operation results
pub type StorageResult<T> = Result<T, crate::error::RustyPotatoError>;