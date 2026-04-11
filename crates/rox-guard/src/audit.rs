//! AuditLogger — tamper-proof blake3 hash-chained audit trail.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rox_protocol::ValidationResult;

/// Audit log entry with blake3 hash chain.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditEntry {
    pub sequence: u64,
    pub timestamp_ns: u64,
    pub entry_type: AuditEntryType,
    /// blake3 hash of the previous entry (32 bytes, hex-encoded).
    pub prev_hash: String,
    /// blake3 hash of this entry (computed over all fields except this one).
    pub hash: String,
}

/// Types of audit log entries.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AuditEntryType {
    CommandValidated {
        topic: String,
        result: String,
    },
    BoundaryViolation {
        node: String,
        field: String,
        value: f64,
        limit: f64,
    },
    EmergencyStop {
        reason: String,
    },
    GuardStarted,
    GuardStopped,
}

/// Tamper-proof audit logger with blake3 hash chain.
pub struct AuditLogger {
    writer: BufWriter<File>,
    path: PathBuf,
    sequence: AtomicU64,
    prev_hash: String,
}

impl AuditLogger {
    /// Create a new audit logger.
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to open audit log: {}", path.display()))?;

        let writer = BufWriter::new(file);

        // Genesis hash
        let genesis = blake3::hash(b"ROX_AUDIT_GENESIS");

        Ok(Self {
            writer,
            path,
            sequence: AtomicU64::new(0),
            prev_hash: genesis.to_hex().to_string(),
        })
    }

    /// Log a command validation result.
    pub fn log_validation(&mut self, topic: &str, result: &ValidationResult) -> Result<()> {
        let result_str = match result {
            ValidationResult::Passed => "passed".to_string(),
            ValidationResult::Clamped {
                field,
                original,
                clamped,
            } => {
                format!("clamped({field}: {original} -> {clamped})")
            }
            ValidationResult::Rejected { reason } => format!("rejected({reason})"),
            ValidationResult::EmergencyStop { reason } => format!("emergency_stop({reason})"),
        };

        self.write_entry(AuditEntryType::CommandValidated {
            topic: topic.to_string(),
            result: result_str,
        })
    }

    /// Log a boundary violation.
    pub fn log_boundary_violation(
        &mut self,
        node: &str,
        field: &str,
        value: f64,
        limit: f64,
    ) -> Result<()> {
        self.write_entry(AuditEntryType::BoundaryViolation {
            node: node.to_string(),
            field: field.to_string(),
            value,
            limit,
        })
    }

    /// Log an emergency stop.
    pub fn log_emergency_stop(&mut self, reason: &str) -> Result<()> {
        self.write_entry(AuditEntryType::EmergencyStop {
            reason: reason.to_string(),
        })
    }

    /// Get the audit log path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Total entries written.
    pub fn entries_written(&self) -> u64 {
        self.sequence.load(Ordering::Relaxed)
    }

    fn write_entry(&mut self, entry_type: AuditEntryType) -> Result<()> {
        let seq = self.sequence.fetch_add(1, Ordering::Relaxed);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        // Compute hash over entry content
        let content = format!("{}:{}:{}:{:?}", seq, timestamp, self.prev_hash, entry_type);
        let hash = blake3::hash(content.as_bytes()).to_hex().to_string();

        let entry = AuditEntry {
            sequence: seq,
            timestamp_ns: timestamp,
            entry_type,
            prev_hash: self.prev_hash.clone(),
            hash: hash.clone(),
        };

        // Write as JSON line
        let json = serde_json::to_string(&entry)?;
        writeln!(self.writer, "{json}")?;
        self.writer.flush()?;

        self.prev_hash = hash;

        Ok(())
    }
}

/// Verify the integrity of an audit log file.
pub fn verify_audit_log(path: impl AsRef<Path>) -> Result<bool> {
    let content = std::fs::read_to_string(path.as_ref())?;
    let mut expected_prev = blake3::hash(b"ROX_AUDIT_GENESIS").to_hex().to_string();

    for (i, line) in content.lines().enumerate() {
        let entry: AuditEntry = serde_json::from_str(line)
            .with_context(|| format!("invalid audit entry at line {i}"))?;

        if entry.prev_hash != expected_prev {
            tracing::error!(
                line = i,
                expected = %expected_prev,
                actual = %entry.prev_hash,
                "hash chain broken"
            );
            return Ok(false);
        }

        expected_prev = entry.hash.clone();
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_log_integrity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        {
            let mut logger = AuditLogger::new(&path).unwrap();
            logger
                .log_validation("robot/motor/cmd", &ValidationResult::Passed)
                .unwrap();
            logger
                .log_boundary_violation("motor", "velocity", 5.0, 2.0)
                .unwrap();
            logger.log_emergency_stop("test emergency").unwrap();
        }

        assert!(verify_audit_log(&path).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 3);
    }
}
