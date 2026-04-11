//! # rox-core
//!
//! Core runtime engine for Rox.
//! Session management, Node lifecycle, Topic Pub/Sub,
//! TaskGraph scheduling, Config loading, and hot-reload.

pub mod contracts;
pub mod mock;

mod config;
mod engine;
mod graph;
mod registry;
mod scheduler;
mod session;
mod topic;

pub use config::{ConfigWatcher, RoxConfig};
pub use contracts::*;
pub use engine::RoxEngine;
pub use graph::TaskGraph;
pub use registry::{NodeFactory, NodeRegistry};
pub use scheduler::{CyclePlan, Scheduler};
pub use session::Session;
pub use topic::TopicManager;
