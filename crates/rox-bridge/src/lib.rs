//! # rox-bridge
//!
//! Protocol bridges for Rox.
//! Provides translation between Rox messages and external protocols.
//! ROS 2 bridge (via zenoh-plugin-ros2dds wrapping concept).
//! Zenoh compatibility bridge.

pub mod contracts;
pub mod mock;

mod topic_bridge;

pub use contracts::*;
pub use topic_bridge::TopicBridge;
