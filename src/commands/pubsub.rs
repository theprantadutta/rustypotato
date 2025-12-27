//! Pub/Sub command implementations (SUBSCRIBE, UNSUBSCRIBE, PUBLISH, PSUBSCRIBE, PUNSUBSCRIBE)

use crate::commands::{Command, CommandArity, CommandResult, ResponseValue};
use crate::pubsub::PubSubManager;
use crate::storage::MemoryStore;
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

/// SUBSCRIBE command implementation
/// Subscribe to one or more channels
pub struct SubscribeCommand {
    pub pubsub: Arc<PubSubManager>,
}

impl SubscribeCommand {
    pub fn new(pubsub: Arc<PubSubManager>) -> Self {
        Self { pubsub }
    }
}

#[async_trait]
impl Command for SubscribeCommand {
    async fn execute(
        &self,
        args: &[String],
        _store: &MemoryStore,
        client_id: Uuid,
    ) -> CommandResult {
        if args.is_empty() {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'SUBSCRIBE' command".to_string(),
            );
        }

        let results = self.pubsub.subscribe(client_id, args).await;

        // Build response array with subscription confirmations
        // Each subscription returns ["subscribe", channel, count]
        let mut responses = Vec::new();
        for (channel, count) in results {
            responses.push(ResponseValue::Array(vec![
                ResponseValue::BulkString(Some("subscribe".to_string())),
                ResponseValue::BulkString(Some(channel)),
                ResponseValue::Integer(count as i64),
            ]));
        }

        // Return the first confirmation (client should receive push messages for the rest)
        if responses.len() == 1 {
            CommandResult::Ok(responses.remove(0))
        } else {
            CommandResult::Ok(ResponseValue::Array(responses))
        }
    }

    fn name(&self) -> &'static str {
        "SUBSCRIBE"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::AtLeast(2) // SUBSCRIBE channel [channel ...]
    }
}

/// UNSUBSCRIBE command implementation
/// Unsubscribe from one or more channels
pub struct UnsubscribeCommand {
    pub pubsub: Arc<PubSubManager>,
}

impl UnsubscribeCommand {
    pub fn new(pubsub: Arc<PubSubManager>) -> Self {
        Self { pubsub }
    }
}

#[async_trait]
impl Command for UnsubscribeCommand {
    async fn execute(
        &self,
        args: &[String],
        _store: &MemoryStore,
        client_id: Uuid,
    ) -> CommandResult {
        let results = self.pubsub.unsubscribe(client_id, args).await;

        // If no channels were specified and client wasn't subscribed, return default response
        if results.is_empty() {
            return CommandResult::Ok(ResponseValue::Array(vec![
                ResponseValue::BulkString(Some("unsubscribe".to_string())),
                ResponseValue::BulkString(None),
                ResponseValue::Integer(0),
            ]));
        }

        // Build response array with unsubscription confirmations
        let mut responses = Vec::new();
        for (channel, count) in results {
            responses.push(ResponseValue::Array(vec![
                ResponseValue::BulkString(Some("unsubscribe".to_string())),
                ResponseValue::BulkString(Some(channel)),
                ResponseValue::Integer(count as i64),
            ]));
        }

        if responses.len() == 1 {
            CommandResult::Ok(responses.remove(0))
        } else {
            CommandResult::Ok(ResponseValue::Array(responses))
        }
    }

    fn name(&self) -> &'static str {
        "UNSUBSCRIBE"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::AtLeast(1) // UNSUBSCRIBE [channel ...]
    }
}

/// PUBLISH command implementation
/// Post a message to a channel
pub struct PublishCommand {
    pub pubsub: Arc<PubSubManager>,
}

impl PublishCommand {
    pub fn new(pubsub: Arc<PubSubManager>) -> Self {
        Self { pubsub }
    }
}

#[async_trait]
impl Command for PublishCommand {
    async fn execute(
        &self,
        args: &[String],
        _store: &MemoryStore,
        _client_id: Uuid,
    ) -> CommandResult {
        if args.len() != 2 {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'PUBLISH' command".to_string(),
            );
        }

        let channel = &args[0];
        let message = &args[1];

        let receivers = self.pubsub.publish(channel, message).await;

        CommandResult::Ok(ResponseValue::Integer(receivers as i64))
    }

    fn name(&self) -> &'static str {
        "PUBLISH"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::Fixed(3) // PUBLISH channel message
    }
}

/// PSUBSCRIBE command implementation
/// Subscribe to one or more patterns
pub struct PsubscribeCommand {
    pub pubsub: Arc<PubSubManager>,
}

impl PsubscribeCommand {
    pub fn new(pubsub: Arc<PubSubManager>) -> Self {
        Self { pubsub }
    }
}

