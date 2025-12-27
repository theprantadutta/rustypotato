//! Keyspace notification manager
//!
//! Emits keyspace and keyevent notifications through the pub/sub system.
//!
//! Redis-compatible notification channels:
//! - `__keyspace@0__:<key>` -> event type (what happened to key)
//! - `__keyevent@0__:<event>` -> key (what keys affected by event)

use super::PubSubManager;
use crate::config::SecurityConfig;
use std::sync::Arc;

/// Event types for keyspace notifications
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationEvent {
    // String operations
    Set,

    // Generic operations
    Del,
    Expire,
    Rename,

    // List operations
    Lpush,
    Rpush,
    Lpop,
    Rpop,

    // Hash operations
    Hset,
    Hdel,

    // Expiration events
    Expired,

    // Eviction events
    Evicted,
}

impl NotificationEvent {
    /// Get the event name as a string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Set => "set",
            Self::Del => "del",
            Self::Expire => "expire",
            Self::Rename => "rename",
            Self::Lpush => "lpush",
            Self::Rpush => "rpush",
            Self::Lpop => "lpop",
            Self::Rpop => "rpop",
            Self::Hset => "hset",
            Self::Hdel => "hdel",
            Self::Expired => "expired",
            Self::Evicted => "evicted",
        }
    }

    /// Check if this event should be emitted based on config
    fn should_emit(&self, config: &SecurityConfig) -> bool {
        match self {
            Self::Set => config.emit_string_events(),
            Self::Del | Self::Expire | Self::Rename => config.emit_generic_events(),
            Self::Lpush | Self::Rpush | Self::Lpop | Self::Rpop => config.emit_list_events(),
            Self::Hset | Self::Hdel => config.emit_hash_events(),
            Self::Expired => config.emit_expired_events(),
            Self::Evicted => config.emit_evicted_events(),
        }
    }
}

/// Manages keyspace notifications
pub struct NotificationManager {
    /// Pub/Sub manager for message delivery
    pubsub: Arc<PubSubManager>,
    /// Security configuration for notification settings
    config: SecurityConfig,
    /// Database index (always 0 for now)
    db_index: u8,
}

impl std::fmt::Debug for NotificationManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotificationManager")
            .field("config", &self.config)
            .field("db_index", &self.db_index)
            .finish_non_exhaustive()
    }
}

impl NotificationManager {
    /// Create a new notification manager
    pub fn new(pubsub: Arc<PubSubManager>, config: SecurityConfig) -> Self {
        Self {
            pubsub,
            config,
            db_index: 0,
        }
    }

    /// Check if notifications are enabled
    pub fn is_enabled(&self) -> bool {
        self.config.notifications_enabled()
    }

    /// Emit a notification for a key event
    ///
    /// This will publish to:
    /// - `__keyspace@0__:<key>` with the event name (if K flag is set)
    /// - `__keyevent@0__:<event>` with the key name (if E flag is set)
    pub async fn notify(&self, key: &str, event: NotificationEvent) -> usize {
        if !self.is_enabled() || !event.should_emit(&self.config) {
            return 0;
        }

        let mut total_receivers = 0;

        // Emit keyspace event: __keyspace@0__:<key> -> event_name
        if self.config.emit_keyspace_events() {
            let channel = format!("__keyspace@{}__:{}", self.db_index, key);
            total_receivers += self.pubsub.publish(&channel, event.as_str()).await;
        }

        // Emit keyevent event: __keyevent@0__:<event> -> key_name
        if self.config.emit_keyevent_events() {
            let channel = format!("__keyevent@{}__:{}", self.db_index, event.as_str());
            total_receivers += self.pubsub.publish(&channel, key).await;
        }

        total_receivers
    }

    /// Emit a SET notification
    pub async fn notify_set(&self, key: &str) -> usize {
        self.notify(key, NotificationEvent::Set).await
    }

    /// Emit a DEL notification
    pub async fn notify_del(&self, key: &str) -> usize {
        self.notify(key, NotificationEvent::Del).await
    }

    /// Emit an EXPIRE notification
    pub async fn notify_expire(&self, key: &str) -> usize {
        self.notify(key, NotificationEvent::Expire).await
    }

