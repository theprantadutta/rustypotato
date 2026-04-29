//! In-memory storage implementation using DashMap

use crate::error::{Result, RustyPotatoError};
use bytes::Bytes;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::time::Instant;

/// Value types supported by RustyPotato.
///
/// `String` carries `Bytes` (not `String`) so the store is binary-safe
/// for value payloads — Redis bulk strings are arbitrary octets, not
/// guaranteed-UTF-8. Hash field names and keys remain `String` for
/// now; binary-safe field/key support is a follow-up stage.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueType {
    String(Bytes),
    Integer(i64),
    Hash(HashMap<String, String>),
    List(VecDeque<String>),
}

impl ValueType {
    /// Convert value to string representation. Non-UTF-8 byte values
    /// are rendered via `from_utf8_lossy` (replacement-char) — this is
    /// purely for debug/display surfaces; the wire codec still emits
    /// the raw bytes.
    pub fn as_string(&self) -> String {
        match self {
            ValueType::String(s) => String::from_utf8_lossy(s).into_owned(),
            ValueType::Integer(i) => i.to_string(),
            ValueType::Hash(_) => "(hash)".to_string(),
            ValueType::List(_) => "(list)".to_string(),
        }
    }

    /// Get the raw bytes when this is a String value (zero-copy).
    pub fn as_bytes(&self) -> Option<&Bytes> {
        match self {
            ValueType::String(b) => Some(b),
            _ => None,
        }
    }

    /// Try to convert value to integer
    pub fn to_integer(&self) -> Result<i64> {
        match self {
            ValueType::Integer(i) => Ok(*i),
            ValueType::String(s) => {
                let text = std::str::from_utf8(s).map_err(|_| RustyPotatoError::NotAnInteger {
                    value: String::from_utf8_lossy(s).into_owned(),
                })?;
                text.parse::<i64>()
                    .map_err(|_| RustyPotatoError::NotAnInteger {
                        value: text.to_string(),
                    })
            }
            ValueType::Hash(_) => Err(RustyPotatoError::NotAnInteger {
                value: "hash".to_string(),
            }),
            ValueType::List(_) => Err(RustyPotatoError::NotAnInteger {
                value: "list".to_string(),
            }),
        }
    }

    /// Check if value is a string type
    pub fn is_string(&self) -> bool {
        matches!(self, ValueType::String(_))
    }

    /// Check if value is an integer type
    pub fn is_integer(&self) -> bool {
        matches!(self, ValueType::Integer(_))
    }

    /// Check if value is a hash type
    pub fn is_hash(&self) -> bool {
        matches!(self, ValueType::Hash(_))
    }

    /// Check if value is a list type
    pub fn is_list(&self) -> bool {
        matches!(self, ValueType::List(_))
    }

    /// Get the type name as a string
    pub fn type_name(&self) -> &'static str {
        match self {
            ValueType::String(_) => "string",
            ValueType::Integer(_) => "integer",
            ValueType::Hash(_) => "hash",
            ValueType::List(_) => "list",
        }
    }

    /// Get hash reference if this is a hash type
    pub fn as_hash(&self) -> Result<&HashMap<String, String>> {
        match self {
            ValueType::Hash(h) => Ok(h),
            _ => Err(RustyPotatoError::StorageError {
                message: format!("Value is not a hash, it's a {}", self.type_name()),
                operation: Some("as_hash".to_string()),
                source: None,
            }),
        }
    }

    /// Get mutable hash reference if this is a hash type
    pub fn as_hash_mut(&mut self) -> Result<&mut HashMap<String, String>> {
        match self {
            ValueType::Hash(h) => Ok(h),
            _ => Err(RustyPotatoError::StorageError {
                message: format!("Value is not a hash, it's a {}", self.type_name()),
                operation: Some("as_hash_mut".to_string()),
                source: None,
            }),
        }
    }

    /// Get list reference if this is a list type
    pub fn as_list(&self) -> Result<&VecDeque<String>> {
        match self {
            ValueType::List(l) => Ok(l),
            _ => Err(RustyPotatoError::StorageError {
                message: format!("Value is not a list, it's a {}", self.type_name()),
                operation: Some("as_list".to_string()),
                source: None,
            }),
        }
    }

    /// Get mutable list reference if this is a list type
    pub fn as_list_mut(&mut self) -> Result<&mut VecDeque<String>> {
        match self {
            ValueType::List(l) => Ok(l),
            _ => Err(RustyPotatoError::StorageError {
                message: format!("Value is not a list, it's a {}", self.type_name()),
                operation: Some("as_list_mut".to_string()),
                source: None,
            }),
        }
    }
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_string())
    }
}

impl From<String> for ValueType {
    fn from(s: String) -> Self {
        ValueType::String(Bytes::from(s))
    }
}

impl From<&str> for ValueType {
    fn from(s: &str) -> Self {
        ValueType::String(Bytes::copy_from_slice(s.as_bytes()))
    }
}

impl From<Bytes> for ValueType {
    fn from(b: Bytes) -> Self {
        ValueType::String(b)
    }
}

impl From<Vec<u8>> for ValueType {
    fn from(v: Vec<u8>) -> Self {
        ValueType::String(Bytes::from(v))
    }
}

impl From<i64> for ValueType {
    fn from(i: i64) -> Self {
        ValueType::Integer(i)
    }
}

impl From<i32> for ValueType {
    fn from(i: i32) -> Self {
        ValueType::Integer(i as i64)
    }
}

impl From<HashMap<String, String>> for ValueType {
    fn from(h: HashMap<String, String>) -> Self {
        ValueType::Hash(h)
    }
}

impl From<VecDeque<String>> for ValueType {
    fn from(l: VecDeque<String>) -> Self {
        ValueType::List(l)
    }
}

impl From<Vec<String>> for ValueType {
    fn from(v: Vec<String>) -> Self {
        ValueType::List(VecDeque::from(v))
    }
}

/// Metadata for complex data types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueMetadata {
    /// Size in bytes (estimated for complex types)
    pub size_bytes: usize,
    /// Number of elements (for hash/list types)
    pub element_count: usize,
    /// Last modification time (for complex types)
    pub last_modified: Option<std::time::SystemTime>,
    /// Custom metadata for extensions
    pub custom: HashMap<String, String>,
}

impl ValueMetadata {
    /// Create metadata for a value
    pub fn for_value(value: &ValueType) -> Self {
        let (size_bytes, element_count) = match value {
            ValueType::String(s) => (s.len(), 1),
            ValueType::Integer(_) => (8, 1), // i64 is 8 bytes
            ValueType::Hash(h) => {
                let size = h.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>();
                (size, h.len())
            }
            ValueType::List(l) => {
                let size = l.iter().map(|s| s.len()).sum::<usize>();
                (size, l.len())
            }
        };

        Self {
            size_bytes,
            element_count,
            last_modified: Some(std::time::SystemTime::now()),
            custom: HashMap::new(),
        }
    }

    /// Update metadata after value modification
    pub fn update_for_value(&mut self, value: &ValueType) {
        let (size_bytes, element_count) = match value {
            ValueType::String(s) => (s.len(), 1),
            ValueType::Integer(_) => (8, 1),
            ValueType::Hash(h) => {
                let size = h.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>();
                (size, h.len())
            }
            ValueType::List(l) => {
                let size = l.iter().map(|s| s.len()).sum::<usize>();
                (size, l.len())
            }
        };

        self.size_bytes = size_bytes;
        self.element_count = element_count;
        self.last_modified = Some(std::time::SystemTime::now());
    }
}