#[async_trait]
impl Command for PsubscribeCommand {
    async fn execute(
        &self,
        args: &[String],
        _store: &MemoryStore,
        client_id: Uuid,
    ) -> CommandResult {
        if args.is_empty() {
            return CommandResult::Error(
                "ERR wrong number of arguments for 'PSUBSCRIBE' command".to_string(),
            );
        }

        let results = self.pubsub.psubscribe(client_id, args).await;

        // Build response array with subscription confirmations
        let mut responses = Vec::new();
        for (pattern, count) in results {
            responses.push(ResponseValue::Array(vec![
                ResponseValue::BulkString(Some("psubscribe".to_string())),
                ResponseValue::BulkString(Some(pattern)),
                ResponseValue::Integer(count as i64),
            ]));
        }

        if responses.len() == 1 {
            CommandResult::Ok(responses.remove(0))
        } else {
            CommandResult::Ok(ResponseValue::Array(responses))
        }
    }

    fn name(&self) -> &'static str {
        "PSUBSCRIBE"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::AtLeast(2) // PSUBSCRIBE pattern [pattern ...]
    }
}

/// PUNSUBSCRIBE command implementation
/// Unsubscribe from one or more patterns
pub struct PunsubscribeCommand {
    pub pubsub: Arc<PubSubManager>,
}

impl PunsubscribeCommand {
    pub fn new(pubsub: Arc<PubSubManager>) -> Self {
        Self { pubsub }
    }
}

#[async_trait]
impl Command for PunsubscribeCommand {
    async fn execute(
        &self,
        args: &[String],
        _store: &MemoryStore,
        client_id: Uuid,
    ) -> CommandResult {
        let results = self.pubsub.punsubscribe(client_id, args).await;

        // If no patterns were specified and client wasn't subscribed, return default response
        if results.is_empty() {
            return CommandResult::Ok(ResponseValue::Array(vec![
                ResponseValue::BulkString(Some("punsubscribe".to_string())),
                ResponseValue::BulkString(None),
                ResponseValue::Integer(0),
            ]));
        }

        // Build response array with unsubscription confirmations
        let mut responses = Vec::new();
        for (pattern, count) in results {
            responses.push(ResponseValue::Array(vec![
                ResponseValue::BulkString(Some("punsubscribe".to_string())),
                ResponseValue::BulkString(Some(pattern)),
                ResponseValue::Integer(count as i64),
            ]));
        }

        if responses.len() == 1 {
            CommandResult::Ok(responses.remove(0))
        } else {
            CommandResult::Ok(ResponseValue::Array(responses))
        }
    }