    /// Emit an LPUSH notification
    pub async fn notify_lpush(&self, key: &str) -> usize {
        self.notify(key, NotificationEvent::Lpush).await
    }

    /// Emit an RPUSH notification
    pub async fn notify_rpush(&self, key: &str) -> usize {
        self.notify(key, NotificationEvent::Rpush).await
    }

    /// Emit an LPOP notification
    pub async fn notify_lpop(&self, key: &str) -> usize {
        self.notify(key, NotificationEvent::Lpop).await
    }

    /// Emit an RPOP notification
    pub async fn notify_rpop(&self, key: &str) -> usize {
        self.notify(key, NotificationEvent::Rpop).await
    }

    /// Emit an HSET notification
    pub async fn notify_hset(&self, key: &str) -> usize {
        self.notify(key, NotificationEvent::Hset).await
    }

    /// Emit an HDEL notification
    pub async fn notify_hdel(&self, key: &str) -> usize {
        self.notify(key, NotificationEvent::Hdel).await
    }

    /// Emit an EXPIRED notification (key expired due to TTL)
    pub async fn notify_expired(&self, key: &str) -> usize {
        self.notify(key, NotificationEvent::Expired).await
    }

    /// Emit an EVICTED notification (key evicted due to memory limit)
    pub async fn notify_evicted(&self, key: &str) -> usize {
        self.notify(key, NotificationEvent::Evicted).await
    }

    /// Get the underlying pub/sub manager
    pub fn pubsub(&self) -> &Arc<PubSubManager> {
        &self.pubsub
    }

    /// Get the current configuration
    pub fn config(&self) -> &SecurityConfig {
        &self.config
    }

