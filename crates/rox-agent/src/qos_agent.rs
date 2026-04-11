//! QoS Prediction Agent — predicts congestion and pre-adjusts priorities.
//!
//! Level 0: Rule-based (statistical thresholds, no ML)
//! Level 1: Self-learning (online AutoEncoder) — future
//! Level 2: Pre-trained (ONNX model) — future

use rox_protocol::{AgentEvent, KeyExpr, Priority, TransportMetrics};
use std::collections::{HashMap, VecDeque};

/// QoS prediction agent. Level 0 = rule-based (default).
#[allow(dead_code)]
pub struct QoSPredictionAgent {
    history: VecDeque<TransportMetrics>,
    window_size: usize,
    latency_threshold_us: u64,
    jitter_threshold_us: u64,
    predictions: HashMap<String, QoSPrediction>,
}

/// QoS prediction result for a topic.
#[derive(Clone, Debug)]
pub struct QoSPrediction {
    pub congestion_probability: f32,
    pub recommended_priority: Priority,
    pub predicted_latency_us: u64,
}

impl QoSPredictionAgent {
    /// Create with default thresholds.
    pub fn new() -> Self {
        Self {
            history: VecDeque::new(),
            window_size: 1000,
            latency_threshold_us: 5000,
            jitter_threshold_us: 500,
            predictions: HashMap::new(),
        }
    }

    /// Create with custom thresholds.
    pub fn with_thresholds(latency_us: u64, jitter_us: u64, window: usize) -> Self {
        Self {
            history: VecDeque::new(),
            window_size: window,
            latency_threshold_us: latency_us,
            jitter_threshold_us: jitter_us,
            predictions: HashMap::new(),
        }
    }

    /// Observe a new metric data point.
    pub fn observe(&mut self, metrics: &TransportMetrics) {
        self.history.push_back(metrics.clone());
        if self.history.len() > self.window_size {
            self.history.pop_front();
        }
    }

    /// Evaluate current state and produce events (Level 0: rule-based).
    pub fn evaluate(&mut self) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        let mut new_predictions = Vec::new();

        // Group metrics by topic
        let grouped = self.group_by_topic();

        for (topic_key, window) in &grouped {
            if window.is_empty() {
                continue;
            }

            let avg_latency =
                window.iter().map(|m| m.latency_us).sum::<u64>() / window.len() as u64;

            // Congestion prediction: latency approaching threshold
            let congestion_ratio = avg_latency as f32 / self.latency_threshold_us as f32;

            if congestion_ratio > 0.8 {
                let recommended = if congestion_ratio > 0.95 {
                    Priority::Highest
                } else {
                    Priority::High
                };

                events.push(AgentEvent::CongestionWarning {
                    topic: KeyExpr(topic_key.clone()),
                    probability: congestion_ratio.min(1.0),
                    recommended_priority: recommended,
                });
            }

            new_predictions.push((
                topic_key.clone(),
                QoSPrediction {
                    congestion_probability: congestion_ratio.min(1.0),
                    recommended_priority: if congestion_ratio > 0.8 {
                        Priority::High
                    } else {
                        Priority::Normal
                    },
                    predicted_latency_us: avg_latency,
                },
            ));
        }

        for (key, pred) in new_predictions {
            self.predictions.insert(key, pred);
        }

        events
    }

    /// Get the latest prediction for a topic.
    pub fn prediction(&self, topic: &str) -> Option<&QoSPrediction> {
        self.predictions.get(topic)
    }

    /// Number of data points collected.
    pub fn data_points(&self) -> usize {
        self.history.len()
    }

    fn group_by_topic(&self) -> HashMap<String, Vec<&TransportMetrics>> {
        let mut grouped: HashMap<String, Vec<&TransportMetrics>> = HashMap::new();
        for m in &self.history {
            grouped
                .entry(m.topic.as_str().to_string())
                .or_default()
                .push(m);
        }
        grouped
    }
}

impl Default for QoSPredictionAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rox_protocol::TransportKind;

    fn make_metrics(topic: &str, latency: u64, jitter: u64) -> TransportMetrics {
        TransportMetrics {
            timestamp: 0,
            topic: KeyExpr(topic.to_string()),
            latency_us: latency,
            jitter_us: jitter,
            throughput_bps: 1_000_000,
            packet_loss_ratio: 0.0,
            queue_depth: 0,
            transport_kind: TransportKind::Tcp,
        }
    }

    #[test]
    fn test_no_events_when_healthy() {
        let mut agent = QoSPredictionAgent::with_thresholds(5000, 500, 100);

        for _ in 0..10 {
            agent.observe(&make_metrics("robot/lidar/scan", 100, 10));
        }

        let events = agent.evaluate();
        assert!(events.is_empty());
    }

    #[test]
    fn test_congestion_warning() {
        let mut agent = QoSPredictionAgent::with_thresholds(5000, 500, 100);

        // Push high latency metrics
        for _ in 0..10 {
            agent.observe(&make_metrics("robot/lidar/scan", 4500, 100));
        }

        let events = agent.evaluate();
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::CongestionWarning { probability, .. } => {
                assert!(*probability > 0.8);
            }
            _ => panic!("expected CongestionWarning"),
        }
    }
}
