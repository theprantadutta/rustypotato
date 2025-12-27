//! Pub/Sub manager for handling subscriptions and message delivery
//!
//! Manages channel and pattern subscriptions with thread-safe data structures
//! and uses tokio broadcast channels for message delivery.

use super::PatternMatcher;
use dashmap::DashMap;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

/// Maximum number of messages that can be buffered per subscriber
const MESSAGE_BUFFER_SIZE: usize = 1000;

/// A message to be delivered to subscribers
#[derive(Debug, Clone)]
pub struct PubSubMessage {
    /// The type of message
    pub kind: MessageKind,
    /// The pattern that matched (for pattern subscriptions), None for direct channel subscriptions
    pub pattern: Option<String>,
    /// The channel the message was published to
    pub channel: String,
    /// The message payload
    pub payload: String,
}

/// Types of pub/sub messages
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageKind {
    /// Regular message from PUBLISH
    Message,
    /// Pattern-matched message
    PMessage,
    /// Subscription confirmation
    Subscribe,
    /// Pattern subscription confirmation
    PSubscribe,
    /// Unsubscription confirmation
    Unsubscribe,
    /// Pattern unsubscription confirmation
    PUnsubscribe,
}

/// State for a single subscriber
pub struct SubscriberState {
    /// Client ID
    pub client_id: Uuid,
    /// Channels this subscriber is subscribed to
    pub channels: RwLock<HashSet<String>>,
    /// Patterns this subscriber is subscribed to
    pub patterns: RwLock<HashSet<String>>,
    /// Sender for delivering messages to this subscriber
    pub message_tx: broadcast::Sender<PubSubMessage>,
    /// Keep-alive receiver to prevent send() from failing when no external receivers exist
    _keep_alive: broadcast::Receiver<PubSubMessage>,
}

impl std::fmt::Debug for SubscriberState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubscriberState")
            .field("client_id", &self.client_id)
            .field("channels", &self.channels)
            .field("patterns", &self.patterns)
            .finish_non_exhaustive()
    }
}

impl SubscriberState {
    /// Create a new subscriber state
    pub fn new(client_id: Uuid) -> Self {
        let (message_tx, keep_alive) = broadcast::channel(MESSAGE_BUFFER_SIZE);
        Self {
            client_id,
            channels: RwLock::new(HashSet::new()),
            patterns: RwLock::new(HashSet::new()),
            message_tx,
            _keep_alive: keep_alive,
        }
    }

    /// Get a receiver for messages
    pub fn subscribe_messages(&self) -> broadcast::Receiver<PubSubMessage> {
        self.message_tx.subscribe()
    }

    /// Get the total subscription count (channels + patterns)
    pub async fn subscription_count(&self) -> usize {
        let channels = self.channels.read().await;
        let patterns = self.patterns.read().await;
        channels.len() + patterns.len()
    }

    /// Check if the subscriber is in subscriber mode (has any subscriptions)
    pub async fn is_subscribed(&self) -> bool {
        self.subscription_count().await > 0
    }
}

/// Pub/Sub manager handling subscriptions and message routing
pub struct PubSubManager {
    /// Map of channel name -> set of subscriber client IDs
    channel_subscribers: DashMap<String, HashSet<Uuid>>,
    /// Map of pattern -> set of subscriber client IDs
    pattern_subscribers: DashMap<String, HashSet<Uuid>>,
    /// Map of client ID -> subscriber state
    subscriber_states: DashMap<Uuid, Arc<SubscriberState>>,
    /// Compiled pattern matchers (cached for performance)
    pattern_matchers: DashMap<String, PatternMatcher>,
}

impl PubSubManager {
    /// Create a new PubSubManager
    pub fn new() -> Self {
        Self {
            channel_subscribers: DashMap::new(),
            pattern_subscribers: DashMap::new(),
            subscriber_states: DashMap::new(),
            pattern_matchers: DashMap::new(),
        }
    }

    /// Get or create subscriber state for a client
    pub fn get_or_create_subscriber(&self, client_id: Uuid) -> Arc<SubscriberState> {
        self.subscriber_states
            .entry(client_id)
            .or_insert_with(|| Arc::new(SubscriberState::new(client_id)))
            .clone()
    }

