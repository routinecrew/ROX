//! RoxMessage construction helpers.

use crate::contracts::*;
use bytes::Bytes;

impl RoxMessage {
    /// Create a new RoxMessage with the given key and payload.
    pub fn new(key: KeyExpr, source_id: NodeId, sequence: u64, payload: Bytes) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();

        Self {
            header: MessageHeader {
                key,
                timestamp: RoxTimestamp {
                    hw_time: now.as_nanos() as u64,
                    logical_time: sequence,
                },
                qos: QoSMetadata::default(),
                source_id,
                sequence,
            },
            payload,
        }
    }

    /// Create a RoxMessage with custom QoS settings.
    pub fn with_qos(mut self, qos: QoSMetadata) -> Self {
        self.header.qos = qos;
        self
    }

    /// Size of the message payload in bytes.
    pub fn payload_len(&self) -> usize {
        self.payload.len()
    }

    /// Total estimated wire size (header + payload).
    pub fn estimated_wire_size(&self) -> usize {
        // Rough estimate: header fields + payload
        std::mem::size_of::<MessageHeader>() + self.payload.len()
    }
}

impl MessageHeader {
    /// Check if the message has exceeded its deadline.
    pub fn is_expired(&self) -> bool {
        if let Some(lifespan_us) = self.qos.lifespan_us {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64;
            let msg_time_us = self.timestamp.hw_time / 1000; // nanos to micros
            now.saturating_sub(msg_time_us) > lifespan_us
        } else {
            false
        }
    }
}

impl RoxTimestamp {
    /// Create a timestamp for the current instant.
    pub fn now(logical_time: u64) -> Self {
        let hw_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        Self {
            hw_time,
            logical_time,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let msg = RoxMessage::new(
            KeyExpr::new("robot-01", "lidar", "scan"),
            NodeId("lidar_driver".to_string()),
            0,
            Bytes::from(vec![1u8, 2, 3]),
        );
        assert_eq!(msg.payload_len(), 3);
        assert_eq!(msg.header.sequence, 0);
        assert_eq!(msg.header.key.robot_id(), Some("robot-01"));
    }

    #[test]
    fn test_message_with_qos() {
        let msg = RoxMessage::new(
            KeyExpr::new("robot-01", "motor", "cmd"),
            NodeId("planner".to_string()),
            1,
            Bytes::new(),
        )
        .with_qos(QoSMetadata {
            priority: Priority::Highest,
            reliability: Reliability::Reliable,
            durability: Durability::Volatile,
            deadline_us: Some(10_000),
            lifespan_us: None,
        });
        assert_eq!(msg.header.qos.priority, Priority::Highest);
        assert_eq!(msg.header.qos.reliability, Reliability::Reliable);
    }

    #[test]
    fn test_timestamp_now() {
        let ts = RoxTimestamp::now(42);
        assert!(ts.hw_time > 0);
        assert_eq!(ts.logical_time, 42);
    }
}
