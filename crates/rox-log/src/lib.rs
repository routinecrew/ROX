//! # rox-log
//!
//! Deterministic structured logging for Rox (copper cu29-log inspired).
//! Binary log format for high-throughput recording and bit-exact replay.

pub mod contracts;
pub mod mock;

mod logger;
mod reader;

pub use contracts::*;
pub use logger::{LogEntryType, RoxLogger};
pub use reader::{LogEntry, LogReader};
