//! Deterministic Scheduler — executes TaskGraph nodes in topological order.
//!
//! Copper-inspired cycle-based execution with policy application at cycle boundaries.

use anyhow::Result;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::debug;

use rox_protocol::VersionedPolicyUpdate;

/// Plan for a single cycle, returned by `prepare_cycle()`.
#[derive(Clone, Debug)]
pub struct CyclePlan {
    pub cycle: u64,
    pub execution_order: Vec<String>,
    pub policies: Vec<VersionedPolicyUpdate>,
}

/// Metrics for a single scheduler cycle.
#[derive(Clone, Debug)]
pub struct CycleMetrics {
    pub cycle: u64,
    pub duration: Duration,
    pub nodes_executed: usize,
    pub policies_applied: usize,
}

/// Deterministic scheduler that runs TaskGraph nodes in cycle-based execution.
pub struct Scheduler {
    cycle: u64,
    execution_order: Vec<String>,
    policy_rx: mpsc::Receiver<VersionedPolicyUpdate>,
    policy_pending: VecDeque<VersionedPolicyUpdate>,
    applied_policies: Vec<(u64, VersionedPolicyUpdate)>,
    cycle_metrics: Vec<CycleMetrics>,
    max_metrics_history: usize,
}

impl Scheduler {
    /// Create a new scheduler with an execution order and a policy receiver.
    pub fn new(
        execution_order: Vec<String>,
        policy_rx: mpsc::Receiver<VersionedPolicyUpdate>,
    ) -> Self {
        Self {
            cycle: 0,
            execution_order,
            policy_rx,
            policy_pending: VecDeque::new(),
            applied_policies: Vec::new(),
            cycle_metrics: Vec::new(),
            max_metrics_history: 1000,
        }
    }

    /// Create a scheduler without policy support (for standalone use).
    pub fn without_policy(execution_order: Vec<String>) -> Self {
        let (_tx, rx) = mpsc::channel(1);
        Self::new(execution_order, rx)
    }

    /// Current cycle number.
    pub fn current_cycle(&self) -> u64 {
        self.cycle
    }

    /// Get list of policies applied at a given cycle (for replay).
    pub fn policies_at_cycle(&self, cycle: u64) -> Vec<&VersionedPolicyUpdate> {
        self.applied_policies
            .iter()
            .filter(|(c, _)| *c == cycle)
            .map(|(_, p)| p)
            .collect()
    }

    /// Run a single scheduler cycle.
    ///
    /// Protocol (copper-inspired):
    /// 1. Drain policy queue at cycle boundary
    /// 2. Apply pending policies for this cycle
    /// 3. Execute nodes in topological order
    /// 4. Record cycle metrics
    ///
    /// The `execute_node` callback is called for each node in order.
    pub async fn run_cycle<F>(&mut self, mut execute_node: F) -> Result<CycleMetrics>
    where
        F: FnMut(&str, u64) -> Result<()>,
    {
        let cycle_start = Instant::now();
        let mut policies_applied = 0;

        // Step 1: Drain policy queue at cycle boundary
        while let Ok(update) = self.policy_rx.try_recv() {
            if update.apply_at_cycle <= self.cycle {
                debug!(
                    cycle = self.cycle,
                    policy_version = update.version,
                    "applying policy at cycle boundary"
                );
                self.applied_policies.push((self.cycle, update));
                policies_applied += 1;
            } else {
                self.policy_pending.push_back(update);
            }
        }

        // Step 2: Apply pending policies for this cycle
        let mut still_pending = VecDeque::new();
        while let Some(update) = self.policy_pending.pop_front() {
            if update.apply_at_cycle <= self.cycle {
                debug!(
                    cycle = self.cycle,
                    policy_version = update.version,
                    "applying deferred policy"
                );
                self.applied_policies.push((self.cycle, update));
                policies_applied += 1;
            } else {
                still_pending.push_back(update);
            }
        }
        self.policy_pending = still_pending;

        // Step 3: Execute nodes in topological order (deterministic)
        let mut nodes_executed = 0;
        for node_id in &self.execution_order {
            execute_node(node_id, self.cycle)?;
            nodes_executed += 1;
        }

        // Step 4: Record cycle metrics
        let metrics = CycleMetrics {
            cycle: self.cycle,
            duration: cycle_start.elapsed(),
            nodes_executed,
            policies_applied,
        };

        self.cycle_metrics.push(metrics.clone());
        if self.cycle_metrics.len() > self.max_metrics_history {
            self.cycle_metrics.remove(0);
        }

        self.cycle += 1;

        Ok(metrics)
    }

