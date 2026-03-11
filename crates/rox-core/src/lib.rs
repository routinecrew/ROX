//! # rox-core
//!
//! Core runtime engine for Rox.
//! Session management, Node lifecycle, Topic Pub/Sub, Service Req/Rep,
//! TaskGraph scheduling, Discovery, QoS policy, and Config hot-reload.

pub mod contracts;
pub mod mock;

pub use contracts::*;
