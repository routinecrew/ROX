//! RoxLogger — deterministic binary log writer.
//!
//! Records RoxMessages and policy updates in a binary format
//! that enables bit-exact replay.

use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rox_protocol::{RoxMessage, VersionedPolicyUpdate};

const LOG_MAGIC: &[u8; 4] = b"RLOG";
const LOG_VERSION: u8 = 1;

/// Entry types in the deterministic log.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogEntryType {
    Message = 1,
    PolicyApplied = 2,
    CycleComplete = 3,
}

/// Deterministic log writer.
///
/// Writes RoxMessages and policy updates in binary format for replay.
/// Inspired by copper-rs cu29-log.
pub struct RoxLogger {
    writer: BufWriter<File>,
    path: PathBuf,
    sequence: AtomicU64,
    cycle: u64,
}

impl RoxLogger {
    /// Create a new logger writing to the given path.
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create log directory: {}", parent.display()))?;
        }

        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .with_context(|| format!("failed to create log file: {}", path.display()))?;

        let mut writer = BufWriter::with_capacity(64 * 1024, file);

        // Write file header
        writer.write_all(LOG_MAGIC)?;
        writer.write_all(&[LOG_VERSION])?;
        writer.flush()?;

        Ok(Self {
            writer,
            path,
            sequence: AtomicU64::new(0),
            cycle: 0,
        })
    }

    /// Record a message.
    pub fn record_message(&mut self, msg: &RoxMessage) -> Result<()> {
        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);

        // Entry format: [type:1B] [seq:8B] [cycle:8B] [key_len:2B] [key] [timestamp:16B] [payload_len:4B] [payload]
        self.writer.write_all(&[LogEntryType::Message as u8])?;
        self.writer.write_all(&seq.to_le_bytes())?;
        self.writer.write_all(&self.cycle.to_le_bytes())?;

        let key_bytes = msg.header.key.as_str().as_bytes();
        self.writer
            .write_all(&(key_bytes.len() as u16).to_le_bytes())?;
        self.writer.write_all(key_bytes)?;

        self.writer
            .write_all(&msg.header.timestamp.hw_time.to_le_bytes())?;
        self.writer
            .write_all(&msg.header.timestamp.logical_time.to_le_bytes())?;

        self.writer
            .write_all(&(msg.payload.len() as u32).to_le_bytes())?;
        self.writer.write_all(&msg.payload)?;

        Ok(())
    }

    /// Record a policy application event.
    pub fn record_policy_applied(
        &mut self,
        cycle: u64,
        update: &VersionedPolicyUpdate,
    ) -> Result<()> {
        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);

        // Serialize policy update with bincode
        let policy_bytes =
            bincode::serialize(update).context("failed to serialize policy update")?;

        self.writer
            .write_all(&[LogEntryType::PolicyApplied as u8])?;
        self.writer.write_all(&seq.to_le_bytes())?;
        self.writer.write_all(&cycle.to_le_bytes())?;
        self.writer
            .write_all(&(policy_bytes.len() as u32).to_le_bytes())?;
        self.writer.write_all(&policy_bytes)?;

        Ok(())
    }

    /// Mark a cycle as complete.
    pub fn record_cycle_complete(&mut self, cycle: u64) -> Result<()> {
        self.writer
            .write_all(&[LogEntryType::CycleComplete as u8])?;
        self.writer.write_all(&cycle.to_le_bytes())?;
        self.cycle = cycle + 1;
        Ok(())
    }

    /// Flush the buffer to disk.
    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush().context("failed to flush log")
    }

    /// Get the log file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get the current sequence number.
    pub fn sequence(&self) -> u64 {
        self.sequence.load(Ordering::Relaxed)
    }
}

impl Drop for RoxLogger {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use rox_protocol::{KeyExpr, NodeId, RoxMessage};

    #[test]
    fn test_logger_basic() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("test.rlog");

        let mut logger = RoxLogger::new(&log_path).unwrap();

        let msg = RoxMessage::new(
            KeyExpr::new("robot-01", "lidar", "scan"),
            NodeId("driver".to_string()),
            0,
            Bytes::from(vec![1, 2, 3]),
        );

        logger.record_message(&msg).unwrap();
        logger.record_cycle_complete(0).unwrap();
        logger.flush().unwrap();

        assert!(log_path.exists());
        assert_eq!(logger.sequence(), 1);
    }
}