    /// Prepare a cycle: drain policies, return execution order and applied policies.
    ///
    /// This is the split API for Engine integration, where the Engine needs
    /// mutable access to node instances while iterating the execution order.
    pub fn prepare_cycle(&mut self) -> CyclePlan {
        let mut policies_applied = Vec::new();

        // Drain policy queue
        while let Ok(update) = self.policy_rx.try_recv() {
            if update.apply_at_cycle <= self.cycle {
                debug!(
                    cycle = self.cycle,
                    policy_version = update.version,
                    "applying policy at cycle boundary"
                );
                self.applied_policies.push((self.cycle, update.clone()));
                policies_applied.push(update);
            } else {
                self.policy_pending.push_back(update);
            }
        }

        // Apply pending policies
        let mut still_pending = VecDeque::new();
        while let Some(update) = self.policy_pending.pop_front() {
            if update.apply_at_cycle <= self.cycle {
                debug!(
                    cycle = self.cycle,
                    policy_version = update.version,
                    "applying deferred policy"
                );
                self.applied_policies.push((self.cycle, update.clone()));
                policies_applied.push(update);
            } else {
                still_pending.push_back(update);
            }
        }
        self.policy_pending = still_pending;

        CyclePlan {
            cycle: self.cycle,
            execution_order: self.execution_order.clone(),
            policies: policies_applied,
        }
    }

    /// Complete a cycle: record metrics and advance the cycle counter.
    pub fn complete_cycle(&mut self, nodes_executed: usize, cycle_start: Instant) {
        let metrics = CycleMetrics {
            cycle: self.cycle,
            duration: cycle_start.elapsed(),
            nodes_executed,
            policies_applied: 0,
        };

        self.cycle_metrics.push(metrics);
        if self.cycle_metrics.len() > self.max_metrics_history {
            self.cycle_metrics.remove(0);
        }

        self.cycle += 1;
    }

    /// Get the execution order (read-only).
    pub fn execution_order(&self) -> &[String] {
        &self.execution_order
    }

    /// Get recent cycle metrics.
    pub fn recent_metrics(&self, count: usize) -> &[CycleMetrics] {
        let start = self.cycle_metrics.len().saturating_sub(count);
        &self.cycle_metrics[start..]
    }

    /// Average cycle duration over the last N cycles.
    pub fn avg_cycle_duration(&self, last_n: usize) -> Duration {
        let metrics = self.recent_metrics(last_n);
        if metrics.is_empty() {
            return Duration::ZERO;
        }
        let total: Duration = metrics.iter().map(|m| m.duration).sum();
        total / metrics.len() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_cycle() {
        let order = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut scheduler = Scheduler::without_policy(order);

        let mut executed = Vec::new();
        let metrics = scheduler
            .run_cycle(|node_id, cycle| {
                executed.push((node_id.to_string(), cycle));
                Ok(())
            })
            .await
            .unwrap();

        assert_eq!(executed.len(), 3);
        assert_eq!(executed[0], ("a".to_string(), 0));
        assert_eq!(executed[1], ("b".to_string(), 0));
        assert_eq!(executed[2], ("c".to_string(), 0));
        assert_eq!(metrics.nodes_executed, 3);
        assert_eq!(metrics.cycle, 0);
        assert_eq!(scheduler.current_cycle(), 1);
    }

    #[tokio::test]
    async fn test_policy_application() {
        let order = vec!["node".to_string()];
        let (tx, rx) = mpsc::channel(16);
        let mut scheduler = Scheduler::new(order, rx);

        // Send a policy for cycle 1
        tx.send(VersionedPolicyUpdate {
            version: 1,
            apply_at_cycle: 1,
            confidence: 0.9,
            update: rox_protocol::PolicyUpdateType::Alert {
                level: rox_protocol::AlertLevel::Info,
                message: "test".to_string(),
            },
        })
        .await
        .unwrap();

        // Cycle 0: policy should be deferred
        let m0 = scheduler.run_cycle(|_, _| Ok(())).await.unwrap();
        assert_eq!(m0.policies_applied, 0);

        // Cycle 1: policy should be applied
        let m1 = scheduler.run_cycle(|_, _| Ok(())).await.unwrap();
        assert_eq!(m1.policies_applied, 1);
    }
}
