//! Session — zenoh-inspired session management.
//!
//! A Session is the main entry point for interacting with the Rox runtime.
//! It owns the TopicManager and provides high-level Pub/Sub APIs.

use anyhow::Result;
use std::sync::Arc;
use tracing::info;

use crate::topic::TopicManager;
use rox_protocol::{KeyExpr, MessageReceiver, MessageSender, QoSMetadata, TopicRegistry};

/// A Rox session — the main entry point for application code.
///
/// Inspired by zenoh's Session: provides publisher/subscriber creation
/// and manages the lifecycle of topics.
pub struct Session {
    topic_manager: Arc<TopicManager>,
    session_id: String,
}

impl Session {
    /// Create a new session with a given ID.
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            topic_manager: Arc::new(TopicManager::new()),
            session_id: session_id.into(),
        }
    }

    /// Create a session with a custom TopicManager.
    pub fn with_topic_manager(session_id: impl Into<String>, manager: Arc<TopicManager>) -> Self {
        Self {
            topic_manager: manager,
            session_id: session_id.into(),
        }
    }

    /// Session identifier.
    pub fn id(&self) -> &str {
        &self.session_id
    }

    /// Declare a publisher on a topic.
    pub async fn declare_publisher(&self, key: &KeyExpr, qos: QoSMetadata) -> Result<Publisher> {
        let sender = self.topic_manager.publish(key, qos).await?;
        Ok(Publisher {
            key: key.clone(),
            sender,
        })
    }

    /// Declare a subscriber on a topic.
    pub async fn declare_subscriber(&self, key: &KeyExpr) -> Result<Subscriber> {
        let receiver = self.topic_manager.subscribe(key).await?;
        Ok(Subscriber {
            key: key.clone(),
            receiver,
        })
    }

    /// Undeclare (remove) a publisher.
    pub async fn undeclare_publisher(&self, key: &KeyExpr) -> Result<()> {
        self.topic_manager.unpublish(key).await
    }

    /// List all active topics.
    pub async fn list_topics(&self) -> Vec<KeyExpr> {
        self.topic_manager.list_topics().await
    }

    /// Get a reference to the underlying TopicManager.
    pub fn topic_manager(&self) -> &Arc<TopicManager> {
        &self.topic_manager
    }

    /// Close the session.
    pub async fn close(self) -> Result<()> {
        info!(session = %self.session_id, "session closed");
        Ok(())
    }
}

/// A publisher bound to a specific topic.
pub struct Publisher {
    key: KeyExpr,
    sender: MessageSender,
}

impl Publisher {
    /// The topic key this publisher is bound to.
    pub fn key(&self) -> &KeyExpr {
        &self.key
    }

    /// Send a message. Returns the number of active receivers.
    pub fn send(&self, msg: Arc<rox_protocol::RoxMessage>) -> Result<usize> {
        self.sender
            .send(msg)
            .map_err(|_| anyhow::anyhow!("no active subscribers for {}", self.key))
    }

    /// Number of active subscribers on this topic.
    pub fn receiver_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

/// A subscriber bound to a specific topic.
pub struct Subscriber {
    key: KeyExpr,
    receiver: MessageReceiver,
}

impl Subscriber {
    /// The topic key this subscriber is listening on.
    pub fn key(&self) -> &KeyExpr {
        &self.key
    }

    /// Receive the next message (async, blocks until available).
    pub async fn recv(&mut self) -> Result<Arc<rox_protocol::RoxMessage>> {
        self.receiver
            .recv()
            .await
            .map_err(|e| anyhow::anyhow!("recv error on {}: {e}", self.key))
    }

    /// Try to receive a message without blocking.
    pub fn try_recv(&mut self) -> Result<Option<Arc<rox_protocol::RoxMessage>>> {
        match self.receiver.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("try_recv error on {}: {e}", self.key)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use rox_protocol::{NodeId, RoxMessage};

    #[tokio::test]
    async fn test_session_pubsub() {
        let session = Session::new("test-session");
        let key = KeyExpr::new("robot-01", "lidar", "scan");

        let publisher = session
            .declare_publisher(&key, QoSMetadata::default())
            .await
            .unwrap();

        let mut subscriber = session.declare_subscriber(&key).await.unwrap();

        let msg = Arc::new(RoxMessage::new(
            key.clone(),
            NodeId("driver".to_string()),
            0,
            Bytes::from(vec![42]),
        ));

        publisher.send(msg.clone()).unwrap();
        let received = subscriber.recv().await.unwrap();
        assert_eq!(received.payload.as_ref(), &[42]);
    }

    #[tokio::test]
    async fn test_session_list_topics() {
        let session = Session::new("test");
        let k1 = KeyExpr::new("r1", "lidar", "scan");
        let k2 = KeyExpr::new("r1", "motor", "cmd");

        let _p1 = session
            .declare_publisher(&k1, QoSMetadata::default())
            .await
            .unwrap();
        let _p2 = session
            .declare_publisher(&k2, QoSMetadata::default())
            .await
            .unwrap();

        let topics = session.list_topics().await;
        assert_eq!(topics.len(), 2);
    }
}