/// Conditional flag for `EXPIRE`/`PEXPIRE`.
///
/// Mirrors the Redis 7 `EXPIRE … [NX|XX|GT|LT]` options. `Gt`/`Lt` are
/// compared against `+inf` when the key has no current TTL, matching
/// Redis (`GT` rejects, `LT` applies).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExpireFlag {
    #[default]
    None,
    /// `NX`: only set TTL if the key currently has no TTL.
    Nx,
    /// `XX`: only set TTL if the key currently has a TTL.
    Xx,
    /// `GT`: only set TTL if the new TTL is greater than the current one.
    Gt,
    /// `LT`: only set TTL if the new TTL is less than the current one.
    Lt,
}

/// Existence constraint for `SET` (Redis NX/XX semantics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SetExistence {
    /// Always overwrite (default `SET key value`).
    #[default]
    Always,
    /// Only set if the key does not currently exist (`NX`).
    OnlyIfNotExists,
    /// Only set if the key currently exists (`XX`).
    OnlyIfExists,
}

/// TTL handling for `SET`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetTtlOption {
    /// Clear any existing TTL (default `SET key value`).
    None,
    /// Apply this absolute deadline (caller resolves `EX`/`PX`/`EXAT`/`PXAT` → `Instant`).
    At(Instant),
    /// Preserve the key's current TTL on overwrite (`KEEPTTL`).
    KeepTtl,
}

/// Result of a `SET` with options.
///
/// `applied` distinguishes the NX/XX-rejected case from the success
/// case. `previous` carries the prior value for the `GET` option,
/// which Redis specifies must be returned even when the write itself
/// was skipped.
#[derive(Debug, Clone)]
pub struct SetOutcome {
    pub applied: bool,
    pub previous: Option<StoredValue>,
}

/// Stored value with metadata
#[derive(Debug, Clone)]
pub struct StoredValue {
    pub value: ValueType,
    pub created_at: Instant,
    pub last_accessed: Instant,
    pub expires_at: Option<Instant>,
    pub metadata: ValueMetadata,
}

/// High-performance concurrent in-memory store
#[derive(Debug)]
pub struct MemoryStore {
    data: DashMap<String, StoredValue>,
    expiration_index: DashMap<String, Instant>,
}

impl MemoryStore {
    /// Create a new memory store
    pub fn new() -> Self {
        Self {
            data: DashMap::new(),
            expiration_index: DashMap::new(),
        }
    }

    /// Get the number of keys in the store
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the store is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Set a key-value pair in the store
    pub async fn set<K, V>(&self, key: K, value: V) -> Result<()>
    where
        K: Into<String>,
        V: Into<ValueType>,
    {
        let key_string = key.into();
        let value_type = value.into();
        let stored_value = StoredValue::new(value_type);

        // Remove from expiration index if it exists
        self.expiration_index.remove(&key_string);

        // Insert the new value
        self.data.insert(key_string, stored_value);

        Ok(())
    }

    /// Apply a `SET key value [options]` atomically.
    ///
    /// The command-level options are evaluated under a single
    /// per-shard write lock so concurrent NX/XX/GET observers never see
    /// a partial state. Returns a `SetOutcome` describing whether the
    /// new value was written and what (if anything) was previously
    /// stored — the latter is needed for the `GET` option, which must
    /// return the prior value even when NX/XX caused the write to be
    /// skipped.
    pub async fn set_with_options<K, V>(
        &self,
        key: K,
        value: V,
        ttl: SetTtlOption,
        existence: SetExistence,
    ) -> Result<SetOutcome>
    where
        K: Into<String>,
        V: Into<ValueType>,
    {
        let key_string = key.into();
        let value_type = value.into();

        // Resolve KeepTtl now (if the key is gone or has no TTL the
        // option becomes a no-op equivalent to "no TTL").
        let resolved_ttl: Option<Instant> = match ttl {
            SetTtlOption::None => None,
            SetTtlOption::At(at) => Some(at),
            SetTtlOption::KeepTtl => self.data.get(&key_string).and_then(|e| {
                if e.is_expired() {
                    None
                } else {
                    e.expires_at
                }
            }),
        };

        let outcome = match self.data.entry(key_string.clone()) {
            dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                // Treat expired-but-not-yet-evicted as "missing" for
                // existence checks — Redis behaves this way: a key with
                // a passed TTL is observationally absent.
                let expired = entry.get().is_expired();
                let logically_present = !expired;

                let allowed = match existence {
                    SetExistence::Always => true,
                    SetExistence::OnlyIfNotExists => !logically_present,
                    SetExistence::OnlyIfExists => logically_present,
                };

                if !allowed {
                    // Surface the previous value (or None for expired)
                    // so the caller's GET option can still read it.
                    let previous = if logically_present {
                        Some(entry.get().clone())
                    } else {
                        None
                    };
                    SetOutcome {
                        applied: false,
                        previous,
                    }
                } else {
                    let previous = if logically_present {
                        Some(entry.get().clone())
                    } else {
                        None
                    };
                    let stored_value = match resolved_ttl {
                        Some(at) => StoredValue::new_with_expiration(value_type, at),
                        None => StoredValue::new(value_type),
                    };
                    if let Some(at) = resolved_ttl {
                        self.expiration_index.insert(key_string.clone(), at);
                    } else {
                        self.expiration_index.remove(&key_string);
                    }
                    entry.insert(stored_value);
                    SetOutcome {
                        applied: true,
                        previous,
                    }
                }
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => match existence {
                SetExistence::OnlyIfExists => SetOutcome {
                    applied: false,
                    previous: None,
                },
                _ => {
                    let stored_value = match resolved_ttl {
                        Some(at) => StoredValue::new_with_expiration(value_type, at),
                        None => StoredValue::new(value_type),
                    };
                    if let Some(at) = resolved_ttl {
                        self.expiration_index.insert(key_string.clone(), at);
                    }
                    entry.insert(stored_value);
                    SetOutcome {
                        applied: true,
                        previous: None,
                    }
                }
            },
        };

        Ok(outcome)
    }

    /// Set a key-value pair with TTL in seconds
    pub async fn set_with_ttl<K, V>(&self, key: K, value: V, ttl_seconds: u64) -> Result<()>
    where
        K: Into<String>,
        V: Into<ValueType>,
    {
        let key_string = key.into();
        let value_type = value.into();
        let stored_value = StoredValue::new_with_ttl(value_type, ttl_seconds);
        let expires_at = stored_value.expires_at.unwrap();

        // Update expiration index
        self.expiration_index.insert(key_string.clone(), expires_at);

        // Insert the new value
        self.data.insert(key_string, stored_value);

        Ok(())
    }

    /// Set a key-value pair with specific expiration time
    pub async fn set_with_expiration<K, V>(
        &self,
        key: K,
        value: V,
        expires_at: Instant,
    ) -> Result<()>
    where
        K: Into<String>,
        V: Into<ValueType>,
    {
        let key_string = key.into();
        let value_type = value.into();
        let stored_value = StoredValue::new_with_expiration(value_type, expires_at);

        // Update expiration index
        self.expiration_index.insert(key_string.clone(), expires_at);

        // Insert the new value
        self.data.insert(key_string, stored_value);

        Ok(())
    }