    fn name(&self) -> &'static str {
        "PUNSUBSCRIBE"
    }

    fn arity(&self) -> CommandArity {
        CommandArity::AtLeast(1) // PUNSUBSCRIBE [pattern ...]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStore;

    fn create_test_store() -> MemoryStore {
        MemoryStore::new()
    }

    fn create_test_pubsub() -> Arc<PubSubManager> {
        Arc::new(PubSubManager::new())
    }

    fn test_client_id() -> Uuid {
        Uuid::new_v4()
    }

    // ==================== SUBSCRIBE Command Tests ====================

    #[tokio::test]
    async fn test_subscribe_command_single_channel() {
        let store = create_test_store();
        let pubsub = create_test_pubsub();
        let cmd = SubscribeCommand::new(pubsub);
        let client_id = test_client_id();
        let args = vec!["news".to_string()];

        let result = cmd.execute(&args, &store, client_id).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(arr)) => {
                assert_eq!(arr.len(), 3);
                assert!(matches!(&arr[0], ResponseValue::BulkString(Some(s)) if s == "subscribe"));
                assert!(matches!(&arr[1], ResponseValue::BulkString(Some(s)) if s == "news"));
                assert!(matches!(&arr[2], ResponseValue::Integer(1)));
            }
            _ => panic!("Expected array response"),
        }
    }

    #[tokio::test]
    async fn test_subscribe_command_multiple_channels() {
        let store = create_test_store();
        let pubsub = create_test_pubsub();
        let cmd = SubscribeCommand::new(pubsub);
        let client_id = test_client_id();
        let args = vec!["news".to_string(), "sports".to_string()];

        let result = cmd.execute(&args, &store, client_id).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(arr)) => {
                // Should have 2 confirmation arrays
                assert_eq!(arr.len(), 2);
            }
            _ => panic!("Expected array response"),
        }
    }

    #[tokio::test]
    async fn test_subscribe_command_no_args() {
        let store = create_test_store();
        let pubsub = create_test_pubsub();
        let cmd = SubscribeCommand::new(pubsub);
        let client_id = test_client_id();
        let args: Vec<String> = vec![];

        let result = cmd.execute(&args, &store, client_id).await;

        assert!(matches!(result, CommandResult::Error(ref s) if s.contains("wrong number")));
    }

    #[tokio::test]
    async fn test_subscribe_command_properties() {
        let pubsub = create_test_pubsub();
        let cmd = SubscribeCommand::new(pubsub);
        assert_eq!(cmd.name(), "SUBSCRIBE");
        assert_eq!(cmd.arity(), CommandArity::AtLeast(2));
    }

    // ==================== UNSUBSCRIBE Command Tests ====================

    #[tokio::test]
    async fn test_unsubscribe_command_from_channel() {
        let store = create_test_store();
        let pubsub = create_test_pubsub();
        let client_id = test_client_id();

        // First subscribe
        pubsub.subscribe(client_id, &["news".to_string()]).await;

        let cmd = UnsubscribeCommand::new(Arc::clone(&pubsub));
        let args = vec!["news".to_string()];

        let result = cmd.execute(&args, &store, client_id).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(arr)) => {
                assert_eq!(arr.len(), 3);
                assert!(matches!(&arr[0], ResponseValue::BulkString(Some(s)) if s == "unsubscribe"));
                assert!(matches!(&arr[1], ResponseValue::BulkString(Some(s)) if s == "news"));
                assert!(matches!(&arr[2], ResponseValue::Integer(0)));
            }
            _ => panic!("Expected array response"),
        }
    }

    #[tokio::test]
    async fn test_unsubscribe_command_all_channels() {
        let store = create_test_store();
        let pubsub = create_test_pubsub();
        let client_id = test_client_id();

        // Subscribe to multiple channels
        pubsub
            .subscribe(client_id, &["news".to_string(), "sports".to_string()])
            .await;

        let cmd = UnsubscribeCommand::new(Arc::clone(&pubsub));
        let args: Vec<String> = vec![]; // Empty = unsubscribe from all

        let result = cmd.execute(&args, &store, client_id).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(_)) => {
                // Should unsubscribe from both
            }
            _ => panic!("Expected array response"),
        }

        // Verify client is no longer subscribed
        assert!(!pubsub.is_subscriber(client_id).await);
    }

    #[tokio::test]
    async fn test_unsubscribe_command_properties() {
        let pubsub = create_test_pubsub();
        let cmd = UnsubscribeCommand::new(pubsub);
        assert_eq!(cmd.name(), "UNSUBSCRIBE");
        assert_eq!(cmd.arity(), CommandArity::AtLeast(1));
    }

    // ==================== PUBLISH Command Tests ====================

    #[tokio::test]
    async fn test_publish_command_no_subscribers() {
        let store = create_test_store();
        let pubsub = create_test_pubsub();
        let cmd = PublishCommand::new(pubsub);
        let client_id = test_client_id();
        let args = vec!["news".to_string(), "Hello!".to_string()];

        let result = cmd.execute(&args, &store, client_id).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 0); // No subscribers
            }
            _ => panic!("Expected integer response"),
        }
    }

    #[tokio::test]
    async fn test_publish_command_with_subscriber() {
        let store = create_test_store();
        let pubsub = create_test_pubsub();
        let subscriber_id = Uuid::new_v4();
        let publisher_id = test_client_id();

        // Subscribe a client
        pubsub.subscribe(subscriber_id, &["news".to_string()]).await;

        let cmd = PublishCommand::new(Arc::clone(&pubsub));
        let args = vec!["news".to_string(), "Hello!".to_string()];

        let result = cmd.execute(&args, &store, publisher_id).await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 1); // One subscriber
            }
            _ => panic!("Expected integer response"),
        }
    }

    #[tokio::test]
    async fn test_publish_command_wrong_args() {
        let store = create_test_store();
        let pubsub = create_test_pubsub();
        let cmd = PublishCommand::new(pubsub);
        let client_id = test_client_id();
        let args = vec!["news".to_string()]; // Missing message

        let result = cmd.execute(&args, &store, client_id).await;

        assert!(matches!(result, CommandResult::Error(ref s) if s.contains("wrong number")));
    }

    #[tokio::test]
    async fn test_publish_command_properties() {
        let pubsub = create_test_pubsub();
        let cmd = PublishCommand::new(pubsub);
        assert_eq!(cmd.name(), "PUBLISH");
        assert_eq!(cmd.arity(), CommandArity::Fixed(3));
    }

    // ==================== PSUBSCRIBE Command Tests ====================

    #[tokio::test]
    async fn test_psubscribe_command_single_pattern() {
        let store = create_test_store();
        let pubsub = create_test_pubsub();
        let cmd = PsubscribeCommand::new(pubsub);
        let client_id = test_client_id();
        let args = vec!["news.*".to_string()];

        let result = cmd.execute(&args, &store, client_id).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(arr)) => {
                assert_eq!(arr.len(), 3);
                assert!(matches!(&arr[0], ResponseValue::BulkString(Some(s)) if s == "psubscribe"));
                assert!(matches!(&arr[1], ResponseValue::BulkString(Some(s)) if s == "news.*"));
                assert!(matches!(&arr[2], ResponseValue::Integer(1)));
            }
            _ => panic!("Expected array response"),
        }
    }

    #[tokio::test]
    async fn test_psubscribe_command_no_args() {
        let store = create_test_store();
        let pubsub = create_test_pubsub();
        let cmd = PsubscribeCommand::new(pubsub);
        let client_id = test_client_id();
        let args: Vec<String> = vec![];

        let result = cmd.execute(&args, &store, client_id).await;

        assert!(matches!(result, CommandResult::Error(ref s) if s.contains("wrong number")));
    }

    #[tokio::test]
    async fn test_psubscribe_command_properties() {
        let pubsub = create_test_pubsub();
        let cmd = PsubscribeCommand::new(pubsub);
        assert_eq!(cmd.name(), "PSUBSCRIBE");
        assert_eq!(cmd.arity(), CommandArity::AtLeast(2));
    }

    // ==================== PUNSUBSCRIBE Command Tests ====================

    #[tokio::test]
    async fn test_punsubscribe_command_from_pattern() {
        let store = create_test_store();
        let pubsub = create_test_pubsub();
        let client_id = test_client_id();

        // First psubscribe
        pubsub.psubscribe(client_id, &["news.*".to_string()]).await;

        let cmd = PunsubscribeCommand::new(Arc::clone(&pubsub));
        let args = vec!["news.*".to_string()];

        let result = cmd.execute(&args, &store, client_id).await;

        match result {
            CommandResult::Ok(ResponseValue::Array(arr)) => {
                assert_eq!(arr.len(), 3);
                assert!(
                    matches!(&arr[0], ResponseValue::BulkString(Some(s)) if s == "punsubscribe")
                );
            }
            _ => panic!("Expected array response"),
        }
    }

    #[tokio::test]
    async fn test_punsubscribe_command_properties() {
        let pubsub = create_test_pubsub();
        let cmd = PunsubscribeCommand::new(pubsub);
        assert_eq!(cmd.name(), "PUNSUBSCRIBE");
        assert_eq!(cmd.arity(), CommandArity::AtLeast(1));
    }

    // ==================== Integration Tests ====================

    #[tokio::test]
    async fn test_pubsub_workflow() {
        let store = create_test_store();
        let pubsub = create_test_pubsub();
        let subscriber_id = Uuid::new_v4();
        let publisher_id = Uuid::new_v4();

        // Get a message receiver before subscribing
        let subscribe_cmd = SubscribeCommand::new(Arc::clone(&pubsub));
        let _ = subscribe_cmd
            .execute(&["events".to_string()], &store, subscriber_id)
            .await;

        // Get the subscriber to receive messages
        let subscriber = pubsub.get_subscriber(subscriber_id).unwrap();
        let mut receiver = subscriber.subscribe_messages();

        // Publish a message
        let publish_cmd = PublishCommand::new(Arc::clone(&pubsub));
        let result = publish_cmd
            .execute(
                &["events".to_string(), "Hello, world!".to_string()],
                &store,
                publisher_id,
            )
            .await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 1);
            }
            _ => panic!("Expected integer response"),
        }

        // Verify message was received
        let msg = receiver.try_recv().unwrap();
        assert_eq!(msg.channel, "events");
        assert_eq!(msg.payload, "Hello, world!");
    }

    #[tokio::test]
    async fn test_pattern_subscription_workflow() {
        let store = create_test_store();
        let pubsub = create_test_pubsub();
        let subscriber_id = Uuid::new_v4();
        let publisher_id = Uuid::new_v4();

        // Subscribe to pattern
        let psubscribe_cmd = PsubscribeCommand::new(Arc::clone(&pubsub));
        let _ = psubscribe_cmd
            .execute(&["events.*".to_string()], &store, subscriber_id)
            .await;

        // Get the subscriber to receive messages
        let subscriber = pubsub.get_subscriber(subscriber_id).unwrap();
        let mut receiver = subscriber.subscribe_messages();

        // Publish to matching channel
        let publish_cmd = PublishCommand::new(Arc::clone(&pubsub));
        let result = publish_cmd
            .execute(
                &["events.user.login".to_string(), "User logged in".to_string()],
                &store,
                publisher_id,
            )
            .await;

        match result {
            CommandResult::Ok(ResponseValue::Integer(count)) => {
                assert_eq!(count, 1);
            }
            _ => panic!("Expected integer response"),
        }

        // Verify message was received with pattern info
        let msg = receiver.try_recv().unwrap();
        assert_eq!(msg.channel, "events.user.login");
        assert_eq!(msg.payload, "User logged in");
        assert_eq!(msg.pattern, Some("events.*".to_string()));
    }
}
