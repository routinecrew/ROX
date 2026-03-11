//! # rox
//!
//! Unified entry crate for Rox — Intelligent Nerve System for Robotics.
//!
//! ```toml
//! [dependencies]
//! rox = { version = "0.1", features = ["agent", "guard"] }
//! ```

pub use rox_core::*;
pub use rox_protocol::*;
pub use rox_codec as codec;
pub use rox_buffer as buffer;
pub use rox_transport as transport;
pub use rox_log as log;

#[cfg(feature = "agent")]
pub use rox_agent as agent;

#[cfg(feature = "guard")]
pub use rox_guard as guard;

#[cfg(feature = "bridge-ros2")]
pub use rox_bridge as bridge;