    /// Get subscriber state if it exists
    pub fn get_subscriber(&self, client_id: Uuid) -> Option<Arc<SubscriberState>> {
        self.subscriber_states.get(&client_id).map(|s| s.clone())
    }

    /// Subscribe a client to one or more channels
    /// Returns a list of (channel, subscription_count) for confirmation messages
    pub async fn subscribe(
        &self,
        client_id: Uuid,
        channels: &[String],
    ) -> Vec<(String, usize)> {
        let subscriber = self.get_or_create_subscriber(client_id);
        let mut results = Vec::with_capacity(channels.len());

        for channel in channels {
            // Add to subscriber's channel set
            {
                let mut sub_channels = subscriber.channels.write().await;
                sub_channels.insert(channel.clone());
            }

            // Add to channel's subscriber set
            self.channel_subscribers
                .entry(channel.clone())
                .or_default()
                .insert(client_id);

            // Get current subscription count
            let count = subscriber.subscription_count().await;
            results.push((channel.clone(), count));
        }

        results
    }

    /// Unsubscribe a client from channels
    /// If channels is empty, unsubscribe from all channels
    /// Returns a list of (channel, subscription_count) for confirmation messages
    pub async fn unsubscribe(
        &self,
        client_id: Uuid,
        channels: &[String],
    ) -> Vec<(String, usize)> {
        let subscriber = match self.get_subscriber(client_id) {
            Some(s) => s,
            None => return Vec::new(),
        };

        let channels_to_remove: Vec<String> = if channels.is_empty() {
            // Unsubscribe from all channels
            subscriber.channels.read().await.iter().cloned().collect()
        } else {
            channels.to_vec()
        };

        let mut results = Vec::with_capacity(channels_to_remove.len());

        for channel in channels_to_remove {
            // Remove from subscriber's channel set
            {
                let mut sub_channels = subscriber.channels.write().await;
                sub_channels.remove(&channel);
            }

            // Remove from channel's subscriber set
            if let Some(mut subscribers) = self.channel_subscribers.get_mut(&channel) {
                subscribers.remove(&client_id);
                if subscribers.is_empty() {
                    drop(subscribers);
                    self.channel_subscribers.remove(&channel);
                }
            }

            // Get current subscription count
            let count = subscriber.subscription_count().await;
            results.push((channel, count));
        }

        // Clean up subscriber state if no longer subscribed to anything
        if !subscriber.is_subscribed().await {
            self.subscriber_states.remove(&client_id);
        }

        results
    }

    /// Subscribe a client to one or more patterns
    /// Returns a list of (pattern, subscription_count) for confirmation messages
    pub async fn psubscribe(
        &self,
        client_id: Uuid,
        patterns: &[String],
    ) -> Vec<(String, usize)> {
        let subscriber = self.get_or_create_subscriber(client_id);
        let mut results = Vec::with_capacity(patterns.len());

        for pattern in patterns {
            // Add to subscriber's pattern set
            {
                let mut sub_patterns = subscriber.patterns.write().await;
                sub_patterns.insert(pattern.clone());
            }

            // Add to pattern's subscriber set
            self.pattern_subscribers
                .entry(pattern.clone())
                .or_default()
                .insert(client_id);

            // Cache the pattern matcher
            self.pattern_matchers
                .entry(pattern.clone())
                .or_insert_with(|| PatternMatcher::new(pattern));

            // Get current subscription count
            let count = subscriber.subscription_count().await;
            results.push((pattern.clone(), count));
        }

        results
    }

