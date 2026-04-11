//! Anomaly Detection Agent — statistical anomaly detection on transport metrics.
//!
//! Level 0: Z-score based detection (no ML).

use rox_protocol::{AgentEvent, KeyExpr, TransportMetrics};
use std::collections::{HashMap, VecDeque};

/// Statistical anomaly detector.
#[allow(dead_code)]
pub struct AnomalyDetectionAgent {
    /// Per-topic statistics.
    topic_stats: HashMap<String, TopicStats>,
    /// Detection thresholds.
    jitter_threshold_us: u64,
    packet_loss_threshold: f32,
    /// Z-score threshold for anomaly detection.
    z_score_threshold: f64,
    window_size: usize,
}

/// Rolling statistics for a topic.
struct TopicStats {
    latencies: VecDeque<u64>,
    jitters: VecDeque<u64>,
    losses: VecDeque<f32>,
    anomaly_count: u64,
    total_count: u64,
}

impl TopicStats {
    fn new(window_size: usize) -> Self {
        Self {
            latencies: VecDeque::with_capacity(window_size),
            jitters: VecDeque::with_capacity(window_size),
            losses: VecDeque::with_capacity(window_size),
            anomaly_count: 0,
            total_count: 0,
        }
    }

    fn push(&mut self, metrics: &TransportMetrics, window_size: usize) {
        self.latencies.push_back(metrics.latency_us);
        self.jitters.push_back(metrics.jitter_us);
        self.losses.push_back(metrics.packet_loss_ratio);
        self.total_count += 1;

        if self.latencies.len() > window_size {
            self.latencies.pop_front();
        }
        if self.jitters.len() > window_size {
            self.jitters.pop_front();
        }
        if self.losses.len() > window_size {
            self.losses.pop_front();
        }
    }

    fn mean_latency(&self) -> f64 {
        if self.latencies.is_empty() {
            return 0.0;
        }
        self.latencies.iter().sum::<u64>() as f64 / self.latencies.len() as f64
    }

    fn std_latency(&self) -> f64 {
        let mean = self.mean_latency();
        if self.latencies.len() < 2 {
            return 0.0;
        }
        let variance = self
            .latencies
            .iter()
            .map(|&x| {
                let diff = x as f64 - mean;
                diff * diff
            })
            .sum::<f64>()
            / (self.latencies.len() - 1) as f64;
        variance.sqrt()
    }

    fn z_score_latency(&self, value: u64) -> f64 {
        let std = self.std_latency();
        if std < 1.0 {
            return 0.0;
        }
        (value as f64 - self.mean_latency()) / std
    }

    fn anomaly_ratio(&self) -> f32 {
        if self.total_count == 0 {
            return 0.0;
        }
        self.anomaly_count as f32 / self.total_count as f32
    }
}

impl AnomalyDetectionAgent {
    /// Create with default settings.
    pub fn new() -> Self {
        Self {
            topic_stats: HashMap::new(),
            jitter_threshold_us: 500,
            packet_loss_threshold: 0.01,
            z_score_threshold: 3.0,
            window_size: 1000,
        }
    }

    /// Create with custom thresholds.
    pub fn with_thresholds(jitter_us: u64, loss: f32) -> Self {
        Self {
            jitter_threshold_us: jitter_us,
            packet_loss_threshold: loss,
            ..Self::new()
        }
    }

    /// Observe a new metric data point.
    pub fn observe(&mut self, metrics: &TransportMetrics) {
        let stats = self
            .topic_stats
            .entry(metrics.topic.as_str().to_string())
            .or_insert_with(|| TopicStats::new(self.window_size));
        stats.push(metrics, self.window_size);
    }

    /// Evaluate and detect anomalies.
    pub fn evaluate(&mut self) -> Vec<AgentEvent> {
        let mut events = Vec::new();

        for (topic_key, stats) in &mut self.topic_stats {
            if stats.latencies.len() < 10 {
                continue; // Need minimum data
            }

            let last_latency = *stats.latencies.back().unwrap_or(&0);
            let last_jitter = *stats.jitters.back().unwrap_or(&0);
            let last_loss = *stats.losses.back().unwrap_or(&0.0);

            let z_score = stats.z_score_latency(last_latency);

            // Anomaly conditions
            let mut anomalies = Vec::new();

            if z_score.abs() > 3.0 {
                anomalies.push(("latency_spike", z_score.abs() as f32 / 5.0));
            }

            if last_jitter
                > stats.jitters.iter().sum::<u64>() / stats.jitters.len().max(1) as u64 * 3
            {
                anomalies.push((
                    "jitter_spike",
                    last_jitter as f32 / self.jitter_threshold_us as f32,
                ));
            }

            if last_loss > self.packet_loss_threshold * 3.0 {
                anomalies.push(("packet_loss", last_loss / self.packet_loss_threshold));
            }

            for (anomaly_type, severity) in anomalies {
                stats.anomaly_count += 1;
                events.push(AgentEvent::AnomalyDetected {
                    topic: KeyExpr(topic_key.clone()),
                    anomaly_type: anomaly_type.to_string(),
                    severity: severity.min(1.0),
                });
            }
        }

        events
    }

    /// Get anomaly ratio for a topic.
    pub fn anomaly_ratio(&self, topic: &str) -> f32 {
        self.topic_stats
            .get(topic)
            .map(|s| s.anomaly_ratio())
            .unwrap_or(0.0)
    }

    /// Total data points observed.
    pub fn total_data_points(&self) -> u64 {
        self.topic_stats.values().map(|s| s.total_count).sum()
    }

    /// Number of topics being monitored.
    pub fn monitored_topics(&self) -> usize {
        self.topic_stats.len()
    }
}

impl Default for AnomalyDetectionAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rox_protocol::TransportKind;

    fn make_metrics(latency: u64, jitter: u64, loss: f32) -> TransportMetrics {
        TransportMetrics {
            timestamp: 0,
            topic: KeyExpr("robot/lidar/scan".to_string()),
            latency_us: latency,
            jitter_us: jitter,
            throughput_bps: 1_000_000,
            packet_loss_ratio: loss,
            queue_depth: 0,
            transport_kind: TransportKind::Tcp,
        }
    }

    #[test]
    fn test_no_anomaly_when_stable() {
        let mut agent = AnomalyDetectionAgent::new();

        for _ in 0..100 {
            agent.observe(&make_metrics(100, 10, 0.0));
        }

        let events = agent.evaluate();
        assert!(events.is_empty());
    }

    #[test]
    fn test_detects_latency_spike() {
        let mut agent = AnomalyDetectionAgent::new();

        // Build baseline
        for _ in 0..50 {
            agent.observe(&make_metrics(100, 10, 0.0));
        }

        // Inject spike
        agent.observe(&make_metrics(10000, 10, 0.0));

        let events = agent.evaluate();
        assert!(!events.is_empty());
    }
}
