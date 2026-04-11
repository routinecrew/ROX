//! Bincode-based serialization codec.
//!
//! High-performance binary serialization for RoxMessage payloads.
//! Typical throughput: >1M msg/sec for small payloads.

use anyhow::{Context, Result};
use bytes::Bytes;
use serde::{de::DeserializeOwned, Serialize};

/// Trait for types that can be serialized into RoxMessage payloads.
pub trait RoxSerialize: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// Serialize into bytes using the default codec (bincode).
    fn to_bytes(&self) -> Result<Bytes> {
        BincodeCodec::encode(self)
    }

    /// Deserialize from bytes using the default codec (bincode).
    fn from_bytes(data: &[u8]) -> Result<Self> {
        BincodeCodec::decode(data)
    }
}

/// Blanket implementation: any type that is Serialize + DeserializeOwned + Send + Sync
/// automatically implements RoxSerialize.
impl<T> RoxSerialize for T where T: Serialize + DeserializeOwned + Send + Sync + 'static {}

/// Bincode codec for fast binary serialization.
pub struct BincodeCodec;

impl BincodeCodec {
    /// Encode a value into bytes.
    pub fn encode<T: Serialize>(value: &T) -> Result<Bytes> {
        let data = bincode::serialize(value).context("bincode encode failed")?;
        Ok(Bytes::from(data))
    }

    /// Decode a value from bytes.
    pub fn decode<T: DeserializeOwned>(data: &[u8]) -> Result<T> {
        bincode::deserialize(data).context("bincode decode failed")
    }

    /// Encode with size limit to prevent oversized payloads.
    pub fn encode_with_limit<T: Serialize>(value: &T, max_bytes: u64) -> Result<Bytes> {
        let data = bincode::serialize(value).context("bincode encode failed")?;
        if data.len() as u64 > max_bytes {
            anyhow::bail!("encoded size {} exceeds limit {}", data.len(), max_bytes);
        }
        Ok(Bytes::from(data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct PointCloud {
        points: Vec<[f32; 3]>,
        intensity: Vec<f32>,
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct VelocityCmd {
        linear_x: f64,
        linear_y: f64,
        angular_z: f64,
    }

    #[test]
    fn test_pointcloud_roundtrip() {
        let cloud = PointCloud {
            points: vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]],
            intensity: vec![0.5, 0.8],
        };

        let encoded = cloud.to_bytes().unwrap();
        let decoded = PointCloud::from_bytes(&encoded).unwrap();
        assert_eq!(cloud, decoded);
    }

    #[test]
    fn test_velocity_cmd_roundtrip() {
        let cmd = VelocityCmd {
            linear_x: 1.5,
            linear_y: 0.0,
            angular_z: -0.3,
        };

        let encoded = BincodeCodec::encode(&cmd).unwrap();
        let decoded: VelocityCmd = BincodeCodec::decode(&encoded).unwrap();
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn test_size_limit() {
        let large = vec![0u8; 1024];
        assert!(BincodeCodec::encode_with_limit(&large, 100).is_err());
        assert!(BincodeCodec::encode_with_limit(&large, 2048).is_ok());
    }
}
