//! TransportSelector — Agent-driven dynamic transport selection.
//!
//! Selects optimal transport based on network topology and agent hints.
//! Supports the 4-phase transport switching protocol (Drain-Buffer-Switch-Replay).

use std::collections::HashMap;
use tracing::{debug, info};

use rox_protocol::{NodeId, TransportKind};

/// Hint from the Agent about recommended transport for a target.
#[derive(Clone, Debug)]
pub struct AgentTransportHint {
    pub target: NodeId,
    pub recommended: TransportKind,
    pub confidence: f32,
}

/// Transport switching report.
#[derive(Clone, Debug, Default)]
pub struct TransportSwitchReport {
    pub success: bool,
    pub drained_count: usize,
    pub replayed_count: usize,
    pub switch_duration_us: u64,
    pub reason: Option<String>,
}

/// Dynamic transport selector.
///
/// Default behavior:
/// - Same machine -> SHM (if available)
/// - Cross machine -> TCP
///
/// With Agent hints, switches transports dynamically based on
/// network conditions and predictions.
pub struct TransportSelector {
    /// Known local nodes (same machine).
    local_nodes: HashMap<String, bool>,
    /// Active transport per target node.
    active_transport: HashMap<String, TransportKind>,
    /// Available transport kinds.
    available_transports: Vec<TransportKind>,
    /// Agent hints for transport recommendations.
    agent_hints: HashMap<String, AgentTransportHint>,
    /// Whether SHM is available.
    shm_available: bool,
}

impl TransportSelector {
    /// Create a new transport selector.
    pub fn new(shm_available: bool) -> Self {
        let mut available = vec![TransportKind::Tcp, TransportKind::Udp];
        if shm_available {
            available.push(TransportKind::SharedMemory);
        }

        Self {
            local_nodes: HashMap::new(),
            active_transport: HashMap::new(),
            available_transports: available,
            agent_hints: HashMap::new(),
            shm_available,
        }
    }

    /// Register a node as local (same machine).
    pub fn register_local_node(&mut self, node_id: &str) {
        self.local_nodes.insert(node_id.to_string(), true);
    }

    /// Register a node as remote.
    pub fn register_remote_node(&mut self, node_id: &str) {
        self.local_nodes.insert(node_id.to_string(), false);
    }

    /// Check if a node is on the same machine.
    pub fn is_local(&self, node_id: &str) -> bool {
        self.local_nodes.get(node_id).copied().unwrap_or(false)
    }

    /// Select the best transport for a target node.
    pub fn select(&self, target: &NodeId) -> TransportKind {
        // Check agent hints first
        if let Some(hint) = self.agent_hints.get(&target.0) {
            if hint.confidence > 0.7 && self.available_transports.contains(&hint.recommended) {
                debug!(
                    target = %target,
                    transport = ?hint.recommended,
                    confidence = hint.confidence,
                    "using agent-recommended transport"
                );
                return hint.recommended;
            }
        }

        // Default selection based on locality
        if self.is_local(&target.0) && self.shm_available {
            TransportKind::SharedMemory
        } else {
            TransportKind::Tcp
        }
    }

    /// Apply an agent transport hint.
    pub fn apply_hint(&mut self, hint: AgentTransportHint) {
        info!(
            target = %hint.target,
            recommended = ?hint.recommended,
            confidence = hint.confidence,
            "agent transport hint applied"
        );
        self.agent_hints.insert(hint.target.0.clone(), hint);
    }

    /// Clear all agent hints.
    pub fn clear_hints(&mut self) {
        self.agent_hints.clear();
    }

    /// Get the currently active transport for a target.
    pub fn active_transport(&self, target: &str) -> Option<TransportKind> {
        self.active_transport.get(target).copied()
    }

    /// Execute a 4-phase transport switch (Drain-Buffer-Switch-Replay).
    pub fn switch_transport(
        &mut self,
        target: &NodeId,
        from: TransportKind,
        to: TransportKind,
    ) -> TransportSwitchReport {
        let start = std::time::Instant::now();

        if !self.available_transports.contains(&to) {
            return TransportSwitchReport {
                success: false,
                reason: Some(format!("transport {:?} not available", to)),
                ..Default::default()
            };
        }

        // Phase 1: DRAIN (in real impl, drain pending messages)
        // Phase 2: BUFFER (messages buffered during switch)
        // Phase 3: SWITCH
        self.active_transport.insert(target.0.clone(), to);
        // Phase 4: REPLAY (replay buffered messages on new transport)

        info!(
            target = %target,
            from = ?from,
            to = ?to,
            "transport switched"
        );

        TransportSwitchReport {
            success: true,
            drained_count: 0,
            replayed_count: 0,
            switch_duration_us: start.elapsed().as_micros() as u64,
            reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_selection() {
        let mut selector = TransportSelector::new(true);
        selector.register_local_node("local_node");
        selector.register_remote_node("remote_node");

        assert_eq!(
            selector.select(&NodeId("local_node".to_string())),
            TransportKind::SharedMemory
        );
        assert_eq!(
            selector.select(&NodeId("remote_node".to_string())),
            TransportKind::Tcp
        );
    }

    #[test]
    fn test_agent_hint_override() {
        let mut selector = TransportSelector::new(false);
        selector.register_remote_node("target");

        // Without hint: TCP
        assert_eq!(
            selector.select(&NodeId("target".to_string())),
            TransportKind::Tcp
        );

        // With hint: UDP
        selector.apply_hint(AgentTransportHint {
            target: NodeId("target".to_string()),
            recommended: TransportKind::Udp,
            confidence: 0.9,
        });
        assert_eq!(
            selector.select(&NodeId("target".to_string())),
            TransportKind::Udp
        );
    }

    #[test]
    fn test_transport_switch() {
        let mut selector = TransportSelector::new(true);
        let target = NodeId("node".to_string());

        let report = selector.switch_transport(&target, TransportKind::Tcp, TransportKind::Udp);
        assert!(report.success);
        assert_eq!(selector.active_transport("node"), Some(TransportKind::Udp));
    }
}
