//! # rox-cli
//!
//! CLI tool for Rox.
//! Commands: `rox new`, `rox run`, `rox monitor`, `rox replay`.

pub mod contracts;
pub mod mock;

mod commands;

pub use commands::{exec_new, exec_run, Cli, Commands};
pub use contracts::*;
