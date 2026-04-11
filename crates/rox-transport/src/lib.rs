//! # rox-transport
//!
//! Multi-transport layer for Rox.
//! Supports TCP, UDP, Serial transports with dynamic selection.
//! SHM (iceoryx2) available via feature flag `shm`.

pub mod contracts;
pub mod mock;

mod manager;
mod selector;
mod tcp;
mod udp;

pub use contracts::*;
pub use manager::{MessageInjector, PeerConfig, TransportManager};
pub use selector::TransportSelector;
pub use tcp::{TcpConnection, TcpListenerHandle, TcpTransport};
pub use udp::UdpTransport;

#[cfg(feature = "shm")]
mod shm;

#[cfg(feature = "shm")]
pub use shm::ShmTransport;
