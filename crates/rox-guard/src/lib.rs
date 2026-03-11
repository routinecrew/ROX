//! # rox-guard
//!
//! Safety validation layer for Rox.
//! Command validation, safety boundaries, watchdog, fail-safe policies,
//! and tamper-proof audit logging.

pub mod contracts;
pub mod mock;

pub use contracts::*;
