//! # rox-codec
//!
//! Serialization and deserialization codecs for Rox messages.
//! Default: bincode. Optional: Apache Arrow (feature flag).

pub mod contracts;
pub mod mock;

pub use contracts::*;
