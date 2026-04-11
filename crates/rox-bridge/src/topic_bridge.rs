//! TopicBridge — bridges Rox topics to/from external protocols.
//!
//! Translates between Rox's KeyExpr namespace and external protocols
//! (ROS 2, Zenoh) using configurable topic mappings.

use bytes::Bytes;
use std::collections::HashMap;
use tracing::debug;

use rox_protocol::{KeyExpr, NodeId, RoxMessage};

/// Direction of the bridge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BridgeDirection {
    /// Rox -> External
    Export,
    /// External -> Rox
    Import,
    /// Bidirectional
    Bidirectional,
}

/// A topic mapping rule.
#[derive(Clone, Debug)]
pub struct TopicMapping {
    /// Rox topic pattern.
    pub rox_topic: String,
    /// External topic name.
    pub external_topic: String,
    /// Bridge direction.
    pub direction: BridgeDirection,
}

/// Protocol bridge that translates between Rox and external systems.
pub struct TopicBridge {
    name: String,
    mappings: Vec<TopicMapping>,
    rox_to_external: HashMap<String, String>,
    external_to_rox: HashMap<String, String>,
    messages_bridged: u64,
}

impl TopicBridge {
    /// Create a new topic bridge.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            mappings: Vec::new(),
            rox_to_external: HashMap::new(),
            external_to_rox: HashMap::new(),
            messages_bridged: 0,
        }
    }

    /// Add a topic mapping.
    pub fn add_mapping(&mut self, mapping: TopicMapping) {
        match mapping.direction {
            BridgeDirection::Export | BridgeDirection::Bidirectional => {
                self.rox_to_external
                    .insert(mapping.rox_topic.clone(), mapping.external_topic.clone());
            }
            _ => {}
        }
        match mapping.direction {
            BridgeDirection::Import | BridgeDirection::Bidirectional => {
                self.external_to_rox
                    .insert(mapping.external_topic.clone(), mapping.rox_topic.clone());
            }
            _ => {}
        }
        self.mappings.push(mapping);
    }

    /// Translate a Rox topic to an external topic.
    pub fn rox_to_external(&self, rox_topic: &str) -> Option<&str> {
        self.rox_to_external.get(rox_topic).map(|s| s.as_str())
    }

    /// Translate an external topic to a Rox topic.
    pub fn external_to_rox(&self, external_topic: &str) -> Option<&str> {
        self.external_to_rox.get(external_topic).map(|s| s.as_str())
    }

    /// Bridge a Rox message to external format.
    /// Returns (external_topic, payload bytes).
    pub fn export_message(&mut self, msg: &RoxMessage) -> Option<(String, Bytes)> {
        let rox_topic = msg.header.key.as_str();
        let external_topic = self.rox_to_external.get(rox_topic)?.clone();

        self.messages_bridged += 1;
        debug!(
            from = rox_topic,
            to = %external_topic,
            "bridged message (export)"
        );

        Some((external_topic, msg.payload.clone()))
    }

    /// Bridge an external message into Rox format.
    pub fn import_message(
        &mut self,
        external_topic: &str,
        payload: Bytes,
        sequence: u64,
    ) -> Option<RoxMessage> {
        let rox_topic = self.external_to_rox.get(external_topic)?.clone();

        self.messages_bridged += 1;
        debug!(
            from = external_topic,
            to = %rox_topic,
            "bridged message (import)"
        );

        Some(RoxMessage::new(
            KeyExpr(rox_topic),
            NodeId(format!("bridge:{}", self.name)),
            sequence,
            payload,
        ))
    }

    /// Bridge name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Total messages bridged.
    pub fn messages_bridged(&self) -> u64 {
        self.messages_bridged
    }

    /// Number of topic mappings.
    pub fn mapping_count(&self) -> usize {
        self.mappings.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topic_mapping() {
        let mut bridge = TopicBridge::new("ros2");

        bridge.add_mapping(TopicMapping {
            rox_topic: "robot-01/lidar/scan".to_string(),
            external_topic: "/scan".to_string(),
            direction: BridgeDirection::Bidirectional,
        });

        assert_eq!(bridge.rox_to_external("robot-01/lidar/scan"), Some("/scan"));
        assert_eq!(bridge.external_to_rox("/scan"), Some("robot-01/lidar/scan"));
    }

    #[test]
    fn test_export_message() {
        let mut bridge = TopicBridge::new("zenoh");
        bridge.add_mapping(TopicMapping {
            rox_topic: "robot-01/motor/cmd".to_string(),
            external_topic: "zenoh/motor/cmd".to_string(),
            direction: BridgeDirection::Export,
        });

        let msg = RoxMessage::new(
            KeyExpr("robot-01/motor/cmd".to_string()),
            NodeId("planner".to_string()),
            0,
            Bytes::from(vec![1, 2, 3]),
        );

        let (topic, payload) = bridge.export_message(&msg).unwrap();
        assert_eq!(topic, "zenoh/motor/cmd");
        assert_eq!(payload.as_ref(), &[1, 2, 3]);
    }

    #[test]
    fn test_import_message() {
        let mut bridge = TopicBridge::new("ros2");
        bridge.add_mapping(TopicMapping {
            rox_topic: "robot-01/lidar/scan".to_string(),
            external_topic: "/scan".to_string(),
            direction: BridgeDirection::Import,
        });

        let msg = bridge
            .import_message("/scan", Bytes::from(vec![10, 20]), 5)
            .unwrap();

        assert_eq!(msg.header.key.as_str(), "robot-01/lidar/scan");
        assert_eq!(msg.payload.as_ref(), &[10, 20]);
        assert_eq!(msg.header.sequence, 5);
    }
}