    /// Unsubscribe a client from patterns
    /// If patterns is empty, unsubscribe from all patterns
    /// Returns a list of (pattern, subscription_count) for confirmation messages
    pub async fn punsubscribe(
        &self,
        client_id: Uuid,
        patterns: &[String],
    ) -> Vec<(String, usize)> {
        let subscriber = match self.get_subscriber(client_id) {
            Some(s) => s,
            None => return Vec::new(),
        };

        let patterns_to_remove: Vec<String> = if patterns.is_empty() {
            // Unsubscribe from all patterns
            subscriber.patterns.read().await.iter().cloned().collect()
        } else {
            patterns.to_vec()
        };

        let mut results = Vec::with_capacity(patterns_to_remove.len());

        for pattern in patterns_to_remove {
            // Remove from subscriber's pattern set
            {
                let mut sub_patterns = subscriber.patterns.write().await;
                sub_patterns.remove(&pattern);
            }

            // Remove from pattern's subscriber set
            if let Some(mut subscribers) = self.pattern_subscribers.get_mut(&pattern) {
                subscribers.remove(&client_id);
                if subscribers.is_empty() {
                    drop(subscribers);
                    self.pattern_subscribers.remove(&pattern);
                    self.pattern_matchers.remove(&pattern);
                }
            }

            // Get current subscription count
            let count = subscriber.subscription_count().await;
            results.push((pattern, count));
        }

        // Clean up subscriber state if no longer subscribed to anything
        if !subscriber.is_subscribed().await {
            self.subscriber_states.remove(&client_id);
        }

        results
    }

    /// Publish a message to a channel
    /// Returns the number of subscribers that received the message
    pub async fn publish(&self, channel: &str, message: &str) -> usize {
        let mut receivers = 0;

        // Deliver to direct channel subscribers
        if let Some(subscribers) = self.channel_subscribers.get(channel) {
            for client_id in subscribers.iter() {
                if let Some(subscriber) = self.subscriber_states.get(client_id) {
                    let msg = PubSubMessage {
                        kind: MessageKind::Message,
                        pattern: None,
                        channel: channel.to_string(),
                        payload: message.to_string(),
                    };
                    // Try to send, ignore if receiver is dropped
                    if subscriber.message_tx.send(msg).is_ok() {
                        receivers += 1;
                    }
                }
            }
        }

        // Deliver to pattern subscribers
        for entry in self.pattern_subscribers.iter() {
            let pattern = entry.key();
            let subscribers = entry.value();

            // Check if pattern matches channel
            let matches = self
                .pattern_matchers
                .get(pattern)
                .map(|m| m.matches(channel))
                .unwrap_or(false);

            if matches {
                for client_id in subscribers.iter() {
                    if let Some(subscriber) = self.subscriber_states.get(client_id) {
                        let msg = PubSubMessage {
                            kind: MessageKind::PMessage,
                            pattern: Some(pattern.clone()),
                            channel: channel.to_string(),
                            payload: message.to_string(),
                        };
                        // Try to send, ignore if receiver is dropped
                        if subscriber.message_tx.send(msg).is_ok() {
                            receivers += 1;
                        }
                    }
                }
            }
        }

        receivers
    }

    /// Remove all subscriptions for a client (called when client disconnects)
    pub async fn remove_client(&self, client_id: Uuid) {
        // Get subscriber state
        if let Some((_, subscriber)) = self.subscriber_states.remove(&client_id) {
            // Remove from all channel subscriptions
            let channels: Vec<String> = subscriber.channels.read().await.iter().cloned().collect();
            for channel in channels {
                if let Some(mut subscribers) = self.channel_subscribers.get_mut(&channel) {
                    subscribers.remove(&client_id);
                    if subscribers.is_empty() {
                        drop(subscribers);
                        self.channel_subscribers.remove(&channel);
                    }
                }
            }

            // Remove from all pattern subscriptions
            let patterns: Vec<String> = subscriber.patterns.read().await.iter().cloned().collect();
            for pattern in patterns {
                if let Some(mut subscribers) = self.pattern_subscribers.get_mut(&pattern) {
                    subscribers.remove(&client_id);
                    if subscribers.is_empty() {
                        drop(subscribers);
                        self.pattern_subscribers.remove(&pattern);
                        self.pattern_matchers.remove(&pattern);
                    }
                }
            }
        }
    }

    /// Get statistics about the pub/sub system
    pub fn stats(&self) -> PubSubStats {
        PubSubStats {
            total_channels: self.channel_subscribers.len(),
            total_patterns: self.pattern_subscribers.len(),
            total_subscribers: self.subscriber_states.len(),
        }
    }

    /// Check if a client is in subscriber mode
    pub async fn is_subscriber(&self, client_id: Uuid) -> bool {
        if let Some(subscriber) = self.subscriber_states.get(&client_id) {
            subscriber.is_subscribed().await
        } else {
            false
        }
    }
}

