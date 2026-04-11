//! LogReader — reads deterministic log files for replay.

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use rox_protocol::{
    KeyExpr, MessageHeader, NodeId, QoSMetadata, RoxMessage, RoxTimestamp, VersionedPolicyUpdate,
};

/// A single entry from the log file.
#[derive(Debug)]
pub enum LogEntry {
    Message {
        sequence: u64,
        cycle: u64,
        message: RoxMessage,
    },
    PolicyApplied {
        sequence: u64,
        cycle: u64,
        update: VersionedPolicyUpdate,
    },
    CycleComplete {
        cycle: u64,
    },
}

/// Reads binary log files produced by RoxLogger.
pub struct LogReader {
    reader: BufReader<File>,
    entry_count: u64,
}

impl LogReader {
    /// Open a log file for reading.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let file =
            File::open(path).with_context(|| format!("failed to open log: {}", path.display()))?;
        let mut reader = BufReader::new(file);

        // Read and verify header
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;
        if &magic != b"RLOG" {
            bail!("invalid log file magic");
        }

        let mut version = [0u8; 1];
        reader.read_exact(&mut version)?;
        if version[0] != 1 {
            bail!("unsupported log version: {}", version[0]);
        }

        Ok(Self {
            reader,
            entry_count: 0,
        })
    }

    /// Read the next log entry. Returns None at EOF.
    pub fn next_entry(&mut self) -> Result<Option<LogEntry>> {
        let mut type_buf = [0u8; 1];
        match self.reader.read_exact(&mut type_buf) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        let entry = match type_buf[0] {
            1 => self.read_message_entry()?,
            2 => self.read_policy_entry()?,
            3 => self.read_cycle_complete()?,
            other => bail!("unknown log entry type: {other}"),
        };

        self.entry_count += 1;
        Ok(Some(entry))
    }

    /// Read all entries into a vector.
    pub fn read_all(&mut self) -> Result<Vec<LogEntry>> {
        let mut entries = Vec::new();
        while let Some(entry) = self.next_entry()? {
            entries.push(entry);
        }
        Ok(entries)
    }

    /// Number of entries read so far.
    pub fn entries_read(&self) -> u64 {
        self.entry_count
    }

    fn read_message_entry(&mut self) -> Result<LogEntry> {
        let seq = self.read_u64()?;
        let cycle = self.read_u64()?;

        let key_len = self.read_u16()? as usize;
        let mut key_buf = vec![0u8; key_len];
        self.reader.read_exact(&mut key_buf)?;
        let key_str = String::from_utf8(key_buf)?;

        let hw_time = self.read_u64()?;
        let logical_time = self.read_u64()?;

        let payload_len = self.read_u32()? as usize;
        let mut payload = vec![0u8; payload_len];
        self.reader.read_exact(&mut payload)?;

        let msg = RoxMessage {
            header: MessageHeader {
                key: KeyExpr(key_str),
                timestamp: RoxTimestamp {
                    hw_time,
                    logical_time,
                },
                qos: QoSMetadata::default(),
                source_id: NodeId("replay".to_string()),
                sequence: seq,
            },
            payload: Bytes::from(payload),
        };

        Ok(LogEntry::Message {
            sequence: seq,
            cycle,
            message: msg,
        })
    }

    fn read_policy_entry(&mut self) -> Result<LogEntry> {
        let seq = self.read_u64()?;
        let cycle = self.read_u64()?;

        let data_len = self.read_u32()? as usize;
        let mut data = vec![0u8; data_len];
        self.reader.read_exact(&mut data)?;

        let update: VersionedPolicyUpdate =
            bincode::deserialize(&data).context("failed to deserialize policy update")?;

        Ok(LogEntry::PolicyApplied {
            sequence: seq,
            cycle,
            update,
        })
    }

    fn read_cycle_complete(&mut self) -> Result<LogEntry> {
        let cycle = self.read_u64()?;
        Ok(LogEntry::CycleComplete { cycle })
    }

    fn read_u64(&mut self) -> Result<u64> {
        let mut buf = [0u8; 8];
        self.reader.read_exact(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    fn read_u32(&mut self) -> Result<u32> {
        let mut buf = [0u8; 4];
        self.reader.read_exact(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    fn read_u16(&mut self) -> Result<u16> {
        let mut buf = [0u8; 2];
        self.reader.read_exact(&mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logger::RoxLogger;

    #[test]
    fn test_write_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("test.rlog");

        // Write
        {
            let mut logger = RoxLogger::new(&log_path).unwrap();
            let msg = RoxMessage::new(
                KeyExpr::new("robot-01", "lidar", "scan"),
                NodeId("driver".to_string()),
                0,
                Bytes::from(vec![10, 20, 30]),
            );
            logger.record_message(&msg).unwrap();
            logger.record_cycle_complete(0).unwrap();
            logger.flush().unwrap();
        }

        // Read
        let mut reader = LogReader::open(&log_path).unwrap();
        let entries = reader.read_all().unwrap();
        assert_eq!(entries.len(), 2);

        match &entries[0] {
            LogEntry::Message {
                sequence,
                cycle,
                message,
            } => {
                assert_eq!(*sequence, 0);
                assert_eq!(*cycle, 0);
                assert_eq!(message.header.key.as_str(), "robot-01/lidar/scan");
                assert_eq!(message.payload.as_ref(), &[10, 20, 30]);
            }
            _ => panic!("expected Message entry"),
        }

        match &entries[1] {
            LogEntry::CycleComplete { cycle } => assert_eq!(*cycle, 0),
            _ => panic!("expected CycleComplete entry"),
        }
    }
}
