//! # rox-buffer
//!
//! Zero-copy buffer abstractions for Rox.
//! Includes ZBuf (zenoh-inspired), MemoryPool (copper-inspired), and SHM helpers.

pub mod contracts;
pub mod mock;

mod pool;
mod zbuf;

pub use contracts::*;
pub use pool::MemoryPool;
pub use zbuf::ZBuf;