impl Default for PubSubManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Pub/Sub statistics
#[derive(Debug, Clone)]
pub struct PubSubStats {
    /// Number of active channels with subscribers
    pub total_channels: usize,
    /// Number of active patterns with subscribers
    pub total_patterns: usize,
    /// Number of unique subscribers
    pub total_subscribers: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pubsub_manager_creation() {
        let manager = PubSubManager::new();
        let stats = manager.stats();
        assert_eq!(stats.total_channels, 0);
        assert_eq!(stats.total_patterns, 0);
        assert_eq!(stats.total_subscribers, 0);
    }

    #[tokio::test]
    async fn test_subscribe_single_channel() {
        let manager = PubSubManager::new();
        let client_id = Uuid::new_v4();

        let results = manager.subscribe(client_id, &["news".to_string()]).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "news");
        assert_eq!(results[0].1, 1); // subscription count

        let stats = manager.stats();
        assert_eq!(stats.total_channels, 1);
        assert_eq!(stats.total_subscribers, 1);
    }

    #[tokio::test]
    async fn test_subscribe_multiple_channels() {
        let manager = PubSubManager::new();
        let client_id = Uuid::new_v4();

        let results = manager
            .subscribe(client_id, &["news".to_string(), "sports".to_string()])
            .await;

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].1, 1); // After first subscription
        assert_eq!(results[1].1, 2); // After second subscription

        let stats = manager.stats();
        assert_eq!(stats.total_channels, 2);
        assert_eq!(stats.total_subscribers, 1);
    }

    #[tokio::test]
    async fn test_unsubscribe_channel() {
        let manager = PubSubManager::new();
        let client_id = Uuid::new_v4();

        manager
            .subscribe(client_id, &["news".to_string(), "sports".to_string()])
            .await;

        let results = manager.unsubscribe(client_id, &["news".to_string()]).await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "news");
        assert_eq!(results[0].1, 1); // One subscription remaining

        let stats = manager.stats();
        assert_eq!(stats.total_channels, 1);
    }

    #[tokio::test]
    async fn test_unsubscribe_all_channels() {
        let manager = PubSubManager::new();
        let client_id = Uuid::new_v4();

        manager
            .subscribe(client_id, &["news".to_string(), "sports".to_string()])
            .await;

        let results = manager.unsubscribe(client_id, &[]).await;

        assert_eq!(results.len(), 2);

        let stats = manager.stats();
        assert_eq!(stats.total_channels, 0);
        assert_eq!(stats.total_subscribers, 0);
    }

    #[tokio::test]
    async fn test_psubscribe() {
        let manager = PubSubManager::new();
        let client_id = Uuid::new_v4();

        let results = manager
            .psubscribe(client_id, &["news.*".to_string()])
            .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "news.*");
        assert_eq!(results[0].1, 1);

        let stats = manager.stats();
        assert_eq!(stats.total_patterns, 1);
        assert_eq!(stats.total_subscribers, 1);
    }

    #[tokio::test]
    async fn test_punsubscribe() {
        let manager = PubSubManager::new();
        let client_id = Uuid::new_v4();

        manager
            .psubscribe(client_id, &["news.*".to_string(), "sports.*".to_string()])
            .await;

        let results = manager
            .punsubscribe(client_id, &["news.*".to_string()])
            .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, 1); // One pattern remaining

        let stats = manager.stats();
        assert_eq!(stats.total_patterns, 1);
    }

    #[tokio::test]
    async fn test_publish_to_channel() {
        let manager = PubSubManager::new();
        let client_id = Uuid::new_v4();

        manager.subscribe(client_id, &["news".to_string()]).await;

        // Get a receiver before publishing
        let subscriber = manager.get_subscriber(client_id).unwrap();
        let mut receiver = subscriber.subscribe_messages();

        let receivers = manager.publish("news", "Hello, world!").await;
        assert_eq!(receivers, 1);

        // Check received message
        let msg = receiver.try_recv().unwrap();
        assert_eq!(msg.kind, MessageKind::Message);
        assert_eq!(msg.channel, "news");
        assert_eq!(msg.payload, "Hello, world!");
        assert!(msg.pattern.is_none());
    }

    #[tokio::test]
    async fn test_publish_to_pattern() {
        let manager = PubSubManager::new();
        let client_id = Uuid::new_v4();

        manager
            .psubscribe(client_id, &["news.*".to_string()])
            .await;

        // Get a receiver before publishing
        let subscriber = manager.get_subscriber(client_id).unwrap();
        let mut receiver = subscriber.subscribe_messages();

        let receivers = manager.publish("news.sports", "Sports update!").await;
        assert_eq!(receivers, 1);

        // Check received message
        let msg = receiver.try_recv().unwrap();
        assert_eq!(msg.kind, MessageKind::PMessage);
        assert_eq!(msg.channel, "news.sports");
        assert_eq!(msg.payload, "Sports update!");
        assert_eq!(msg.pattern, Some("news.*".to_string()));
    }

    #[tokio::test]
    async fn test_publish_no_subscribers() {
        let manager = PubSubManager::new();

        let receivers = manager.publish("news", "Hello!").await;
        assert_eq!(receivers, 0);
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let manager = PubSubManager::new();
        let client1 = Uuid::new_v4();
        let client2 = Uuid::new_v4();

        manager.subscribe(client1, &["news".to_string()]).await;
        manager.subscribe(client2, &["news".to_string()]).await;

        let receivers = manager.publish("news", "Hello!").await;
        assert_eq!(receivers, 2);

        let stats = manager.stats();
        assert_eq!(stats.total_channels, 1);
        assert_eq!(stats.total_subscribers, 2);
    }

    #[tokio::test]
    async fn test_remove_client() {
        let manager = PubSubManager::new();
        let client_id = Uuid::new_v4();

        manager
            .subscribe(client_id, &["news".to_string(), "sports".to_string()])
            .await;
        manager
            .psubscribe(client_id, &["events.*".to_string()])
            .await;

        manager.remove_client(client_id).await;

        let stats = manager.stats();
        assert_eq!(stats.total_channels, 0);
        assert_eq!(stats.total_patterns, 0);
        assert_eq!(stats.total_subscribers, 0);
    }

    #[tokio::test]
    async fn test_is_subscriber() {
        let manager = PubSubManager::new();
        let client_id = Uuid::new_v4();

        assert!(!manager.is_subscriber(client_id).await);

        manager.subscribe(client_id, &["news".to_string()]).await;
        assert!(manager.is_subscriber(client_id).await);

        manager.unsubscribe(client_id, &["news".to_string()]).await;
        assert!(!manager.is_subscriber(client_id).await);
    }

    #[tokio::test]
    async fn test_mixed_channel_and_pattern_subscriptions() {
        let manager = PubSubManager::new();
        let client_id = Uuid::new_v4();

        manager.subscribe(client_id, &["news".to_string()]).await;
        manager
            .psubscribe(client_id, &["events.*".to_string()])
            .await;

        let subscriber = manager.get_subscriber(client_id).unwrap();
        assert_eq!(subscriber.subscription_count().await, 2);

        let stats = manager.stats();
        assert_eq!(stats.total_channels, 1);
        assert_eq!(stats.total_patterns, 1);
        assert_eq!(stats.total_subscribers, 1);
    }

    #[tokio::test]
    async fn test_subscriber_state_creation() {
        let client_id = Uuid::new_v4();
        let state = SubscriberState::new(client_id);

        assert_eq!(state.client_id, client_id);
        assert_eq!(state.subscription_count().await, 0);
        assert!(!state.is_subscribed().await);
    }

    #[tokio::test]
    async fn test_concurrent_subscriptions() {
        let manager = Arc::new(PubSubManager::new());
        let mut handles = vec![];

        // 10 clients subscribing concurrently
        for i in 0..10 {
            let manager = Arc::clone(&manager);
            handles.push(tokio::spawn(async move {
                let client_id = Uuid::new_v4();
                manager
                    .subscribe(client_id, &[format!("channel{}", i)])
                    .await;
                client_id
            }));
        }

        let mut client_ids = vec![];
        for handle in handles {
            client_ids.push(handle.await.unwrap());
        }

        let stats = manager.stats();
        assert_eq!(stats.total_channels, 10);
        assert_eq!(stats.total_subscribers, 10);
    }
}
