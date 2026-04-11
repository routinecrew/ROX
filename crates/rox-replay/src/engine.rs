//! ReplayEngine — bit-exact replay from deterministic logs.
//!
//! Key design decisions (copper-inspired):
//! - Uses recorded timestamps, NOT wall clock
//! - Agent decisions are injected from log, NOT re-executed
//! - Messages are replayed in exact cycle order

use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, info};

use rox_log::{LogEntry, LogReader};
use rox_protocol::{RoxMessage, VersionedPolicyUpdate};

/// Replay statistics.
#[derive(Clone, Debug, Default)]
pub struct ReplayStats {
    pub total_cycles: u64,
    pub total_messages: u64,
    pub total_policies: u64,
}

/// Clock mode for replay.
pub enum ReplayClock {
    /// Replay as fast as possible.
    AsFastAsPossible,
    /// Replay at original timing (based on recorded timestamps).
    OriginalTiming,
    /// Replay at a scaled speed (2.0 = 2x speed).
    Scaled(f64),
}

/// Bit-exact replay engine.
///
/// Reads a deterministic log file and replays all messages and policies
/// in the exact order and timing they were recorded.
pub struct ReplayEngine {
    reader: LogReader,
    clock: ReplayClock,
    stats: ReplayStats,
    /// Messages grouped by cycle for deterministic replay.
    cycle_messages: HashMap<u64, Vec<RoxMessage>>,
    /// Policies grouped by cycle for injection.
    cycle_policies: HashMap<u64, Vec<VersionedPolicyUpdate>>,
}

