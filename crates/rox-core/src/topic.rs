//! TopicManager — concrete implementation of TopicRegistry.
//!
//! Manages Pub/Sub topics using tokio broadcast channels.
//! This is the real implementation used in production (replacing MockTopicRegistry).

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

use rox_protocol::{KeyExpr, MessageReceiver, MessageSender, QoSMetadata, TopicRegistry};

/// Active topic state.
struct Topic {
    sender: MessageSender,
    qos: QoSMetadata,
    publisher_count: usize,
    subscriber_count: usize,
}

/// Concrete TopicRegistry implementation.
///
/// Thread-safe topic manager that handles publisher/subscriber registration,
/// message routing via broadcast channels, and topic lifecycle.
pub struct TopicManager {
    topics: Arc<RwLock<HashMap<String, Topic>>>,
    channel_capacity: usize,
}

impl TopicManager {
    /// Create a new TopicManager with default channel capacity (1024).
    pub fn new() -> Self {
        Self {
            topics: Arc::new(RwLock::new(HashMap::new())),
            channel_capacity: 1024,
        }
    }

    /// Create with custom broadcast channel capacity.
    pub fn with_capacity(channel_capacity: usize) -> Self {
        Self {
            topics: Arc::new(RwLock::new(HashMap::new())),
            channel_capacity,
        }
    }

    /// Get the number of active topics.
    pub async fn topic_count(&self) -> usize {
        self.topics.read().await.len()
    }

    /// Get QoS metadata for a topic.
    pub async fn get_qos(&self, key: &KeyExpr) -> Option<QoSMetadata> {
        self.topics
            .read()
            .await
            .get(key.as_str())
            .map(|t| t.qos.clone())
    }

    /// Get subscriber count for a topic.
    pub async fn subscriber_count(&self, key: &KeyExpr) -> usize {
        self.topics
            .read()
            .await
            .get(key.as_str())
            .map(|t| t.subscriber_count)
            .unwrap_or(0)
    }

    /// Check if a topic exists.
    pub async fn has_topic(&self, key: &KeyExpr) -> bool {
        self.topics.read().await.contains_key(key.as_str())
    }

    /// Inject a message into the local topic bus from a remote transport.
    ///
    /// If the topic doesn't exist yet, it is auto-created (lazy creation).
    /// Returns the number of receivers that got the message.
    pub async fn inject(&self, msg: Arc<rox_protocol::RoxMessage>) -> Result<usize> {
        let key = &msg.header.key;
        let mut topics = self.topics.write().await;

        let topic = if let Some(t) = topics.get(key.as_str()) {
            t
        } else {
            // Auto-create topic on inject (remote publisher before local subscriber)
            let (sender, _) = tokio::sync::broadcast::channel(self.channel_capacity);
            topics.insert(
                key.as_str().to_string(),
                Topic {
                    sender,
                    qos: msg.header.qos.clone(),
                    publisher_count: 0,
                    subscriber_count: 0,
                },
            );
            debug!(topic = key.as_str(), "topic created on remote inject");
            topics.get(key.as_str()).expect("just inserted")
        };

        match topic.sender.send(msg) {
            Ok(count) => Ok(count),
            // No receivers is not an error for inject — message is simply dropped
            Err(_) => Ok(0),
        }
    }
}

impl Default for TopicManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TopicRegistry for TopicManager {
    async fn publish(&self, key: &KeyExpr, qos: QoSMetadata) -> Result<MessageSender> {
        let mut topics = self.topics.write().await;

        if let Some(topic) = topics.get_mut(key.as_str()) {
            topic.publisher_count += 1;
            debug!(
                topic = key.as_str(),
                publishers = topic.publisher_count,
                "publisher added"
            );
            Ok(topic.sender.clone())
        } else {
            let (sender, _) = tokio::sync::broadcast::channel(self.channel_capacity);
            let tx = sender.clone();
            topics.insert(
                key.as_str().to_string(),
                Topic {
                    sender,
                    qos,
                    publisher_count: 1,
                    subscriber_count: 0,
                },
            );
            info!(topic = key.as_str(), "topic created");
            Ok(tx)
        }
    }

    async fn subscribe(&self, key: &KeyExpr) -> Result<MessageReceiver> {
        let mut topics = self.topics.write().await;

        if let Some(topic) = topics.get_mut(key.as_str()) {
            topic.subscriber_count += 1;
            debug!(
                topic = key.as_str(),
                subscribers = topic.subscriber_count,
                "subscriber added"
            );
            Ok(topic.sender.subscribe())
        } else {
            // Auto-create topic on subscribe (lazy creation)
            let (sender, _) = tokio::sync::broadcast::channel(self.channel_capacity);
            let rx = sender.subscribe();
            topics.insert(
                key.as_str().to_string(),
                Topic {
                    sender,
                    qos: QoSMetadata::default(),
                    publisher_count: 0,
                    subscriber_count: 1,
                },
            );
            info!(topic = key.as_str(), "topic created on subscribe");
            Ok(rx)
        }
    }