    /// Get a value from the store.
    ///
    /// Uses the read-side `data.get()` rather than `get_mut()`. The
    /// previous implementation grabbed a write lock on the per-shard
    /// DashMap entry just so it could call `entry.touch()`, serializing
    /// concurrent reads of the same shard against each other. That cost
    /// is real (was the single biggest unforced perf foot-gun in the
    /// review) and the touch served no purpose: nothing in the store
    /// reads `last_accessed` yet (LRU eviction lands in stage 10).
    ///
    /// When stage 10 wires up LRU, `last_accessed` becomes an
    /// `AtomicU64` updated lock-free here.
    pub fn get<K>(&self, key: K) -> Result<Option<StoredValue>>
    where
        K: AsRef<str>,
    {
        let key_str = key.as_ref();

        if let Some(entry) = self.data.get(key_str) {
            if entry.is_expired() {
                drop(entry);
                self.delete_sync(key_str)?;
                return Ok(None);
            }
            Ok(Some(entry.clone()))
        } else {
            Ok(None)
        }
    }

    /// Delete a key from the store
    pub async fn delete<K>(&self, key: K) -> Result<bool>
    where
        K: AsRef<str>,
    {
        let key_str = key.as_ref();

        // Check if key exists and is not expired first
        if let Some(entry) = self.data.get(key_str) {
            if entry.is_expired() {
                // Remove expired key automatically and return false (key didn't exist)
                drop(entry); // Release the lock before removing
                self.expiration_index.remove(key_str);
                self.data.remove(key_str);
                return Ok(false);
            }
        }

        // Remove from expiration index
        self.expiration_index.remove(key_str);

        // Remove from main data store
        let was_removed = self.data.remove(key_str).is_some();

        Ok(was_removed)
    }

    /// Internal synchronous delete method for use in sync contexts
    fn delete_sync<K>(&self, key: K) -> Result<bool>
    where
        K: AsRef<str>,
    {
        let key_str = key.as_ref();

        // Check if key exists and is not expired first
        if let Some(entry) = self.data.get(key_str) {
            if entry.is_expired() {
                // Remove expired key automatically and return false (key didn't exist)
                drop(entry); // Release the lock before removing
                self.expiration_index.remove(key_str);
                self.data.remove(key_str);
                return Ok(false);
            }
        }

        // Remove from expiration index
        self.expiration_index.remove(key_str);

        // Remove from main data store
        let was_removed = self.data.remove(key_str).is_some();

        Ok(was_removed)
    }