impl ReplayEngine {
    /// Open a log file for replay.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let reader = LogReader::open(path.as_ref())?;
        Ok(Self {
            reader,
            clock: ReplayClock::AsFastAsPossible,
            stats: ReplayStats::default(),
            cycle_messages: HashMap::new(),
            cycle_policies: HashMap::new(),
        })
    }

    /// Set the replay clock mode.
    pub fn with_clock(mut self, clock: ReplayClock) -> Self {
        self.clock = clock;
        self
    }

    /// Load all entries from the log into memory for replay.
    pub fn load(&mut self) -> Result<()> {
        let entries = self.reader.read_all()?;

        for entry in entries {
            match entry {
                LogEntry::Message { cycle, message, .. } => {
                    self.cycle_messages.entry(cycle).or_default().push(message);
                    self.stats.total_messages += 1;
                }
                LogEntry::PolicyApplied { cycle, update, .. } => {
                    self.cycle_policies.entry(cycle).or_default().push(update);
                    self.stats.total_policies += 1;
                }
                LogEntry::CycleComplete { cycle } => {
                    self.stats.total_cycles = self.stats.total_cycles.max(cycle + 1);
                }
            }
        }

        info!(
            cycles = self.stats.total_cycles,
            messages = self.stats.total_messages,
            policies = self.stats.total_policies,
            "replay loaded"
        );

        Ok(())
    }

    /// Replay all cycles, calling the handler for each message and policy.
    pub async fn replay<F, P>(&self, mut on_message: F, mut on_policy: P) -> Result<()>
    where
        F: FnMut(u64, &RoxMessage) -> Result<()>,
        P: FnMut(u64, &VersionedPolicyUpdate) -> Result<()>,
    {
        let mut prev_hw_time: Option<u64> = None;

        for cycle in 0..self.stats.total_cycles {
            // Inject policies at cycle boundary (recorded decisions, not re-executed)
            if let Some(policies) = self.cycle_policies.get(&cycle) {
                for policy in policies {
                    debug!(cycle, version = policy.version, "replaying policy");
                    on_policy(cycle, policy)?;
                }
            }

            // Replay messages in order
            if let Some(messages) = self.cycle_messages.get(&cycle) {
                for msg in messages {
                    // Apply timing based on clock mode
                    if let ReplayClock::OriginalTiming | ReplayClock::Scaled(_) = &self.clock {
                        if let Some(prev) = prev_hw_time {
                            let delta_ns = msg.header.timestamp.hw_time.saturating_sub(prev);
                            let sleep_ns = match &self.clock {
                                ReplayClock::OriginalTiming => delta_ns,
                                ReplayClock::Scaled(factor) => (delta_ns as f64 / factor) as u64,
                                _ => 0,
                            };
                            if sleep_ns > 1000 {
                                // Only sleep for > 1us
                                tokio::time::sleep(std::time::Duration::from_nanos(sleep_ns)).await;
                            }
                        }
                        prev_hw_time = Some(msg.header.timestamp.hw_time);
                    }

                    on_message(cycle, msg)?;
                }
            }
        }

        info!(
            cycles = self.stats.total_cycles,
            messages = self.stats.total_messages,
            "replay complete"
        );

        Ok(())
    }

    /// Get replay statistics.
    pub fn stats(&self) -> &ReplayStats {
        &self.stats
    }

    /// Get messages for a specific cycle.
    pub fn messages_at_cycle(&self, cycle: u64) -> &[RoxMessage] {
        self.cycle_messages
            .get(&cycle)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get policies for a specific cycle.
    pub fn policies_at_cycle(&self, cycle: u64) -> &[VersionedPolicyUpdate] {
        self.cycle_policies
            .get(&cycle)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use rox_log::RoxLogger;
    use rox_protocol::{
        AlertLevel, KeyExpr, NodeId, PolicyUpdateType, VersionedPolicyUpdate,
    };

    #[tokio::test]
    async fn test_replay_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("replay_test.rlog");

        // Record
        {
            let mut logger = RoxLogger::new(&log_path).unwrap();
            for i in 0..3 {
                let msg = RoxMessage::new(
                    KeyExpr::new("robot-01", "lidar", "scan"),
                    NodeId("driver".to_string()),
                    i,
                    Bytes::from(vec![i as u8]),
                );
                logger.record_message(&msg).unwrap();
                logger.record_cycle_complete(i).unwrap();
            }
            logger.flush().unwrap();
        }

        // Replay
        let mut engine = ReplayEngine::open(&log_path).unwrap();
        engine.load().unwrap();

        assert_eq!(engine.stats().total_cycles, 3);
        assert_eq!(engine.stats().total_messages, 3);

        let mut replayed_messages = Vec::new();
        engine
            .replay(
                |cycle, msg| {
                    replayed_messages.push((cycle, msg.payload.clone()));
                    Ok(())
                },
                |_, _| Ok(()),
            )
            .await
            .unwrap();

        assert_eq!(replayed_messages.len(), 3);
        assert_eq!(replayed_messages[0].1.as_ref(), &[0]);
        assert_eq!(replayed_messages[1].1.as_ref(), &[1]);
        assert_eq!(replayed_messages[2].1.as_ref(), &[2]);
    }

    #[tokio::test]
    async fn test_replay_empty_log() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("empty.rlog");

        // Write log with header only, no entries
        {
            let logger = RoxLogger::new(&log_path).unwrap();
            drop(logger); // flush on drop
        }

        let mut engine = ReplayEngine::open(&log_path).unwrap();
        engine.load().unwrap();

        assert_eq!(engine.stats().total_cycles, 0);
        assert_eq!(engine.stats().total_messages, 0);
        assert_eq!(engine.stats().total_policies, 0);

        let mut count = 0u64;
        engine
            .replay(|_, _| { count += 1; Ok(()) }, |_, _| Ok(()))
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_replay_invalid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("corrupt.rlog");

        // Write garbage
        std::fs::write(&path, b"NOT_A_LOG_FILE").unwrap();

        let result = ReplayEngine::open(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_replay_nonexistent_file() {
        let result = ReplayEngine::open("/tmp/does_not_exist_rox.rlog");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_replay_with_policies() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("policy_test.rlog");

        {
            let mut logger = RoxLogger::new(&log_path).unwrap();

            let msg = RoxMessage::new(
                KeyExpr::new("robot-01", "lidar", "scan"),
                NodeId("driver".to_string()),
                0,
                Bytes::from(vec![42]),
            );
            logger.record_message(&msg).unwrap();

            let policy = VersionedPolicyUpdate {
                version: 1,
                apply_at_cycle: 0,
                confidence: 0.95,
                update: PolicyUpdateType::Alert {
                    level: AlertLevel::Warning,
                    message: "test alert".to_string(),
                },
            };
            logger.record_policy_applied(0, &policy).unwrap();
            logger.record_cycle_complete(0).unwrap();
            logger.flush().unwrap();
        }

        let mut engine = ReplayEngine::open(&log_path).unwrap();
        engine.load().unwrap();

        assert_eq!(engine.stats().total_messages, 1);
        assert_eq!(engine.stats().total_policies, 1);
        assert_eq!(engine.stats().total_cycles, 1);

        let mut policy_count = 0u64;
        let mut msg_count = 0u64;
        engine
            .replay(
                |_, _| { msg_count += 1; Ok(()) },
                |_, p| {
                    assert_eq!(p.version, 1);
                    policy_count += 1;
                    Ok(())
                },
            )
            .await
            .unwrap();

        assert_eq!(msg_count, 1);
        assert_eq!(policy_count, 1);
    }

    #[test]
    fn test_messages_at_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("cycle_test.rlog");

        {
            let mut logger = RoxLogger::new(&log_path).unwrap();
            for i in 0..2 {
                let msg = RoxMessage::new(
                    KeyExpr::new("robot-01", "lidar", "scan"),
                    NodeId("driver".to_string()),
                    i,
                    Bytes::from(vec![i as u8]),
                );
                logger.record_message(&msg).unwrap();
                logger.record_cycle_complete(i).unwrap();
            }
            logger.flush().unwrap();
        }

        let mut engine = ReplayEngine::open(&log_path).unwrap();
        engine.load().unwrap();

        assert_eq!(engine.messages_at_cycle(0).len(), 1);
        assert_eq!(engine.messages_at_cycle(1).len(), 1);
        assert_eq!(engine.messages_at_cycle(99).len(), 0);
    }

    #[tokio::test]
    async fn test_replay_scaled_clock() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("scaled.rlog");

        {
            let mut logger = RoxLogger::new(&log_path).unwrap();
            let msg = RoxMessage::new(
                KeyExpr::new("robot-01", "lidar", "scan"),
                NodeId("driver".to_string()),
                0,
                Bytes::from(vec![1]),
            );
            logger.record_message(&msg).unwrap();
            logger.record_cycle_complete(0).unwrap();
            logger.flush().unwrap();
        }

        let mut engine = ReplayEngine::open(&log_path).unwrap();
        engine.load().unwrap();
        let engine = engine.with_clock(ReplayClock::Scaled(10.0));

        let mut count = 0u64;
        engine
            .replay(|_, _| { count += 1; Ok(()) }, |_, _| Ok(()))
            .await
            .unwrap();
        assert_eq!(count, 1);
    }
}
