//! Throttle Agent — frequency limiting for bandwidth savings.
//! Replaces the original "Semantic Pruning" concept from v2.

use rox_protocol::{AgentEvent, KeyExpr, TransportMetrics};
use std::collections::HashMap;

/// Per-topic throttle state.
#[derive(Clone, Debug)]
struct TopicThrottleState {
    current_interval_ms: u64,
    target_bandwidth_bps: u64,
    recent_bandwidth_bps: u64,
    message_count: u64,
}

/// Throttle agent that reduces message frequency to save bandwidth.
pub struct ThrottleAgent {
    topic_states: HashMap<String, TopicThrottleState>,
    /// Max bandwidth per topic (default: unlimited).
    default_max_bandwidth_bps: u64,
    /// Minimum interval between messages (ms).
    min_interval_ms: u64,
}

impl ThrottleAgent {
    /// Create with default settings.
    pub fn new() -> Self {
        Self {
            topic_states: HashMap::new(),
            default_max_bandwidth_bps: u64::MAX,
            min_interval_ms: 10,
        }
    }

    /// Set a bandwidth limit for a specific topic.
    pub fn set_topic_limit(&mut self, topic: &str, max_bandwidth_bps: u64) {
        let state = self
            .topic_states
            .entry(topic.to_string())
            .or_insert_with(|| TopicThrottleState {
                current_interval_ms: 0,
                target_bandwidth_bps: max_bandwidth_bps,
                recent_bandwidth_bps: 0,
                message_count: 0,
            });
        state.target_bandwidth_bps = max_bandwidth_bps;
    }

    /// Observe a metric data point.
    pub fn observe(&mut self, metrics: &TransportMetrics) {
        let state = self
            .topic_states
            .entry(metrics.topic.as_str().to_string())
            .or_insert_with(|| TopicThrottleState {
                current_interval_ms: 0,
                target_bandwidth_bps: self.default_max_bandwidth_bps,
                recent_bandwidth_bps: 0,
                message_count: 0,
            });
        state.recent_bandwidth_bps = metrics.throughput_bps;
        state.message_count += 1;
    }

    /// Evaluate and produce throttle events.
    pub fn evaluate(&mut self) -> Vec<AgentEvent> {
        let mut events = Vec::new();

        for (topic_key, state) in &mut self.topic_states {
            if state.target_bandwidth_bps == u64::MAX {
                continue; // No limit set
            }

            if state.recent_bandwidth_bps > state.target_bandwidth_bps {
                // Calculate needed interval to fit within bandwidth limit
                let ratio = state.recent_bandwidth_bps as f64 / state.target_bandwidth_bps as f64;
                let new_interval = (state.current_interval_ms as f64 * ratio)
                    .max(self.min_interval_ms as f64) as u64;

                if new_interval != state.current_interval_ms {
                    state.current_interval_ms = new_interval;
                    events.push(AgentEvent::ThrottleApplied {
                        topic: KeyExpr(topic_key.clone()),
                        new_interval_ms: new_interval,
                    });
                }
            }
        }

        events
    }

    /// Get current throttle interval for a topic.
    pub fn throttle_interval(&self, topic: &str) -> Option<u64> {
        self.topic_states.get(topic).map(|s| s.current_interval_ms)
    }
}

impl Default for ThrottleAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rox_protocol::TransportKind;

    #[test]
    fn test_no_throttle_without_limit() {
        let mut agent = ThrottleAgent::new();
        let m = TransportMetrics {
            timestamp: 0,
            topic: KeyExpr("topic/a".to_string()),
            latency_us: 100,
            jitter_us: 10,
            throughput_bps: 10_000_000,
            packet_loss_ratio: 0.0,
            queue_depth: 0,
            transport_kind: TransportKind::Tcp,
        };
        agent.observe(&m);
        let events = agent.evaluate();
        assert!(events.is_empty());
    }

    #[test]
    fn test_throttle_when_over_limit() {
        let mut agent = ThrottleAgent::new();
        agent.set_topic_limit("topic/a", 1_000_000);

        let m = TransportMetrics {
            timestamp: 0,
            topic: KeyExpr("topic/a".to_string()),
            latency_us: 100,
            jitter_us: 10,
            throughput_bps: 5_000_000, // 5x over limit
            packet_loss_ratio: 0.0,
            queue_depth: 0,
            transport_kind: TransportKind::Tcp,
        };
        agent.observe(&m);

        let events = agent.evaluate();
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::ThrottleApplied {
                new_interval_ms, ..
            } => {
                assert!(*new_interval_ms >= 10);
            }
            _ => panic!("expected ThrottleApplied"),
        }
    }
}
