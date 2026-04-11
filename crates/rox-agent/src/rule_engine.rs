//! Rule Engine — maps agent events to policy updates.

use rox_protocol::{AgentEvent, AlertLevel, PolicyUpdateType, VersionedPolicyUpdate};

/// Rule engine that converts agent events into policy update proposals.
pub struct RuleEngine {
    next_version: u64,
    current_cycle: u64,
}

impl RuleEngine {
    /// Create a new rule engine.
    pub fn new() -> Self {
        Self {
            next_version: 1,
            current_cycle: 0,
        }
    }

    /// Set the current cycle for versioned policy updates.
    pub fn set_cycle(&mut self, cycle: u64) {
        self.current_cycle = cycle;
    }

    /// Evaluate an agent event and optionally produce a policy update.
    pub fn evaluate(&mut self, event: &AgentEvent) -> Option<VersionedPolicyUpdate> {
        let update = match event {
            AgentEvent::CongestionWarning {
                topic,
                probability,
                recommended_priority,
            } => {
                if *probability > 0.9 {
                    Some(PolicyUpdateType::UpdateQoS {
                        topic: topic.clone(),
                        new_priority: *recommended_priority,
                    })
                } else {
                    Some(PolicyUpdateType::Alert {
                        level: AlertLevel::Warning,
                        message: format!(
                            "congestion warning on {}: {:.0}%",
                            topic,
                            probability * 100.0
                        ),
                    })
                }
            }

            AgentEvent::FailoverActivated {
                failed_node,
                backup_route,
            } => Some(PolicyUpdateType::CreateFailover {
                failed_node: failed_node.clone(),
                backup_route: backup_route.clone(),
            }),

            AgentEvent::AnomalyDetected {
                topic,
                anomaly_type,
                severity,
            } => {
                let level = if *severity > 0.8 {
                    AlertLevel::Critical
                } else if *severity > 0.5 {
                    AlertLevel::Warning
                } else {
                    AlertLevel::Info
                };
                Some(PolicyUpdateType::Alert {
                    level,
                    message: format!("{anomaly_type} on {topic} (severity: {severity:.2})"),
                })
            }

            AgentEvent::ThrottleApplied {
                topic,
                new_interval_ms,
            } => Some(PolicyUpdateType::ThrottleTopic {
                topic: topic.clone(),
                new_interval_ms: *new_interval_ms,
            }),

            AgentEvent::NodeDegraded { node_id } => Some(PolicyUpdateType::Alert {
                level: AlertLevel::Warning,
                message: format!("node {node_id} degraded"),
            }),

            AgentEvent::PolicyApplied { .. } => None,
        };

        update.map(|u| {
            let version = self.next_version;
            self.next_version += 1;
            VersionedPolicyUpdate {
                version,
                apply_at_cycle: self.current_cycle + 1,
                confidence: 0.8,
                update: u,
            }
        })
    }
}

impl Default for RuleEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rox_protocol::{KeyExpr, Priority};

    #[test]
    fn test_congestion_to_policy() {
        let mut engine = RuleEngine::new();
        engine.set_cycle(10);

        let event = AgentEvent::CongestionWarning {
            topic: KeyExpr("robot/lidar/scan".to_string()),
            probability: 0.95,
            recommended_priority: Priority::Highest,
        };

        let update = engine.evaluate(&event).unwrap();
        assert_eq!(update.version, 1);
        assert_eq!(update.apply_at_cycle, 11);
        match update.update {
            PolicyUpdateType::UpdateQoS { new_priority, .. } => {
                assert_eq!(new_priority, Priority::Highest);
            }
            _ => panic!("expected UpdateQoS"),
        }
    }

    #[test]
    fn test_anomaly_to_alert() {
        let mut engine = RuleEngine::new();

        let event = AgentEvent::AnomalyDetected {
            topic: KeyExpr("robot/motor/cmd".to_string()),
            anomaly_type: "latency_spike".to_string(),
            severity: 0.9,
        };

        let update = engine.evaluate(&event).unwrap();
        match update.update {
            PolicyUpdateType::Alert { level, .. } => {
                assert_eq!(level, AlertLevel::Critical);
            }
            _ => panic!("expected Alert"),
        }
    }
}