    /// Update the configuration
    pub fn set_config(&mut self, config: SecurityConfig) {
        self.config = config;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn create_test_config(flags: &str) -> SecurityConfig {
        SecurityConfig {
            requirepass: None,
            notify_keyspace_events: flags.to_string(),
        }
    }

    fn create_test_pubsub() -> Arc<PubSubManager> {
        Arc::new(PubSubManager::new())
    }

    #[test]
    fn test_notification_event_names() {
        assert_eq!(NotificationEvent::Set.as_str(), "set");
        assert_eq!(NotificationEvent::Del.as_str(), "del");
        assert_eq!(NotificationEvent::Expire.as_str(), "expire");
        assert_eq!(NotificationEvent::Lpush.as_str(), "lpush");
        assert_eq!(NotificationEvent::Rpush.as_str(), "rpush");
        assert_eq!(NotificationEvent::Lpop.as_str(), "lpop");
        assert_eq!(NotificationEvent::Rpop.as_str(), "rpop");
        assert_eq!(NotificationEvent::Hset.as_str(), "hset");
        assert_eq!(NotificationEvent::Hdel.as_str(), "hdel");
        assert_eq!(NotificationEvent::Expired.as_str(), "expired");
        assert_eq!(NotificationEvent::Evicted.as_str(), "evicted");
    }

    #[test]
    fn test_notification_manager_disabled() {
        let pubsub = create_test_pubsub();
        let config = create_test_config(""); // Disabled
        let manager = NotificationManager::new(pubsub, config);

        assert!(!manager.is_enabled());
    }

    #[test]
    fn test_notification_manager_enabled() {
        let pubsub = create_test_pubsub();
        let config = create_test_config("KEA"); // All events
        let manager = NotificationManager::new(pubsub, config);

        assert!(manager.is_enabled());
    }

    #[tokio::test]
    async fn test_notify_disabled_returns_zero() {
        let pubsub = create_test_pubsub();
        let config = create_test_config(""); // Disabled
        let manager = NotificationManager::new(pubsub, config);

        let count = manager.notify_set("mykey").await;
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_notify_keyspace_only() {
        let pubsub = create_test_pubsub();
        let subscriber_id = Uuid::new_v4();

        // Subscribe to keyspace channel
        pubsub.subscribe(subscriber_id, &["__keyspace@0__:mykey".to_string()]).await;

        // Get a receiver before publishing
        let subscriber = pubsub.get_subscriber(subscriber_id).unwrap();
        let mut receiver = subscriber.subscribe_messages();

        let config = create_test_config("K$"); // Keyspace + string events
        let manager = NotificationManager::new(Arc::clone(&pubsub), config);

        let count = manager.notify_set("mykey").await;
        assert_eq!(count, 1);

        // Verify message
        let msg = receiver.try_recv().unwrap();
        assert_eq!(msg.channel, "__keyspace@0__:mykey");
        assert_eq!(msg.payload, "set");
    }

    #[tokio::test]
    async fn test_notify_keyevent_only() {
        let pubsub = create_test_pubsub();
        let subscriber_id = Uuid::new_v4();

        // Subscribe to keyevent channel
        pubsub.subscribe(subscriber_id, &["__keyevent@0__:set".to_string()]).await;

        // Get a receiver before publishing
        let subscriber = pubsub.get_subscriber(subscriber_id).unwrap();
        let mut receiver = subscriber.subscribe_messages();

        let config = create_test_config("E$"); // Keyevent + string events
        let manager = NotificationManager::new(Arc::clone(&pubsub), config);

        let count = manager.notify_set("mykey").await;
        assert_eq!(count, 1);

        // Verify message
        let msg = receiver.try_recv().unwrap();
        assert_eq!(msg.channel, "__keyevent@0__:set");
        assert_eq!(msg.payload, "mykey");
    }

    #[tokio::test]
    async fn test_notify_both_keyspace_and_keyevent() {
        let pubsub = create_test_pubsub();
        let subscriber_id = Uuid::new_v4();

        // Subscribe to both channels
        pubsub.subscribe(subscriber_id, &[
            "__keyspace@0__:mykey".to_string(),
            "__keyevent@0__:del".to_string(),
        ]).await;

        // Get a receiver before publishing
        let subscriber = pubsub.get_subscriber(subscriber_id).unwrap();
        let mut receiver = subscriber.subscribe_messages();

        let config = create_test_config("KEg"); // Both + generic events
        let manager = NotificationManager::new(Arc::clone(&pubsub), config);

        let count = manager.notify_del("mykey").await;
        assert_eq!(count, 2); // Both channels received

        // Verify messages
        let msg1 = receiver.try_recv().unwrap();
        let msg2 = receiver.try_recv().unwrap();

        // Order may vary, so check both
        let channels: Vec<&str> = vec![&msg1.channel, &msg2.channel];
        assert!(channels.contains(&"__keyspace@0__:mykey"));
        assert!(channels.contains(&"__keyevent@0__:del"));
    }

    #[tokio::test]
    async fn test_notify_all_events_with_a_flag() {
        let pubsub = create_test_pubsub();
        let subscriber_id = Uuid::new_v4();

        // Subscribe to list event channel
        pubsub.subscribe(subscriber_id, &["__keyevent@0__:lpush".to_string()]).await;

        // Get a receiver before publishing
        let subscriber = pubsub.get_subscriber(subscriber_id).unwrap();
        let mut receiver = subscriber.subscribe_messages();

        let config = create_test_config("A"); // All events (includes KE implicitly)
        let manager = NotificationManager::new(Arc::clone(&pubsub), config);

        // A flag should enable both K and E
        assert!(manager.config().emit_keyspace_events());
        assert!(manager.config().emit_keyevent_events());
        assert!(manager.config().emit_list_events());

        let count = manager.notify_lpush("mylist").await;
        assert!(count >= 1);

        // Verify message
        let msg = receiver.try_recv().unwrap();
        assert_eq!(msg.channel, "__keyevent@0__:lpush");
        assert_eq!(msg.payload, "mylist");
    }

    #[tokio::test]
    async fn test_notify_filtered_by_event_type() {
        let pubsub = create_test_pubsub();
        let subscriber_id = Uuid::new_v4();

        // Subscribe to channels
        pubsub.subscribe(subscriber_id, &[
            "__keyevent@0__:lpush".to_string(),
            "__keyevent@0__:set".to_string(),
        ]).await;

        let config = create_test_config("El"); // Only list events, not string events
        let manager = NotificationManager::new(Arc::clone(&pubsub), config);

        // LPUSH should trigger notification
        let count = manager.notify_lpush("mylist").await;
        assert_eq!(count, 1);

        // SET should NOT trigger notification ($ flag not set)
        let count = manager.notify_set("mykey").await;
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_notify_expired_event() {
        let pubsub = create_test_pubsub();
        let subscriber_id = Uuid::new_v4();

        // Subscribe to expired events
        pubsub.subscribe(subscriber_id, &["__keyevent@0__:expired".to_string()]).await;

        // Get a receiver before publishing
        let subscriber = pubsub.get_subscriber(subscriber_id).unwrap();
        let mut receiver = subscriber.subscribe_messages();

        let config = create_test_config("Ex"); // Expired events
        let manager = NotificationManager::new(Arc::clone(&pubsub), config);

        let count = manager.notify_expired("expiredkey").await;
        assert_eq!(count, 1);

        // Verify message
        let msg = receiver.try_recv().unwrap();
        assert_eq!(msg.channel, "__keyevent@0__:expired");
        assert_eq!(msg.payload, "expiredkey");
    }

    #[tokio::test]
    async fn test_notify_pattern_subscription() {
        let pubsub = create_test_pubsub();
        let subscriber_id = Uuid::new_v4();

        // Subscribe to pattern for all keyspace events
        pubsub.psubscribe(subscriber_id, &["__keyspace@*__:*".to_string()]).await;

        // Get a receiver before publishing
        let subscriber = pubsub.get_subscriber(subscriber_id).unwrap();
        let mut receiver = subscriber.subscribe_messages();

        let config = create_test_config("K$"); // Keyspace + string events
        let manager = NotificationManager::new(Arc::clone(&pubsub), config);

        let count = manager.notify_set("anykey").await;
        assert_eq!(count, 1);

        // Verify message (should be pattern match)
        let msg = receiver.try_recv().unwrap();
        assert_eq!(msg.channel, "__keyspace@0__:anykey");
        assert_eq!(msg.payload, "set");
        assert_eq!(msg.pattern, Some("__keyspace@*__:*".to_string()));
    }

    #[test]
    fn test_event_should_emit() {
        // Test string events
        let config = create_test_config("$");
        assert!(NotificationEvent::Set.should_emit(&config));
        assert!(!NotificationEvent::Del.should_emit(&config));

        // Test generic events
        let config = create_test_config("g");
        assert!(NotificationEvent::Del.should_emit(&config));
        assert!(NotificationEvent::Expire.should_emit(&config));
        assert!(!NotificationEvent::Set.should_emit(&config));

        // Test list events
        let config = create_test_config("l");
        assert!(NotificationEvent::Lpush.should_emit(&config));
        assert!(NotificationEvent::Rpush.should_emit(&config));
        assert!(NotificationEvent::Lpop.should_emit(&config));
        assert!(NotificationEvent::Rpop.should_emit(&config));
        assert!(!NotificationEvent::Set.should_emit(&config));

        // Test hash events
        let config = create_test_config("h");
        assert!(NotificationEvent::Hset.should_emit(&config));
        assert!(NotificationEvent::Hdel.should_emit(&config));
        assert!(!NotificationEvent::Lpush.should_emit(&config));

        // Test expired events
        let config = create_test_config("x");
        assert!(NotificationEvent::Expired.should_emit(&config));
        assert!(!NotificationEvent::Set.should_emit(&config));

        // Test evicted events
        let config = create_test_config("e");
        assert!(NotificationEvent::Evicted.should_emit(&config));
        assert!(!NotificationEvent::Set.should_emit(&config));

        // Test A flag (all events)
        let config = create_test_config("A");
        assert!(NotificationEvent::Set.should_emit(&config));
        assert!(NotificationEvent::Del.should_emit(&config));
        assert!(NotificationEvent::Lpush.should_emit(&config));
        assert!(NotificationEvent::Hset.should_emit(&config));
        assert!(NotificationEvent::Expired.should_emit(&config));
        assert!(NotificationEvent::Evicted.should_emit(&config));
    }

    #[test]
    fn test_manager_config_update() {
        let pubsub = create_test_pubsub();
        let config = create_test_config("");
        let mut manager = NotificationManager::new(pubsub, config);

        assert!(!manager.is_enabled());

        manager.set_config(create_test_config("KEA"));

        assert!(manager.is_enabled());
    }
}
