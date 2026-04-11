//! Self-healing Agent — detects node failures and computes backup routes.

use petgraph::algo::astar;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, info};

use rox_protocol::{AgentEvent, HealthStatus, NodeId, TransportMetrics};

/// Health state for a single node.
#[derive(Clone, Debug)]
pub struct NodeHealthState {
    pub last_seen: Instant,
    pub consecutive_failures: u32,
    pub avg_latency_us: f64,
    pub status: HealthStatus,
    pub total_messages: u64,
}

impl NodeHealthState {
    fn new() -> Self {
        Self {
            last_seen: Instant::now(),
            consecutive_failures: 0,
            avg_latency_us: 0.0,
            status: HealthStatus::Healthy,
            total_messages: 0,
        }
    }
}

/// Self-healing agent that detects failures and computes backup routes.
pub struct SelfHealingAgent {
    node_health: HashMap<String, NodeHealthState>,
    topology: DiGraph<String, f64>,
    node_indices: HashMap<String, NodeIndex>,
    failover_timeout: Duration,
    degraded_threshold: u32,
    suspected_threshold: u32,
}

impl SelfHealingAgent {
    /// Create with default settings.
    pub fn new() -> Self {
        Self {
            node_health: HashMap::new(),
            topology: DiGraph::new(),
            node_indices: HashMap::new(),
            failover_timeout: Duration::from_millis(50),
            degraded_threshold: 3,
            suspected_threshold: 5,
        }
    }

    /// Create with custom thresholds.
    pub fn with_config(failover_timeout_ms: u64, degraded_threshold: u32) -> Self {
        Self {
            failover_timeout: Duration::from_millis(failover_timeout_ms),
            degraded_threshold,
            suspected_threshold: degraded_threshold + 2,
            ..Self::new()
        }
    }

    /// Register a node in the topology.
    pub fn register_node(&mut self, node_id: &str) {
        if !self.node_indices.contains_key(node_id) {
            let idx = self.topology.add_node(node_id.to_string());
            self.node_indices.insert(node_id.to_string(), idx);
            self.node_health
                .insert(node_id.to_string(), NodeHealthState::new());
        }
    }

    /// Register a connection between nodes (for backup route computation).
    pub fn register_connection(&mut self, from: &str, to: &str, weight: f64) {
        if let (Some(&from_idx), Some(&to_idx)) =
            (self.node_indices.get(from), self.node_indices.get(to))
        {
            self.topology.add_edge(from_idx, to_idx, weight);
        }
    }

    /// Observe metrics from a node.
    pub fn observe(&mut self, metrics: &TransportMetrics) {
        // Extract node ID from topic (first segment)
        let node_id = metrics.topic.robot_id().unwrap_or("unknown").to_string();

        let state = self
            .node_health
            .entry(node_id)
            .or_insert_with(NodeHealthState::new);

        state.last_seen = Instant::now();
        state.total_messages += 1;

        // Update rolling average latency
        let alpha = 0.1;
        state.avg_latency_us =
            state.avg_latency_us * (1.0 - alpha) + metrics.latency_us as f64 * alpha;

        // Reset consecutive failures on successful observation
        if metrics.packet_loss_ratio < 0.5 {
            state.consecutive_failures = 0;
            if state.status == HealthStatus::Degraded {
                state.status = HealthStatus::Healthy;
            }
        } else {
            state.consecutive_failures += 1;
        }
    }

    /// Record a communication failure for a node.
    pub fn record_failure(&mut self, node_id: &str) {
        let state = self
            .node_health
            .entry(node_id.to_string())
            .or_insert_with(NodeHealthState::new);
        state.consecutive_failures += 1;
    }

    /// Evaluate all nodes and produce events.
    pub fn evaluate(&mut self) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        let now = Instant::now();

        let node_ids: Vec<String> = self.node_health.keys().cloned().collect();

