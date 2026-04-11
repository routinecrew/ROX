//! AgentRuntime — orchestrates all agents with Observer/Suggestion/Autonomous modes.

use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, info};

use rox_protocol::{AgentEvent, AgentStartMode, TransportMetrics, VersionedPolicyUpdate};

use crate::anomaly_agent::AnomalyDetectionAgent;
use crate::healing_agent::SelfHealingAgent;
use crate::qos_agent::QoSPredictionAgent;
use crate::rule_engine::RuleEngine;
use crate::throttle_agent::ThrottleAgent;

/// Statistics for mode transition decisions.
#[derive(Clone, Debug, Default)]
pub struct ObservationStats {
    pub observation_hours: u32,
    pub total_data_points: u64,
    pub topic_coverage: f32,
    pub anomaly_ratio: f32,
}

/// Agent runtime that orchestrates all sub-agents.
pub struct AgentRuntime {
    mode: AgentStartMode,
    metrics_rx: mpsc::Receiver<TransportMetrics>,
    policy_tx: mpsc::Sender<VersionedPolicyUpdate>,
    event_tx: Option<tokio::sync::broadcast::Sender<AgentEvent>>,

    qos_agent: QoSPredictionAgent,
    healing_agent: SelfHealingAgent,
    anomaly_agent: AnomalyDetectionAgent,
    throttle_agent: ThrottleAgent,
    rule_engine: RuleEngine,

    current_cycle: u64,
    interval: Duration,
    observation_stats: ObservationStats,
    start_time: std::time::Instant,
}

impl AgentRuntime {
    /// Create a new agent runtime.
    pub fn new(
        metrics_rx: mpsc::Receiver<TransportMetrics>,
        policy_tx: mpsc::Sender<VersionedPolicyUpdate>,
    ) -> Self {
        Self {
            mode: AgentStartMode::Observer { duration_hours: 24 },
            metrics_rx,
            policy_tx,
            event_tx: None,
            qos_agent: QoSPredictionAgent::new(),
            healing_agent: SelfHealingAgent::new(),
            anomaly_agent: AnomalyDetectionAgent::new(),
            throttle_agent: ThrottleAgent::new(),
            rule_engine: RuleEngine::new(),
            current_cycle: 0,
            interval: Duration::from_millis(100),
            observation_stats: ObservationStats::default(),
            start_time: std::time::Instant::now(),
        }
    }

    /// Set the agent mode.
    pub fn with_mode(mut self, mode: AgentStartMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the evaluation interval.
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Attach an event broadcast channel for the API/dashboard.
    pub fn with_event_broadcast(mut self, tx: tokio::sync::broadcast::Sender<AgentEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Run the agent loop. Runs until the metrics channel closes.
    pub async fn run(&mut self) {
        info!(mode = ?self.mode, "agent runtime started");

        loop {
            // 1. Collect metrics (non-blocking drain)
            let mut batch_count = 0;
            while let Ok(metrics) = self.metrics_rx.try_recv() {
                self.qos_agent.observe(&metrics);
                self.healing_agent.observe(&metrics);
                self.anomaly_agent.observe(&metrics);
                self.throttle_agent.observe(&metrics);
                batch_count += 1;
            }

            self.observation_stats.total_data_points += batch_count;
            self.observation_stats.observation_hours =
                (self.start_time.elapsed().as_secs() / 3600) as u32;

            // 2. Evaluate all agents
            let mut events = Vec::new();
            events.extend(self.qos_agent.evaluate());
            events.extend(self.healing_agent.evaluate());
            events.extend(self.anomaly_agent.evaluate());
            events.extend(self.throttle_agent.evaluate());

            // 3. Broadcast events
            if let Some(event_tx) = &self.event_tx {
                for event in &events {
                    let _ = event_tx.send(event.clone());
                }
            }

            // 4. Process events through rule engine based on mode
            self.rule_engine.set_cycle(self.current_cycle);

            for event in events {
                if let Some(update) = self.rule_engine.evaluate(&event) {
                    match &self.mode {
                        AgentStartMode::Observer { .. } => {
                            debug!(
                                version = update.version,
                                "observer mode: logging policy (not applying)"
                            );
                        }
                        AgentStartMode::Suggestion => {
                            info!(version = update.version, "suggestion mode: policy proposed");
                            // In suggestion mode, events are broadcast but not applied
                        }
                        AgentStartMode::Autonomous => {
                            debug!(version = update.version, "autonomous mode: applying policy");
                            let _ = self.policy_tx.send(update).await;
                        }
                    }
                }
            }

            self.current_cycle += 1;

            // 5. Sleep for agent interval
            tokio::time::sleep(self.interval).await;
        }
    }

    /// Get current observation statistics.
    pub fn observation_stats(&self) -> &ObservationStats {
        &self.observation_stats
    }

    /// Get current mode.
    pub fn mode(&self) -> &AgentStartMode {
        &self.mode
    }

    /// Manually transition to a new mode.
    pub fn set_mode(&mut self, mode: AgentStartMode) {
        info!(from = ?self.mode, to = ?mode, "agent mode transition");
        self.mode = mode;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_runtime_creation() {
        let (metrics_tx, metrics_rx) = mpsc::channel(100);
        let (policy_tx, _policy_rx) = mpsc::channel(100);

        let runtime = AgentRuntime::new(metrics_rx, policy_tx);
        assert!(matches!(runtime.mode(), AgentStartMode::Observer { .. }));
    }
}
