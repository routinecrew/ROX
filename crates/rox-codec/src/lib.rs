//! # rox-codec
//!
//! Serialization/deserialization codecs for Rox.
//! Default: bincode for high-performance binary serialization.
//! Optional: Apache Arrow for columnar data (feature = "arrow").

pub mod contracts;
pub mod mock;

mod bincode_codec;

pub use bincode_codec::{BincodeCodec, RoxSerialize};
pub use contracts::*;