    /// Check if a key exists in the store
    pub fn exists<K>(&self, key: K) -> Result<bool>
    where
        K: AsRef<str>,
    {
        let key_str = key.as_ref();

        if let Some(entry) = self.data.get(key_str) {
            if entry.is_expired() {
                // Remove expired key
                drop(entry); // Release the lock before removing
                self.delete_sync(key_str)?;
                return Ok(false);
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Set expiration for an existing key
    pub async fn expire<K>(&self, key: K, ttl_seconds: u64) -> Result<bool>
    where
        K: AsRef<str>,
    {
        self.expire_with_options(key, ttl_seconds, ExpireFlag::None)
            .await
    }

    /// Set expiration on a key, conditional on the supplied flag.
    ///
    /// Implements the Redis 7 `EXPIRE key seconds [NX|XX|GT|LT]` semantics.
    /// The check-and-set is performed under a single per-shard write
    /// lock so that two racing `EXPIRE … GT` calls cannot both apply a
    /// shorter TTL than the eventual winner.
    pub async fn expire_with_options<K>(
        &self,
        key: K,
        ttl_seconds: u64,
        flag: ExpireFlag,
    ) -> Result<bool>
    where
        K: AsRef<str>,
    {
        let key_str = key.as_ref();

        let new_expires_at = Instant::now() + std::time::Duration::from_secs(ttl_seconds);

        if let Some(mut entry) = self.data.get_mut(key_str) {
            if entry.is_expired() {
                // Remove expired key
                drop(entry); // Release the lock before removing
                self.delete(key_str).await?;
                return Ok(false);
            }

            let current = entry.expires_at;
            let should_apply = match flag {
                ExpireFlag::None => true,
                ExpireFlag::Nx => current.is_none(),
                ExpireFlag::Xx => current.is_some(),
                ExpireFlag::Gt => match current {
                    Some(c) => new_expires_at > c,
                    // GT: per Redis, if there's no current TTL the
                    // existing "value" is +inf, so any new finite TTL
                    // is shorter and the operation is rejected.
                    None => false,
                },
                ExpireFlag::Lt => match current {
                    Some(c) => new_expires_at < c,
                    // LT: no current TTL is treated as +inf, so any new
                    // finite TTL is shorter and applies.
                    None => true,
                },
            };

            if !should_apply {
                return Ok(false);
            }

            entry.set_expiration(new_expires_at);

            // Update expiration index
            self.expiration_index
                .insert(key_str.to_string(), new_expires_at);

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get TTL for a key in seconds
    pub fn ttl<K>(&self, key: K) -> Result<i64>
    where
        K: AsRef<str>,
    {
        let key_str = key.as_ref();

        if let Some(entry) = self.data.get(key_str) {
            if entry.is_expired() {
                // Remove expired key
                drop(entry); // Release the lock before removing
                self.delete_sync(key_str)?;
                return Ok(-2); // Key doesn't exist
            }

            Ok(entry.ttl_seconds().unwrap_or(-1)) // -1 means no expiration
        } else {
            Ok(-2) // Key doesn't exist
        }
    }

    /// Remove expiration from a key
    pub fn persist<K>(&self, key: K) -> Result<bool>
    where
        K: AsRef<str>,
    {
        let key_str = key.as_ref();

        if let Some(mut entry) = self.data.get_mut(key_str) {
            if entry.is_expired() {
                // Remove expired key
                drop(entry); // Release the lock before removing
                self.delete_sync(key_str)?;
                return Ok(false);
            }

            let had_expiration = entry.expires_at.is_some();
            entry.remove_expiration();

            // Remove from expiration index
            self.expiration_index.remove(key_str);

            Ok(had_expiration)
        } else {
            Ok(false)
        }
    }

    /// Get all keys in the store (for debugging/admin purposes)
    pub fn keys(&self) -> Vec<String> {
        self.data.iter().map(|entry| entry.key().clone()).collect()
    }

    /// Clear all data from the store
    pub fn clear(&self) {
        self.data.clear();
        self.expiration_index.clear();
    }

    /// Atomically increment a key's integer value by 1
    /// If key doesn't exist, initialize it to 1
    /// Returns the new value after increment
    pub async fn incr<K>(&self, key: K) -> Result<i64>
    where
        K: Into<String>,
    {
        self.incr_by(key, 1).await
    }

    /// Atomically decrement a key's integer value by 1
    /// If key doesn't exist, initialize it to -1
    /// Returns the new value after decrement
    pub async fn decr<K>(&self, key: K) -> Result<i64>
    where
        K: Into<String>,
    {
        self.incr_by(key, -1).await
    }

    /// Atomically increment a key's integer value by the specified amount
    /// If key doesn't exist, initialize it to the increment value
    /// Returns the new value after increment
    pub async fn incr_by<K>(&self, key: K, increment: i64) -> Result<i64>
    where
        K: Into<String>,
    {
        let key_string = key.into();

        // Use entry API for atomic operation
        let result = match self.data.entry(key_string.clone()) {
            dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                // Check if expired first
                if entry.get().is_expired() {
                    // Remove expired key and treat as new
                    entry.remove();
                    self.expiration_index.remove(&key_string);

                    // Insert new value with increment
                    let new_value = increment;
                    let stored_value = StoredValue::new(ValueType::Integer(new_value));
                    self.data.insert(key_string.clone(), stored_value);
                    new_value
                } else {
                    // Try to convert existing value to integer and increment
                    let current_value = entry.get().value.to_integer()?;
                    let new_value = current_value
                        .checked_add(increment)
                        .ok_or(RustyPotatoError::IntegerOverflow)?;

                    // Update the value in place
                    let mut stored_value = entry.get().clone();
                    stored_value.value = ValueType::Integer(new_value);
                    stored_value.touch();
                    entry.insert(stored_value);

                    new_value
                }
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                // Key doesn't exist, initialize with increment value
                let new_value = increment;
                let stored_value = StoredValue::new(ValueType::Integer(new_value));
                entry.insert(stored_value);
                new_value
            }
        };

        // Persistence logging happens at the dispatch layer (see network::server)
        // after a successful command completes.
        Ok(result)
    }

    /// Set a field in a hash
    pub async fn hset<K, F, V>(&self, key: K, field: F, value: V) -> Result<bool>
    where
        K: Into<String>,
        F: Into<String>,
        V: Into<String>,
    {
        let key_string = key.into();
        let field_string = field.into();
        let value_string = value.into();

        // Check if key exists and handle expiration
        if let Some(entry) = self.data.get(&key_string) {
            if entry.is_expired() {
                drop(entry);
                self.delete_sync(&key_string)?;
            }
        }

        // Use entry API for atomic operation
        let is_new_field = match self.data.entry(key_string.clone()) {
            dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                // Key exists, check if it's a hash
                match &entry.get().value {
                    ValueType::Hash(existing_hash) => {
                        let is_new = !existing_hash.contains_key(&field_string);

                        // Clone the existing stored value and update it
                        let mut stored_value = entry.get().clone();
                        if let Ok(hash) = stored_value.value.as_hash_mut() {
                            hash.insert(field_string, value_string);
                            stored_value.touch();
                            stored_value.update_metadata();
                        }

                        // Replace the entry
                        entry.insert(stored_value);
                        is_new
                    }
                    _ => {
                        return Err(RustyPotatoError::StorageError {
                            message:
                                "WRONGTYPE Operation against a key holding the wrong kind of value"
                                    .to_string(),
                            operation: Some("hset".to_string()),
                            source: None,
                        });
                    }
                }
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                // Key doesn't exist, create new hash
                let mut hash = HashMap::new();
                hash.insert(field_string, value_string);
                let stored_value = StoredValue::new(ValueType::Hash(hash));
                entry.insert(stored_value);
                true // New field in new hash
            }
        };

        // Persistence logging happens at the dispatch layer.
        let _ = key_string;
        Ok(is_new_field)
    }

    /// Get a field from a hash
    pub async fn hget<K, F>(&self, key: K, field: F) -> Result<Option<String>>
    where
        K: AsRef<str>,
        F: AsRef<str>,
    {
        let key_str = key.as_ref();
        let field_str = field.as_ref();

        if let Some(entry) = self.data.get(key_str) {
            if entry.is_expired() {
                drop(entry);
                self.delete_sync(key_str)?;
                return Ok(None);
            }
            match entry.value.as_hash() {
                Ok(hash) => Ok(hash.get(field_str).cloned()),
                Err(_) => Err(RustyPotatoError::StorageError {
                    message: "WRONGTYPE Operation against a key holding the wrong kind of value"
                        .to_string(),
                    operation: Some("hget".to_string()),
                    source: None,
                }),
            }
        } else {
            Ok(None)
        }
    }

    /// Delete a field from a hash
    pub async fn hdel<K, F>(&self, key: K, field: F) -> Result<bool>
    where
        K: AsRef<str>,
        F: AsRef<str>,
    {
        let key_str = key.as_ref();
        let field_str = field.as_ref();

        // Check if key exists and handle expiration
        if let Some(entry) = self.data.get(key_str) {
            if entry.is_expired() {
                drop(entry);
                self.delete_sync(key_str)?;
                return Ok(false);
            }
        }

        // Check if key exists and is a hash
        let (was_removed, should_remove_key) = if let Some(entry) = self.data.get(key_str) {
            match &entry.value {
                ValueType::Hash(hash) => {
                    let field_exists = hash.contains_key(field_str);
                    let will_be_empty = hash.len() == 1 && field_exists;
                    (field_exists, will_be_empty)
                }
                _ => {
                    return Err(RustyPotatoError::StorageError {
                        message:
                            "WRONGTYPE Operation against a key holding the wrong kind of value"
                                .to_string(),
                        operation: Some("hdel".to_string()),
                        source: None,
                    });
                }
            }
        } else {
            return Ok(false);
        };

        if was_removed {
            if should_remove_key {
                // Remove the entire key if hash will be empty
                self.data.remove(key_str);
                self.expiration_index.remove(key_str);
            } else {
                // Update the hash by removing the field
                if let Some(mut entry) = self.data.get_mut(key_str) {
                    if let Ok(hash) = entry.value.as_hash_mut() {
                        hash.remove(field_str);
                        entry.touch();
                        entry.update_metadata();
                    }
                }
            }
            // Persistence logging happens at the dispatch layer.
        }

        Ok(was_removed)
    }

    /// Get all fields and values from a hash
    pub async fn hgetall<K>(&self, key: K) -> Result<Vec<(String, String)>>
    where
        K: AsRef<str>,
    {
        let key_str = key.as_ref();

        if let Some(entry) = self.data.get(key_str) {
            if entry.is_expired() {
                drop(entry);
                self.delete_sync(key_str)?;
                return Ok(Vec::new());
            }
            match entry.value.as_hash() {
                Ok(hash) => {
                    let mut result = Vec::new();
                    for (field, value) in hash.iter() {
                        result.push((field.clone(), value.clone()));
                    }
                    Ok(result)
                }
                Err(_) => Err(RustyPotatoError::StorageError {
                    message: "WRONGTYPE Operation against a key holding the wrong kind of value"
                        .to_string(),
                    operation: Some("hgetall".to_string()),
                    source: None,
                }),
            }
        } else {
            Ok(Vec::new())
        }
    }

    /// Check if a field exists in a hash
    pub async fn hexists<K, F>(&self, key: K, field: F) -> Result<bool>
    where
        K: AsRef<str>,
        F: AsRef<str>,
    {
        let key_str = key.as_ref();
        let field_str = field.as_ref();

        if let Some(entry) = self.data.get(key_str) {
            if entry.is_expired() {
                drop(entry);
                self.delete_sync(key_str)?;
                return Ok(false);
            }
            match entry.value.as_hash() {
                Ok(hash) => Ok(hash.contains_key(field_str)),
                Err(_) => Err(RustyPotatoError::StorageError {
                    message: "WRONGTYPE Operation against a key holding the wrong kind of value"
                        .to_string(),
                    operation: Some("hexists".to_string()),
                    source: None,
                }),
            }
        } else {
            Ok(false)
        }
    }

    /// Get memory usage statistics
    pub fn memory_stats(&self) -> MemoryStats {
        let key_count = self.data.len();
        let expiration_count = self.expiration_index.len();

        // Rough estimation of memory usage
        let estimated_memory = key_count * 100; // Rough estimate per key-value pair

        MemoryStats {
            key_count,
            expiration_count,
            estimated_memory_bytes: estimated_memory,
        }
    }

    /// Internal method to delete a key without expiration checking
    fn delete_internal(&self, key: &str) -> bool {
        // Remove from expiration index
        self.expiration_index.remove(key);

        // Remove from main data store
        self.data.remove(key).is_some()
    }

    /// Clean up expired keys (should be called periodically)
    pub fn cleanup_expired(&self) -> usize {
        let mut removed_count = 0;
        let now = Instant::now();

        // Collect expired keys to avoid holding locks during iteration
        let expired_keys: Vec<String> = self
            .expiration_index
            .iter()
            .filter_map(|entry| {
                if *entry.value() <= now {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();

        // Remove expired keys using internal delete to avoid expiration check
        for key in expired_keys {
            if self.delete_internal(&key) {
                removed_count += 1;
            }
        }

        removed_count
    }
}

/// Memory usage statistics
#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub key_count: usize,
    pub expiration_count: usize,
    pub estimated_memory_bytes: usize,
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl StoredValue {
    /// Create a new stored value
    pub fn new(value: ValueType) -> Self {
        let now = Instant::now();
        let metadata = ValueMetadata::for_value(&value);
        Self {
            value,
            created_at: now,
            last_accessed: now,
            expires_at: None,
            metadata,
        }
    }

    /// Create a new stored value with expiration
    pub fn new_with_expiration(value: ValueType, expires_at: Instant) -> Self {
        let now = Instant::now();
        let metadata = ValueMetadata::for_value(&value);
        Self {
            value,
            created_at: now,
            last_accessed: now,
            expires_at: Some(expires_at),
            metadata,
        }
    }

    /// Create a new stored value with TTL in seconds
    pub fn new_with_ttl(value: ValueType, ttl_seconds: u64) -> Self {
        let now = Instant::now();
        let expires_at = now + std::time::Duration::from_secs(ttl_seconds);
        let metadata = ValueMetadata::for_value(&value);
        Self {
            value,
            created_at: now,
            last_accessed: now,
            expires_at: Some(expires_at),
            metadata,
        }
    }

    /// Check if the value has expired
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires| Instant::now() > expires)
    }

    /// Update last accessed time
    pub fn touch(&mut self) {
        self.last_accessed = Instant::now();
    }

    /// Update metadata after value modification
    pub fn update_metadata(&mut self) {
        self.metadata.update_for_value(&self.value);
    }

    /// Set expiration time
    pub fn set_expiration(&mut self, expires_at: Instant) {
        self.expires_at = Some(expires_at);
    }

    /// Set TTL in seconds
    pub fn set_ttl(&mut self, ttl_seconds: u64) {
        let expires_at = Instant::now() + std::time::Duration::from_secs(ttl_seconds);
        self.expires_at = Some(expires_at);
    }

    /// Remove expiration
    pub fn remove_expiration(&mut self) {
        self.expires_at = None;
    }

    /// Get remaining TTL in seconds, returns None if no expiration set
    pub fn ttl_seconds(&self) -> Option<i64> {
        self.expires_at.map(|expires| {
            let now = Instant::now();
            if expires > now {
                (expires - now).as_secs() as i64
            } else {
                -2 // Expired
            }
        })
    }

    /// Get age of the value in seconds
    pub fn age_seconds(&self) -> u64 {
        self.created_at.elapsed().as_secs()
    }

    /// Get time since last access in seconds
    pub fn idle_time_seconds(&self) -> u64 {
        self.last_accessed.elapsed().as_secs()
    }

    /// Get the value as a string
    pub fn as_string(&self) -> String {
        self.value.as_string()
    }

    /// Try to get the value as an integer
    pub fn as_integer(&self) -> Result<i64> {
        self.value.to_integer()
    }

    /// Get the value type
    pub fn value_type(&self) -> &ValueType {
        &self.value
    }

    /// Get the type name
    pub fn type_name(&self) -> &'static str {
        self.value.type_name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    // Value type tests
    #[test]
    fn test_value_type_string_creation() {
        let value = ValueType::from("hello".to_string());
        assert!(value.is_string());
        assert!(!value.is_integer());
        assert_eq!(value.as_string(), "hello");
        assert_eq!(value.type_name(), "string");
    }

    #[test]
    fn test_value_type_integer_creation() {
        let value = ValueType::Integer(42);
        assert!(!value.is_string());
        assert!(value.is_integer());
        assert_eq!(value.as_string(), "42");
        assert_eq!(value.type_name(), "integer");
    }

    #[test]
    fn test_value_type_conversions() {
        // String to integer conversion
        let string_value = ValueType::from("123".to_string());
        assert_eq!(string_value.to_integer().unwrap(), 123);

        let invalid_string = ValueType::from("not_a_number".to_string());
        assert!(invalid_string.to_integer().is_err());

        // Integer to string conversion
        let int_value = ValueType::Integer(456);
        assert_eq!(int_value.to_string(), "456");
        assert_eq!(int_value.to_integer().unwrap(), 456);
    }

    // Hash operation tests
    #[tokio::test]
    async fn test_hash_operations_basic() {
        let store = MemoryStore::new();

        // Test HSET
        let result = store.hset("myhash", "field1", "value1").await.unwrap();
        assert!(result); // New field

        let result = store.hset("myhash", "field1", "value2").await.unwrap();
        assert!(!result); // Updated existing field

        // Test HGET
        let value = store.hget("myhash", "field1").await.unwrap();
        assert_eq!(value, Some("value2".to_string()));

        let value = store.hget("myhash", "nonexistent").await.unwrap();
        assert_eq!(value, None);

        // Test HEXISTS
        let exists = store.hexists("myhash", "field1").await.unwrap();
        assert!(exists);

        let exists = store.hexists("myhash", "nonexistent").await.unwrap();
        assert!(!exists);

        // Test HGETALL
        store.hset("myhash", "field2", "value3").await.unwrap();
        let all_fields = store.hgetall("myhash").await.unwrap();
        assert_eq!(all_fields.len(), 2);

        let mut field_map = std::collections::HashMap::new();
        for (field, value) in all_fields {
            field_map.insert(field, value);
        }
        assert_eq!(field_map.get("field1"), Some(&"value2".to_string()));
        assert_eq!(field_map.get("field2"), Some(&"value3".to_string()));

        // Test HDEL
        let deleted = store.hdel("myhash", "field1").await.unwrap();
        assert!(deleted);

        let deleted = store.hdel("myhash", "nonexistent").await.unwrap();
        assert!(!deleted);

        // Verify field was deleted
        let value = store.hget("myhash", "field1").await.unwrap();
        assert_eq!(value, None);

        // Verify other field still exists
        let value = store.hget("myhash", "field2").await.unwrap();
        assert_eq!(value, Some("value3".to_string()));
    }

    #[tokio::test]
    async fn test_hash_operations_type_safety() {
        let store = MemoryStore::new();

        // Set a string value first
        store.set("mykey", "string_value").await.unwrap();

        // Try to use hash operations on string key
        let result = store.hset("mykey", "field1", "value1").await;
        assert!(result.is_err());

        let result = store.hget("mykey", "field1").await;
        assert!(result.is_err());

        let result = store.hdel("mykey", "field1").await;
        assert!(result.is_err());

        let result = store.hexists("mykey", "field1").await;
        assert!(result.is_err());

        let result = store.hgetall("mykey").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_hash_operations_with_expiration() {
        let store = MemoryStore::new();

        // Create hash with expiration
        store.hset("myhash", "field1", "value1").await.unwrap();
        let expires_at = std::time::Instant::now() + std::time::Duration::from_millis(1);
        store
            .set_with_expiration(
                "myhash",
                ValueType::Hash({
                    let mut hash = HashMap::new();
                    hash.insert("field1".to_string(), "value1".to_string());
                    hash
                }),
                expires_at,
            )
            .await
            .unwrap();

        // Wait for expiration
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // All hash operations should return empty/false for expired key
        let value = store.hget("myhash", "field1").await.unwrap();
        assert_eq!(value, None);

        let exists = store.hexists("myhash", "field1").await.unwrap();
        assert!(!exists);

        let all_fields = store.hgetall("myhash").await.unwrap();
        assert!(all_fields.is_empty());

        let deleted = store.hdel("myhash", "field1").await.unwrap();
        assert!(!deleted);

        // HSET should create new hash after expiration
        let result = store.hset("myhash", "field2", "value2").await.unwrap();
        assert!(result); // New field in new hash
    }

    #[tokio::test]
    async fn test_hash_operations_empty_hash_cleanup() {
        let store = MemoryStore::new();

        // Create hash with single field
        store.hset("myhash", "field1", "value1").await.unwrap();
        assert!(store.exists("myhash").unwrap());

        // Delete the only field - should remove the entire key
        let deleted = store.hdel("myhash", "field1").await.unwrap();
        assert!(deleted);

        // Key should no longer exist
        assert!(!store.exists("myhash").unwrap());
    }

    #[tokio::test]
    async fn test_concurrent_hash_operations() {
        use std::sync::Arc;
        use tokio::task::JoinSet;

        let store = Arc::new(MemoryStore::new());
        let mut join_set = JoinSet::new();

        // Spawn multiple tasks performing hash operations concurrently
        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            join_set.spawn(async move {
                let hash_key = format!("hash{}", i % 3); // Use 3 different hash keys
                let field = format!("field{i}");
                let value = format!("value{i}");

                // HSET
                let _result = store_clone.hset(&hash_key, &field, &value).await.unwrap();
                // Could be true or false depending on timing

                // HGET
                let retrieved = store_clone.hget(&hash_key, &field).await.unwrap();
                assert_eq!(retrieved, Some(value));

                // HEXISTS
                let exists = store_clone.hexists(&hash_key, &field).await.unwrap();
                assert!(exists);

                // HGETALL
                let all_fields = store_clone.hgetall(&hash_key).await.unwrap();
                assert!(!all_fields.is_empty());
            });
        }

        // Wait for all tasks to complete
        while let Some(result) = join_set.join_next().await {
            result.unwrap(); // Panic if any task failed
        }

        // Verify final state
        for i in 0..3 {
            let hash_key = format!("hash{i}");
            let all_fields = store.hgetall(&hash_key).await.unwrap();
            assert!(!all_fields.is_empty());
        }
    }

    #[test]
    fn test_value_type_from_conversions() {
        let from_string: ValueType = "test".into();
        assert_eq!(from_string, ValueType::from("test".to_string()));

        let from_string_owned: ValueType = "test".to_string().into();
        assert_eq!(from_string_owned, ValueType::from("test".to_string()));

        let from_i64: ValueType = 42i64.into();
        assert_eq!(from_i64, ValueType::Integer(42));

        let from_i32: ValueType = 42i32.into();
        assert_eq!(from_i32, ValueType::Integer(42));
    }

    #[test]
    fn test_value_type_display() {
        let string_value = ValueType::from("hello".to_string());
        assert_eq!(format!("{string_value}"), "hello");

        let int_value = ValueType::Integer(42);
        assert_eq!(format!("{int_value}"), "42");
    }

    // StoredValue tests
    #[test]
    fn test_stored_value_creation() {
        let value = ValueType::from("test".to_string());
        let stored = StoredValue::new(value.clone());

        assert_eq!(stored.value, value);
        assert!(stored.expires_at.is_none());
        assert!(!stored.is_expired());

        // Check that created_at and last_accessed are recent
        let now = Instant::now();
        assert!(stored.created_at <= now);
        assert!(stored.last_accessed <= now);
        assert!(now.duration_since(stored.created_at) < Duration::from_millis(100));
    }

    #[test]
    fn test_stored_value_with_expiration() {
        let value = ValueType::from("test".to_string());
        let expires_at = Instant::now() + Duration::from_secs(60);
        let stored = StoredValue::new_with_expiration(value.clone(), expires_at);

        assert_eq!(stored.value, value);
        assert_eq!(stored.expires_at, Some(expires_at));
        assert!(!stored.is_expired());
    }

    #[test]
    fn test_stored_value_with_ttl() {
        let value = ValueType::from("test".to_string());
        let stored = StoredValue::new_with_ttl(value.clone(), 60);

        assert_eq!(stored.value, value);
        assert!(stored.expires_at.is_some());
        assert!(!stored.is_expired());

        // TTL should be approximately 60 seconds
        let ttl = stored.ttl_seconds().unwrap();
        assert!((59..=60).contains(&ttl));
    }

    #[test]
    fn test_stored_value_expiration() {
        let value = ValueType::from("test".to_string());
        let expires_at = Instant::now() - Duration::from_secs(1); // Already expired
        let stored = StoredValue::new_with_expiration(value, expires_at);

        assert!(stored.is_expired());
        assert_eq!(stored.ttl_seconds().unwrap(), -2); // Expired
    }

    #[test]
    fn test_stored_value_touch() {
        let value = ValueType::from("test".to_string());
        let mut stored = StoredValue::new(value);
        let original_access_time = stored.last_accessed;

        // Wait a small amount to ensure time difference
        std::thread::sleep(Duration::from_millis(1));
        stored.touch();

        assert!(stored.last_accessed > original_access_time);
    }

    // MemoryStore basic operation tests
    #[test]
    fn test_memory_store_creation() {
        let store = MemoryStore::new();
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());

        let default_store = MemoryStore::default();
        assert_eq!(default_store.len(), 0);
        assert!(default_store.is_empty());
    }

    #[tokio::test]
    async fn test_memory_store_set_and_get() {
        let store = MemoryStore::new();

        // Set a string value
        store.set("key1", "value1").await.unwrap();
        assert_eq!(store.len(), 1);
        assert!(!store.is_empty());

        // Get the value
        let retrieved = store.get("key1").unwrap().unwrap();
        assert_eq!(retrieved.value.to_string(), "value1");

        // Set an integer value
        store.set("key2", 42i64).await.unwrap();
        assert_eq!(store.len(), 2);

        let retrieved_int = store.get("key2").unwrap().unwrap();
        assert_eq!(retrieved_int.value.to_integer().unwrap(), 42);
    }

    #[tokio::test]
    async fn test_memory_store_overwrite() {
        let store = MemoryStore::new();

        // Set initial value
        store.set("key1", "value1").await.unwrap();
        assert_eq!(store.len(), 1);

        // Overwrite with new value
        store.set("key1", "value2").await.unwrap();
        assert_eq!(store.len(), 1); // Should still be 1

        let retrieved = store.get("key1").unwrap().unwrap();
        assert_eq!(retrieved.value.to_string(), "value2");
    }

    #[test]
    fn test_memory_store_get_nonexistent() {
        let store = MemoryStore::new();

        let result = store.get("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_memory_store_delete() {
        let store = MemoryStore::new();

        // Set a value
        store.set("key1", "value1").await.unwrap();
        assert_eq!(store.len(), 1);

        // Delete the value
        let deleted = store.delete("key1").await.unwrap();
        assert!(deleted);
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());

        // Try to delete non-existent key
        let not_deleted = store.delete("nonexistent").await.unwrap();
        assert!(!not_deleted);
    }

    #[tokio::test]
    async fn test_memory_store_exists() {
        let store = MemoryStore::new();

        // Check non-existent key
        assert!(!store.exists("key1").unwrap());

        // Set a value
        store.set("key1", "value1").await.unwrap();

        // Check existing key
        assert!(store.exists("key1").unwrap());

        // Delete and check again
        store.delete("key1").await.unwrap();
        assert!(!store.exists("key1").unwrap());
    }

    #[tokio::test]
    async fn test_memory_store_ttl_operations() {
        let store = MemoryStore::new();

        // Set value with TTL
        store.set_with_ttl("key1", "value1", 60).await.unwrap();
        assert_eq!(store.len(), 1);

        // Check TTL
        let ttl = store.ttl("key1").unwrap();
        assert!((59..=60).contains(&ttl));

        // Set expiration on existing key
        store.set("key2", "value2").await.unwrap();
        let expired = store.expire("key2", 30).await.unwrap();
        assert!(expired);

        let ttl2 = store.ttl("key2").unwrap();
        assert!((29..=30).contains(&ttl2));

        // Try to expire non-existent key
        let not_expired = store.expire("nonexistent", 30).await.unwrap();
        assert!(!not_expired);
    }

    #[tokio::test]
    async fn test_memory_store_persist() {
        let store = MemoryStore::new();

        // Set value with TTL
        store.set_with_ttl("key1", "value1", 60).await.unwrap();

        // Remove expiration
        let persisted = store.persist("key1").unwrap();
        assert!(persisted);

        // Check that TTL is now -1 (no expiration)
        let ttl = store.ttl("key1").unwrap();
        assert_eq!(ttl, -1);

        // Try to persist key without expiration
        store.set("key2", "value2").await.unwrap();
        let not_persisted = store.persist("key2").unwrap();
        assert!(!not_persisted);
    }

    #[tokio::test]
    async fn test_memory_store_expiration_handling() {
        let store = MemoryStore::new();

        // Set value with very short TTL
        let expires_at = Instant::now() + Duration::from_millis(1);
        store
            .set_with_expiration("key1", "value1", expires_at)
            .await
            .unwrap();

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Try to get expired key - should return None and clean up
        let result = store.get("key1").unwrap();
        assert!(result.is_none());
        assert_eq!(store.len(), 0);

        // exists() should also handle expiration
        store
            .set_with_expiration("key2", "value2", Instant::now() + Duration::from_millis(1))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;

        let exists = store.exists("key2").unwrap();
        assert!(!exists);
        assert_eq!(store.len(), 0);
    }

    #[tokio::test]
    async fn test_memory_store_cleanup_expired() {
        let store = MemoryStore::new();

        // Set some values with short TTL
        let expires_at = Instant::now() + Duration::from_millis(1);
        store
            .set_with_expiration("key1", "value1", expires_at)
            .await
            .unwrap();
        store
            .set_with_expiration("key2", "value2", expires_at)
            .await
            .unwrap();
        store.set("key3", "value3").await.unwrap(); // No expiration

        assert_eq!(store.len(), 3);

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Clean up expired keys
        let removed_count = store.cleanup_expired();
        assert_eq!(removed_count, 2);
        assert_eq!(store.len(), 1);

        // Only key3 should remain
        assert!(store.exists("key3").unwrap());
        assert!(!store.exists("key1").unwrap());
        assert!(!store.exists("key2").unwrap());
    }

    #[tokio::test]
    async fn test_memory_store_keys_and_clear() {
        let store = MemoryStore::new();

        // Set some values
        store.set("key1", "value1").await.unwrap();
        store.set("key2", "value2").await.unwrap();
        store.set("key3", "value3").await.unwrap();

        // Get all keys
        let keys = store.keys();
        assert_eq!(keys.len(), 3);
        assert!(keys.contains(&"key1".to_string()));
        assert!(keys.contains(&"key2".to_string()));
        assert!(keys.contains(&"key3".to_string()));

        // Clear all data
        store.clear();
        assert_eq!(store.len(), 0);
        assert!(store.is_empty());
        assert!(store.keys().is_empty());
    }

    #[tokio::test]
    async fn test_memory_store_memory_stats() {
        let store = MemoryStore::new();

        // Initial stats
        let stats = store.memory_stats();
        assert_eq!(stats.key_count, 0);
        assert_eq!(stats.expiration_count, 0);

        // Add some data
        store.set("key1", "value1").await.unwrap();
        store.set_with_ttl("key2", "value2", 60).await.unwrap();

        let stats = store.memory_stats();
        assert_eq!(stats.key_count, 2);
        assert_eq!(stats.expiration_count, 1);
        assert!(stats.estimated_memory_bytes > 0);
    }

    // Concurrent access tests
    #[tokio::test]
    async fn test_concurrent_set_get() {
        let store = Arc::new(MemoryStore::new());
        let mut handles = vec![];

        // Spawn multiple tasks that set and get values
        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            let handle = tokio::spawn(async move {
                let key = format!("key{i}");
                let value = format!("value{i}");

                // Set value
                store_clone.set(&key, value.as_str()).await.unwrap();

                // Get value back
                let retrieved = store_clone.get(&key).unwrap().unwrap();
                assert_eq!(retrieved.value.to_string(), value);
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all values are present
        assert_eq!(store.len(), 10);
        for i in 0..10 {
            let key = format!("key{i}");
            let expected_value = format!("value{i}");
            let retrieved = store.get(&key).unwrap().unwrap();
            assert_eq!(retrieved.value.to_string(), expected_value);
        }
    }

    #[tokio::test]
    async fn test_concurrent_same_key_access() {
        let store = Arc::new(MemoryStore::new());
        let mut handles = vec![];

        // Multiple tasks accessing the same key
        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            let handle = tokio::spawn(async move {
                let value = format!("value{i}");

                // Set value (last one wins)
                store_clone.set("shared_key", value.as_str()).await.unwrap();

                // Try to get the value
                let _retrieved = store_clone.get("shared_key").unwrap();
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Should have exactly one key
        assert_eq!(store.len(), 1);
        assert!(store.exists("shared_key").unwrap());
    }

    #[tokio::test]
    async fn test_concurrent_delete_operations() {
        let store = Arc::new(MemoryStore::new());

        // Pre-populate with data
        for i in 0..20 {
            let key = format!("key{i}");
            let value = format!("value{i}");
            store.set(&key, value.as_str()).await.unwrap();
        }

        assert_eq!(store.len(), 20);

        let mut handles = vec![];

        // Spawn tasks to delete half the keys
        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            let handle = tokio::spawn(async move {
                let key = format!("key{i}");
                store_clone.delete(&key).await.unwrap();
            });
            handles.push(handle);
        }

        // Wait for all deletions to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Should have 10 keys remaining
        assert_eq!(store.len(), 10);

        // Verify which keys remain
        for i in 0..20 {
            let key = format!("key{i}");
            let exists = store.exists(&key).unwrap();
            if i < 10 {
                assert!(!exists, "Key {key} should have been deleted");
            } else {
                assert!(exists, "Key {key} should still exist");
            }
        }
    }

    #[tokio::test]
    async fn test_concurrent_expiration_operations() {
        let store = Arc::new(MemoryStore::new());
        let mut handles = vec![];

        // Pre-populate with data
        for i in 0..10 {
            let key = format!("key{i}");
            let value = format!("value{i}");
            store.set(&key, value.as_str()).await.unwrap();
        }

        // Spawn tasks to set expiration on keys
        for i in 0..10 {
            let store_clone = Arc::clone(&store);
            let handle = tokio::spawn(async move {
                let key = format!("key{i}");
                let ttl = 60 + i as u64; // Different TTL for each key
                let _expired = store_clone.expire(&key, ttl).await.unwrap();
            });
            handles.push(handle);
        }

        // Wait for all operations to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all keys have expiration set
        for i in 0..10 {
            let key = format!("key{i}");
            let ttl = store.ttl(&key).unwrap();
            assert!(ttl > 0, "Key {key} should have positive TTL");
        }
    }

    #[tokio::test]
    async fn test_concurrent_mixed_operations() {
        let store = Arc::new(MemoryStore::new());
        let mut handles = vec![];

        // Spawn tasks doing different operations
        for i in 0..20 {
            let store_clone = Arc::clone(&store);
            let handle = tokio::spawn(async move {
                let key = format!("key{}", i % 10); // Use 10 different keys

                match i % 4 {
                    0 => {
                        // Set operation
                        let value = format!("value{i}");
                        store_clone.set(&key, value.as_str()).await.unwrap();
                    }
                    1 => {
                        // Get operation
                        let _result = store_clone.get(&key).unwrap();
                    }
                    2 => {
                        // Exists operation
                        let _exists = store_clone.exists(&key).unwrap();
                    }
                    3 => {
                        // Expire operation
                        let _expired = store_clone.expire(&key, 60).await.unwrap();
                    }
                    _ => unreachable!(),
                }
            });
            handles.push(handle);
        }

        // Wait for all operations to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Store should be in a consistent state
        assert!(store.len() <= 10); // At most 10 keys

        // All operations should complete without panics
        let stats = store.memory_stats();
        assert!(stats.key_count <= 10);
    }

    #[tokio::test]
    async fn test_concurrent_cleanup_expired() {
        let store = Arc::new(MemoryStore::new());

        // Set values with very short TTL
        for i in 0..20 {
            let key = format!("key{i}");
            let value = format!("value{i}");
            let expires_at = Instant::now() + Duration::from_millis(1);
            store
                .set_with_expiration(&key, value.as_str(), expires_at)
                .await
                .unwrap();
        }

        assert_eq!(store.len(), 20);

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(10)).await;

        let mut handles = vec![];

        // Spawn multiple tasks to clean up expired keys
        for _ in 0..5 {
            let store_clone = Arc::clone(&store);
            let handle = tokio::spawn(async move { store_clone.cleanup_expired() });
            handles.push(handle);
        }

        // Wait for all cleanup operations to complete
        let mut total_removed = 0;
        for handle in handles {
            total_removed += handle.await.unwrap();
        }

        // All keys should be removed (some tasks might not find any to remove)
        assert!(total_removed <= 20);
        assert_eq!(store.len(), 0);
    }

    // Edge case tests
    #[test]
    fn test_value_type_edge_cases() {
        // Test empty string
        let empty_string = ValueType::from("".to_string());
        assert_eq!(empty_string.to_string(), "");
        assert!(empty_string.to_integer().is_err());

        // Test zero integer
        let zero_int = ValueType::Integer(0);
        assert_eq!(zero_int.to_string(), "0");
        assert_eq!(zero_int.to_integer().unwrap(), 0);

        // Test negative integer
        let negative_int = ValueType::Integer(-42);
        assert_eq!(negative_int.to_string(), "-42");
        assert_eq!(negative_int.to_integer().unwrap(), -42);

        // Test string with negative number
        let negative_string = ValueType::from("-123".to_string());
        assert_eq!(negative_string.to_integer().unwrap(), -123);
    }

    #[test]
    fn test_stored_value_metadata_consistency() {
        let value = ValueType::from("test".to_string());
        let stored = StoredValue::new(value);

        // created_at should be <= last_accessed (they're set to the same time)
        assert!(stored.created_at <= stored.last_accessed);

        // Both should be very recent
        let now = Instant::now();
        assert!(now.duration_since(stored.created_at) < Duration::from_millis(100));
        assert!(now.duration_since(stored.last_accessed) < Duration::from_millis(100));
    }

    // Atomic operation tests
    #[tokio::test]
    async fn test_incr_new_key() {
        let store = MemoryStore::new();

        let result = store.incr("new_key").await.unwrap();
        assert_eq!(result, 1);

        let stored = store.get("new_key").unwrap().unwrap();
        assert_eq!(stored.value.to_integer().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_incr_existing_integer() {
        let store = MemoryStore::new();

        store.set("key1", 5i64).await.unwrap();
        let result = store.incr("key1").await.unwrap();
        assert_eq!(result, 6);

        let stored = store.get("key1").unwrap().unwrap();
        assert_eq!(stored.value.to_integer().unwrap(), 6);
    }

    #[tokio::test]
    async fn test_incr_existing_string_number() {
        let store = MemoryStore::new();

        store.set("key1", "42").await.unwrap();
        let result = store.incr("key1").await.unwrap();
        assert_eq!(result, 43);

        let stored = store.get("key1").unwrap().unwrap();
        assert_eq!(stored.value.to_integer().unwrap(), 43);
    }

    #[tokio::test]
    async fn test_incr_non_numeric_string() {
        let store = MemoryStore::new();

        store.set("key1", "not_a_number").await.unwrap();
        let result = store.incr("key1").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RustyPotatoError::NotAnInteger { .. }
        ));
    }

    #[tokio::test]
    async fn test_decr_new_key() {
        let store = MemoryStore::new();

        let result = store.decr("new_key").await.unwrap();
        assert_eq!(result, -1);

        let stored = store.get("new_key").unwrap().unwrap();
        assert_eq!(stored.value.to_integer().unwrap(), -1);
    }
}
