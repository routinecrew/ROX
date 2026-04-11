//! # rox-guard
//!
//! Safety validation layer for Rox.
//! Command validation, safety boundaries, watchdog, fail-safe policies,
//! and tamper-proof blake3 hash-chained audit logging.

pub mod contracts;
pub mod mock;

mod audit;
mod boundary;
mod validator;
mod watchdog;

pub use audit::{verify_audit_log, AuditLogger};
pub use boundary::{GeoFence, SafetyBoundary};
pub use contracts::*;
pub use validator::CommandValidator;
pub use watchdog::GuardWatchdog;