        for node_id in node_ids {
            let state = self.node_health.get_mut(&node_id).unwrap();

            match state.status {
                HealthStatus::Healthy => {
                    if state.consecutive_failures >= self.degraded_threshold {
                        state.status = HealthStatus::Degraded;
                        events.push(AgentEvent::NodeDegraded {
                            node_id: NodeId(node_id.clone()),
                        });
                        debug!(node = %node_id, failures = state.consecutive_failures, "node degraded");
                    }
                }
                HealthStatus::Degraded => {
                    if state.consecutive_failures >= self.suspected_threshold {
                        state.status = HealthStatus::Suspected;
                        debug!(node = %node_id, "node suspected");
                    } else if state.consecutive_failures == 0 {
                        state.status = HealthStatus::Healthy;
                    }
                }
                HealthStatus::Suspected => {
                    if now.duration_since(state.last_seen) > self.failover_timeout {
                        state.status = HealthStatus::Failed;
                        info!(node = %node_id, "node failed, computing backup route");

                        if let Some(backup) = self.find_backup_route(&node_id) {
                            events.push(AgentEvent::FailoverActivated {
                                failed_node: NodeId(node_id.clone()),
                                backup_route: backup,
                            });
                        }
                    } else if state.consecutive_failures == 0 {
                        state.status = HealthStatus::Healthy;
                    }
                }
                HealthStatus::Failed => {
                    // Already failed; would need manual recovery
                }
            }
        }

        events
    }

    /// Get health status of a node.
    pub fn health(&self, node_id: &str) -> Option<&NodeHealthState> {
        self.node_health.get(node_id)
    }

    /// Find an alternative route bypassing the failed node.
    fn find_backup_route(&self, failed_node: &str) -> Option<Vec<NodeId>> {
        let failed_idx = self.node_indices.get(failed_node)?;

        // Find all nodes that connect to the failed node
        let sources: Vec<NodeIndex> = self
            .topology
            .neighbors_directed(*failed_idx, petgraph::Direction::Incoming)
            .collect();

        let targets: Vec<NodeIndex> = self
            .topology
            .neighbors_directed(*failed_idx, petgraph::Direction::Outgoing)
            .collect();

        // Try to find alternative path from any source to any target
        for source in &sources {
            for target in &targets {
                // Use A* to find shortest path that avoids the failed node
                let failed = *failed_idx;
                let result = astar(
                    &self.topology,
                    *source,
                    |n| n == *target,
                    |e| {
                        let target_node = e.target();
                        if target_node == failed {
                            f64::INFINITY
                        } else {
                            *e.weight()
                        }
                    },
                    |_| 0.0,
                );

                if let Some((_, path)) = result {
                    let route: Vec<NodeId> = path
                        .into_iter()
                        .map(|idx| NodeId(self.topology[idx].clone()))
                        .collect();
                    return Some(route);
                }
            }
        }

        None
    }
}

impl Default for SelfHealingAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_degradation() {
        let mut agent = SelfHealingAgent::with_config(50, 3);
        agent.register_node("node-a");

        // Simulate failures
        for _ in 0..3 {
            agent.record_failure("node-a");
        }

        let events = agent.evaluate();
        assert_eq!(events.len(), 1);
        match &events[0] {
            AgentEvent::NodeDegraded { node_id } => {
                assert_eq!(node_id.0, "node-a");
            }
            _ => panic!("expected NodeDegraded"),
        }
    }

    #[test]
    fn test_healthy_recovery() {
        let mut agent = SelfHealingAgent::with_config(50, 3);
        agent.register_node("node-a");

        // Degrade
        for _ in 0..3 {
            agent.record_failure("node-a");
        }
        agent.evaluate();

        // Recover
        let state = agent.node_health.get_mut("node-a").unwrap();
        state.consecutive_failures = 0;

        let events = agent.evaluate();
        assert!(events.is_empty());
        assert_eq!(
            agent.health("node-a").unwrap().status,
            HealthStatus::Healthy
        );
    }
}
