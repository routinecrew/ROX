//! # rox-buffer
//!
//! Zero-copy buffer abstractions for Rox.
//! Includes ZBuf (zenoh-inspired), Memory Pool, and SHM management.

pub mod contracts;
pub mod mock;

pub use contracts::*;
