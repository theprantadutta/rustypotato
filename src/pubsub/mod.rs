//! Pub/Sub messaging system for RustyPotato
//!
//! This module implements Redis-compatible publish/subscribe messaging,
//! allowing clients to subscribe to channels and receive messages in real-time.

pub mod manager;
pub mod pattern;

pub use manager::{PubSubManager, SubscriberState, PubSubMessage};
pub use pattern::PatternMatcher;
