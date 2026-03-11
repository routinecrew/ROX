//! # rox-transport
//!
//! Multi-transport layer for Rox.
//! Supports SHM (iceoryx2), TCP/TLS, UDP, and Serial transports.
//! Includes TransportSelector with Agent-driven dynamic switching.

pub mod contracts;
pub mod mock;

pub use contracts::*;
