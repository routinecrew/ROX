//! # rox
//!
//! Unified entry crate for Rox — Intelligent Nerve System for Robotics.
//!
//! ## Quick Start
//!
//! ```toml
//! [dependencies]
//! rox = { version = "0.1", features = ["agent", "guard"] }
//! ```
//!
//! ## Feature Flags
//!
//! - `agent` — AI communication control agents
//! - `guard` — Safety validation layer
//! - `bridge-ros2` — ROS 2 / Zenoh bridge
//! - `shm` — Shared memory transport (iceoryx2)
//! - `full` — All features enabled

// Re-export protocol types (canonical shared types)
pub use rox_protocol::*;

// Re-export core runtime types under `core` module to avoid ambiguity
pub use rox_core as core;

// Re-export sub-crates as modules
pub use rox_buffer as buffer;
pub use rox_codec as codec;
pub use rox_log as log;
pub use rox_transport as transport;

// Feature-gated re-exports
#[cfg(feature = "agent")]
pub use rox_agent as agent;

#[cfg(feature = "guard")]
pub use rox_guard as guard;

#[cfg(feature = "bridge-ros2")]
pub use rox_bridge as bridge;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use rox_buffer::{MemoryPool, ZBuf};
    pub use rox_codec::{BincodeCodec, RoxSerialize};
    pub use rox_core::{RoxConfig, RoxEngine, Session, TopicManager};
    pub use rox_protocol::{
        KeyExpr, MessageHeader, MessageReceiver, MessageSender, NodeContext, NodeId, Priority,
        QoSMetadata, Reliability, RoxMessage, RoxNode, RoxTimestamp, TopicRegistry,
    };

    #[cfg(feature = "agent")]
    pub use rox_agent::{AgentRuntime, RuleEngine};

    #[cfg(feature = "guard")]
    pub use rox_guard::{AuditLogger, CommandValidator};
}
