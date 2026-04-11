//! # rox-agent
//!
//! Intelligent communication control agents for Rox.
//! Operates OUTSIDE the hot path — manages transport metadata only.
//!
//! Agents:
//! - QoS Prediction: predict congestion, pre-adjust priorities
//! - Self-healing: detect failures, compute backup routes
//! - Anomaly Detection: statistical anomaly detection
//! - Throttle: frequency limiting for bandwidth savings

pub mod contracts;
pub mod mock;

mod anomaly_agent;
mod healing_agent;
mod qos_agent;
mod rule_engine;
mod runtime;
mod throttle_agent;

pub use anomaly_agent::AnomalyDetectionAgent;
pub use contracts::*;
pub use healing_agent::SelfHealingAgent;
pub use qos_agent::QoSPredictionAgent;
pub use rule_engine::RuleEngine;
pub use runtime::AgentRuntime;
pub use throttle_agent::ThrottleAgent;
