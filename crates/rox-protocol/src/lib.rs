//! # rox-protocol
//!
//! Wire protocol definitions for Rox messaging system.
//! Defines RoxMessage, MessageHeader, KeyExpr, QoS types, and wire encoding.

pub mod contracts;
pub mod mock;

mod keyexpr;
mod message;
mod wire;

pub use contracts::*;
pub use keyexpr::KeyExprMatcher;
pub use wire::{WireDecoder, WireEncoder};
