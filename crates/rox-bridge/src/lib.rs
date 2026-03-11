//! # rox-bridge
//!
//! Protocol bridges for Rox.
//! ROS 2 bridge (via zenoh-plugin-ros2dds wrapping), Zenoh compatibility bridge.

pub mod contracts;
pub mod mock;

pub use contracts::*;
