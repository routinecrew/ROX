//! Wire encoding/decoding for RoxMessage.
//!
//! Wire format (binary, little-endian):
//! ```text
//! [magic: 4B "ROX\0"] [version: 1B] [flags: 1B] [header_len: 2B]
//! [key_len: 2B] [key: variable]
//! [hw_time: 8B] [logical_time: 8B]
//! [priority: 1B] [reliability: 1B] [durability: 1B]
//! [deadline_us: 8B (0 = none)] [lifespan_us: 8B (0 = none)]
//! [source_id_len: 2B] [source_id: variable]
//! [sequence: 8B]
//! [payload_len: 4B] [payload: variable]
//! ```

use crate::contracts::*;
use anyhow::{bail, Context, Result};
use bytes::{Buf, BufMut, Bytes, BytesMut};

const MAGIC: &[u8; 4] = b"ROX\0";
const VERSION: u8 = 1;
/// Maximum payload size: 64 MB. Prevents OOM from malformed/malicious frames.
const MAX_PAYLOAD_SIZE: usize = 64 * 1024 * 1024;

/// Wire format encoder.
pub struct WireEncoder;

/// Wire format decoder.
pub struct WireDecoder;

impl WireEncoder {
    /// Encode a RoxMessage into wire format bytes.
    pub fn encode(msg: &RoxMessage) -> Result<Bytes> {
        let key_bytes = msg.header.key.as_str().as_bytes();
        let source_bytes = msg.header.source_id.0.as_bytes();

        let estimated_size = 4 + 1 + 1 + 2  // magic + version + flags + header_len
            + 2 + key_bytes.len()
            + 8 + 8  // timestamps
            + 3  // priority + reliability + durability
            + 8 + 8  // deadline + lifespan
            + 2 + source_bytes.len()
            + 8  // sequence
            + 4 + msg.payload.len();

        let mut buf = BytesMut::with_capacity(estimated_size);

        // Magic + version + flags
        buf.put_slice(MAGIC);
        buf.put_u8(VERSION);
        buf.put_u8(0); // flags (reserved)

        // Header length placeholder
        let header_start = buf.len();
        buf.put_u16_le(0); // will be filled later

        // Key
        buf.put_u16_le(key_bytes.len() as u16);
        buf.put_slice(key_bytes);

        // Timestamps
        buf.put_u64_le(msg.header.timestamp.hw_time);
        buf.put_u64_le(msg.header.timestamp.logical_time);

        // QoS
        buf.put_u8(msg.header.qos.priority as u8);
        buf.put_u8(match msg.header.qos.reliability {
            Reliability::BestEffort => 0,
            Reliability::Reliable => 1,
        });
        buf.put_u8(match msg.header.qos.durability {
            Durability::Volatile => 0,
            Durability::TransientLocal => 1,
        });
        buf.put_u64_le(msg.header.qos.deadline_us.unwrap_or(0));
        buf.put_u64_le(msg.header.qos.lifespan_us.unwrap_or(0));

        // Source ID
        buf.put_u16_le(source_bytes.len() as u16);
        buf.put_slice(source_bytes);

        // Sequence
        buf.put_u64_le(msg.header.sequence);

        // Fill header length
        let header_len = (buf.len() - header_start - 2) as u16;
        let header_len_bytes = header_len.to_le_bytes();
        buf[header_start] = header_len_bytes[0];
        buf[header_start + 1] = header_len_bytes[1];

        // Payload
        buf.put_u32_le(msg.payload.len() as u32);
        buf.put_slice(&msg.payload);

        Ok(buf.freeze())
    }

    /// Encode only the header (for logging/replay without payload).
    pub fn encode_header(header: &MessageHeader) -> Result<Bytes> {
        let dummy_msg = RoxMessage {
            header: header.clone(),
            payload: Bytes::new(),
        };
        Self::encode(&dummy_msg)
    }
}

