//! # rox-agent
//!
//! Intelligent communication control agents for Rox.
//! QoS Prediction, Self-healing, Anomaly Detection, Throttling.
//! Operates outside the hot path — manages transport metadata only.

pub mod contracts;
pub mod mock;

pub use contracts::*;
