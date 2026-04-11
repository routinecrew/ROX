//! # rox-api
//!
//! REST API and SSE event streaming for Rox.
//! Built on Axum, serves on port 9090.

pub mod contracts;
pub mod mock;

mod routes;
mod server;

pub use contracts::*;
pub use server::ApiServer;