    async fn unpublish(&self, key: &KeyExpr) -> Result<()> {
        let mut topics = self.topics.write().await;

        if let Some(topic) = topics.get_mut(key.as_str()) {
            topic.publisher_count = topic.publisher_count.saturating_sub(1);
            debug!(
                topic = key.as_str(),
                publishers = topic.publisher_count,
                "publisher removed"
            );

            // Remove topic if no publishers and no subscribers
            if topic.publisher_count == 0 && topic.subscriber_count == 0 {
                topics.remove(key.as_str());
                info!(
                    topic = key.as_str(),
                    "topic removed (no publishers/subscribers)"
                );
            }
        }
        Ok(())
    }

    async fn list_topics(&self) -> Vec<KeyExpr> {
        self.topics
            .read()
            .await
            .keys()
            .map(|k| KeyExpr(k.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use rox_protocol::*;

    #[tokio::test]
    async fn test_publish_subscribe() {
        let manager = TopicManager::new();
        let key = KeyExpr::new("robot-01", "lidar", "scan");

        let tx = manager.publish(&key, QoSMetadata::default()).await.unwrap();
        let mut rx = manager.subscribe(&key).await.unwrap();

        let msg = Arc::new(RoxMessage::new(
            key.clone(),
            NodeId("driver".to_string()),
            0,
            Bytes::from(vec![1, 2, 3]),
        ));

        tx.send(msg.clone()).unwrap();
        let received = rx.recv().await.unwrap();
        assert_eq!(received.header.key.as_str(), "robot-01/lidar/scan");
    }

    #[tokio::test]
    async fn test_topic_lifecycle() {
        let manager = TopicManager::new();
        let key = KeyExpr::new("robot-01", "motor", "cmd");

        assert_eq!(manager.topic_count().await, 0);

        let _tx = manager.publish(&key, QoSMetadata::default()).await.unwrap();
        assert_eq!(manager.topic_count().await, 1);
        assert!(manager.has_topic(&key).await);

        manager.unpublish(&key).await.unwrap();
        assert_eq!(manager.topic_count().await, 0);
    }

    #[tokio::test]
    async fn test_list_topics() {
        let manager = TopicManager::new();

        let k1 = KeyExpr::new("robot-01", "lidar", "scan");
        let k2 = KeyExpr::new("robot-01", "motor", "cmd");

        let _tx1 = manager.publish(&k1, QoSMetadata::default()).await.unwrap();
        let _tx2 = manager.publish(&k2, QoSMetadata::default()).await.unwrap();

        let topics = manager.list_topics().await;
        assert_eq!(topics.len(), 2);
    }

    use std::sync::Arc;

    #[tokio::test]
    async fn test_inject_creates_topic_and_delivers() {
        let manager = TopicManager::new();
        let key = KeyExpr::new("robot-02", "arm", "joint");

        // Subscribe first
        let mut rx = manager.subscribe(&key).await.unwrap();

        // Inject a message (simulating remote transport delivery)
        let msg = Arc::new(RoxMessage::new(
            key.clone(),
            NodeId("remote-node".to_string()),
            7,
            Bytes::from(vec![0xAB, 0xCD]),
        ));
        let receivers = manager.inject(msg.clone()).await.unwrap();
        assert_eq!(receivers, 1);

        // Verify subscriber received it
        let received = rx.recv().await.unwrap();
        assert_eq!(received.header.key.as_str(), "robot-02/arm/joint");
        assert_eq!(received.header.source_id.0, "remote-node");
        assert_eq!(received.payload.as_ref(), &[0xAB, 0xCD]);
    }

    #[tokio::test]
    async fn test_inject_auto_creates_topic() {
        let manager = TopicManager::new();
        let key = KeyExpr::new("robot-03", "gripper", "force");

        assert!(!manager.has_topic(&key).await);

        // Inject before any subscriber — topic auto-created, message dropped (no receivers)
        let msg = Arc::new(RoxMessage::new(
            key.clone(),
            NodeId("remote".to_string()),
            0,
            Bytes::from(vec![1]),
        ));
        let receivers = manager.inject(msg).await.unwrap();
        assert_eq!(receivers, 0);
        assert!(manager.has_topic(&key).await);
    }
}
