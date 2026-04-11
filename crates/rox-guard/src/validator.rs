//! CommandValidator — validates actuator commands against safety boundaries.
//!
//! Designed for <10us validation latency (branch-predicted, inline).

use crate::boundary::{BoundaryCheckResult, SafetyBoundary};
use rox_protocol::{KeyExpr, RoxMessage, ValidationResult};
use std::collections::HashMap;

/// Command validator with per-node safety schemas.
pub struct CommandValidator {
    boundaries: HashMap<String, SafetyBoundary>,
    total_validated: u64,
    total_rejected: u64,
    total_clamped: u64,
}

impl CommandValidator {
    /// Create a new validator with no boundaries.
    pub fn new() -> Self {
        Self {
            boundaries: HashMap::new(),
            total_validated: 0,
            total_rejected: 0,
            total_clamped: 0,
        }
    }

    /// Register a safety boundary for a node.
    pub fn add_boundary(&mut self, boundary: SafetyBoundary) {
        self.boundaries.insert(boundary.node_id.clone(), boundary);
    }

    /// Validate a command message.
    ///
    /// Extracts the target node from the topic key and checks against
    /// registered safety boundaries.
    #[inline]
    pub fn validate(&mut self, topic: &KeyExpr, msg: &RoxMessage) -> ValidationResult {
        self.total_validated += 1;

        // Extract node ID from topic (last segment before the port)
        let parts: Vec<&str> = topic.as_str().split('/').collect();
        let node_id = if parts.len() >= 2 {
            parts[parts.len() - 2]
        } else {
            return ValidationResult::Passed;
        };

        let boundary = match self.boundaries.get(node_id) {
            Some(b) => b,
            None => return ValidationResult::Passed, // No boundary = pass through
        };

        // Try to parse payload as a velocity command (simplified)
        // In real implementation, would use a schema-based approach
        if msg.payload.len() >= 8 {
            let velocity = f64::from_le_bytes(msg.payload[..8].try_into().unwrap_or([0u8; 8]));

            match boundary.check_velocity(velocity) {
                BoundaryCheckResult::Ok => {}
                BoundaryCheckResult::Exceeded {
                    field,
                    value,
                    clamped,
                    ..
                } => {
                    self.total_clamped += 1;
                    return ValidationResult::Clamped {
                        original: value,
                        clamped,
                        field,
                    };
                }
            }

            // Check acceleration if enough data
            if msg.payload.len() >= 16 {
                let accel = f64::from_le_bytes(msg.payload[8..16].try_into().unwrap_or([0u8; 8]));

                match boundary.check_acceleration(accel) {
                    BoundaryCheckResult::Ok => {}
                    BoundaryCheckResult::Exceeded {
                        field,
                        value,
                        clamped,
                        ..
                    } => {
                        self.total_clamped += 1;
                        return ValidationResult::Clamped {
                            original: value,
                            clamped,
                            field,
                        };
                    }
                }
            }
        }

        ValidationResult::Passed
    }

    /// Total commands validated.
    pub fn total_validated(&self) -> u64 {
        self.total_validated
    }

    /// Total commands rejected.
    pub fn total_rejected(&self) -> u64 {
        self.total_rejected
    }

    /// Total commands clamped.
    pub fn total_clamped(&self) -> u64 {
        self.total_clamped
    }
}

impl Default for CommandValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use rox_protocol::NodeId;

    #[test]
    fn test_pass_without_boundary() {
        let mut validator = CommandValidator::new();
        let key = KeyExpr::new("robot-01", "motor", "cmd");
        let msg = RoxMessage::new(
            key.clone(),
            NodeId("planner".to_string()),
            0,
            Bytes::from(1.0f64.to_le_bytes().to_vec()),
        );

        assert!(matches!(
            validator.validate(&key, &msg),
            ValidationResult::Passed
        ));
    }

    #[test]
    fn test_clamp_over_velocity() {
        let mut validator = CommandValidator::new();
        validator.add_boundary(SafetyBoundary {
            node_id: "motor".to_string(),
            max_velocity: Some(2.0),
            max_acceleration: None,
            max_torque: None,
            geofence: None,
        });

        let key = KeyExpr::new("robot-01", "motor", "cmd");
        let msg = RoxMessage::new(
            key.clone(),
            NodeId("planner".to_string()),
            0,
            Bytes::from(5.0f64.to_le_bytes().to_vec()),
        );

        match validator.validate(&key, &msg) {
            ValidationResult::Clamped {
                original,
                clamped,
                field,
            } => {
                assert_eq!(original, 5.0);
                assert_eq!(clamped, 2.0);
                assert_eq!(field, "velocity");
            }
            other => panic!("expected Clamped, got {:?}", other),
        }
    }
}
