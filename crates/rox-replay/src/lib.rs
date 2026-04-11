//! # rox-replay
//!
//! Bit-exact replay engine for Rox.
//! Reads deterministic logs and replays them with identical timing.
//! Agent decisions are NOT re-executed — recorded policy values are injected.

pub mod contracts;
pub mod mock;

mod engine;

pub use contracts::*;
pub use engine::{ReplayClock, ReplayEngine, ReplayStats};