impl WireDecoder {
    /// Decode a RoxMessage from wire format bytes.
    pub fn decode(mut data: Bytes) -> Result<RoxMessage> {
        // Magic
        if data.remaining() < 4 {
            bail!("insufficient data for magic");
        }
        let magic = data.copy_to_bytes(4);
        if magic.as_ref() != MAGIC {
            bail!("invalid magic: expected ROX\\0");
        }

        // Version
        let version = data.get_u8();
        if version != VERSION {
            bail!("unsupported wire version: {version}");
        }

        // Flags (reserved)
        let _flags = data.get_u8();

        // Header length
        let _header_len = data.get_u16_le();

        // Key
        let key_len = data.get_u16_le() as usize;
        if data.remaining() < key_len {
            bail!("insufficient data for key");
        }
        let key_bytes = data.copy_to_bytes(key_len);
        let key_str = std::str::from_utf8(&key_bytes).context("invalid key utf8")?;
        let key = KeyExpr(key_str.to_string());

        // Timestamps
        let hw_time = data.get_u64_le();
        let logical_time = data.get_u64_le();

        // QoS
        let priority_val = data.get_u8();
        let priority = match priority_val {
            0 => Priority::Highest,
            1 => Priority::High,
            2 => Priority::AboveNormal,
            3 => Priority::Normal,
            4 => Priority::BelowNormal,
            5 => Priority::Low,
            6 => Priority::Background,
            7 => Priority::Lowest,
            _ => Priority::Normal,
        };
        let reliability = match data.get_u8() {
            1 => Reliability::Reliable,
            _ => Reliability::BestEffort,
        };
        let durability = match data.get_u8() {
            1 => Durability::TransientLocal,
            _ => Durability::Volatile,
        };
        let deadline_raw = data.get_u64_le();
        let deadline_us = if deadline_raw == 0 {
            None
        } else {
            Some(deadline_raw)
        };
        let lifespan_raw = data.get_u64_le();
        let lifespan_us = if lifespan_raw == 0 {
            None
        } else {
            Some(lifespan_raw)
        };

        // Source ID
        let source_len = data.get_u16_le() as usize;
        if data.remaining() < source_len {
            bail!("insufficient data for source_id");
        }
        let source_bytes = data.copy_to_bytes(source_len);
        let source_str = std::str::from_utf8(&source_bytes).context("invalid source_id utf8")?;
        let source_id = NodeId(source_str.to_string());

        // Sequence
        let sequence = data.get_u64_le();

        // Payload
        let payload_len = data.get_u32_le() as usize;
        if payload_len > MAX_PAYLOAD_SIZE {
            bail!("payload too large: {payload_len} bytes exceeds max {MAX_PAYLOAD_SIZE}");
        }
        if data.remaining() < payload_len {
            bail!(
                "insufficient data for payload: expected {payload_len}, remaining {}",
                data.remaining()
            );
        }
        let payload = data.copy_to_bytes(payload_len);

        Ok(RoxMessage {
            header: MessageHeader {
                key,
                timestamp: RoxTimestamp {
                    hw_time,
                    logical_time,
                },
                qos: QoSMetadata {
                    priority,
                    reliability,
                    durability,
                    deadline_us,
                    lifespan_us,
                },
                source_id,
                sequence,
            },
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let msg = RoxMessage::new(
            KeyExpr::new("robot-01", "lidar", "scan"),
            NodeId("driver".to_string()),
            42,
            Bytes::from(vec![1, 2, 3, 4, 5]),
        )
        .with_qos(QoSMetadata {
            priority: Priority::High,
            reliability: Reliability::Reliable,
            durability: Durability::TransientLocal,
            deadline_us: Some(10_000),
            lifespan_us: Some(5_000_000),
        });

        let encoded = WireEncoder::encode(&msg).unwrap();
        let decoded = WireDecoder::decode(encoded).unwrap();

        assert_eq!(decoded.header.key.as_str(), "robot-01/lidar/scan");
        assert_eq!(decoded.header.source_id.0, "driver");
        assert_eq!(decoded.header.sequence, 42);
        assert_eq!(decoded.header.qos.priority, Priority::High);
        assert_eq!(decoded.header.qos.reliability, Reliability::Reliable);
        assert_eq!(decoded.header.qos.durability, Durability::TransientLocal);
        assert_eq!(decoded.header.qos.deadline_us, Some(10_000));
        assert_eq!(decoded.header.qos.lifespan_us, Some(5_000_000));
        assert_eq!(decoded.payload.as_ref(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_empty_payload() {
        let msg = RoxMessage::new(
            KeyExpr::global("status"),
            NodeId("monitor".to_string()),
            0,
            Bytes::new(),
        );

        let encoded = WireEncoder::encode(&msg).unwrap();
        let decoded = WireDecoder::decode(encoded).unwrap();

        assert_eq!(decoded.header.key.as_str(), "_global/status");
        assert!(decoded.payload.is_empty());
    }

    #[test]
    fn test_invalid_magic() {
        let data = Bytes::from(vec![0, 0, 0, 0]);
        assert!(WireDecoder::decode(data).is_err());
    }

    #[test]
    fn test_oversized_payload_rejected() {
        // Craft a frame with payload_len exceeding MAX_PAYLOAD_SIZE
        let msg = RoxMessage::new(
            KeyExpr::global("test"),
            NodeId("node".to_string()),
            0,
            Bytes::from(vec![0u8; 10]),
        );
        let mut encoded = WireEncoder::encode(&msg).unwrap();

        // Find and patch payload_len field to a huge value (>64MB)
        // payload_len is u32 LE at the end before the actual payload
        let mut buf = BytesMut::from(encoded.as_ref());
        let payload_offset = buf.len() - 10 - 4; // 10 bytes payload + 4 bytes length
        buf[payload_offset..payload_offset + 4].copy_from_slice(&(100_000_000u32).to_le_bytes());

        let result = WireDecoder::decode(buf.freeze());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("payload too large"), "got: {err}");
    }
}
