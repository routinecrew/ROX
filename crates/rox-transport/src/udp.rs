//! UDP transport implementation for low-latency messaging.

use anyhow::{Context, Result};
use bytes::Bytes;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tracing::{debug, info};

use rox_protocol::TransportKind;

/// UDP transport for low-latency, best-effort messaging.
pub struct UdpTransport {
    bind_addr: String,
    socket: Option<Arc<UdpSocket>>,
    max_datagram_size: usize,
}

impl UdpTransport {
    /// Create a new UDP transport.
    pub fn new(bind_addr: impl Into<String>) -> Self {
        Self {
            bind_addr: bind_addr.into(),
            socket: None,
            max_datagram_size: 65507, // Max UDP payload
        }
    }

    /// Set maximum datagram size.
    pub fn with_max_datagram_size(mut self, size: usize) -> Self {
        self.max_datagram_size = size;
        self
    }

    /// Bind the UDP socket.
    pub async fn bind(&mut self) -> Result<()> {
        let socket = UdpSocket::bind(&self.bind_addr)
            .await
            .with_context(|| format!("failed to bind UDP on {}", self.bind_addr))?;
        let local = socket.local_addr()?;
        info!(addr = %local, "UDP transport bound");
        self.socket = Some(Arc::new(socket));
        Ok(())
    }

    /// Get the local address.
    pub fn local_addr(&self) -> Option<String> {
        self.socket
            .as_ref()
            .and_then(|s| s.local_addr().ok().map(|a| a.to_string()))
    }

    /// Send data to a specific address.
    pub async fn send_to(&self, data: &[u8], addr: &str) -> Result<usize> {
        let socket = self
            .socket
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("UDP socket not bound"))?;
        let target: SocketAddr = addr.parse().context("invalid target address")?;
        let sent = socket.send_to(data, target).await?;
        debug!(bytes = sent, target = addr, "UDP sent");
        Ok(sent)
    }

    /// Receive data from any sender.
    pub async fn recv_from(&self) -> Result<(Bytes, SocketAddr)> {
        let socket = self
            .socket
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("UDP socket not bound"))?;
        let mut buf = vec![0u8; self.max_datagram_size];
        let (len, addr) = socket.recv_from(&mut buf).await?;
        buf.truncate(len);
        Ok((Bytes::from(buf), addr))
    }

    /// Transport kind identifier.
    pub fn kind(&self) -> TransportKind {
        TransportKind::Udp
    }

    /// Estimated latency in microseconds.
    pub fn estimated_latency_us(&self) -> u64 {
        50 // UDP is typically faster than TCP
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_udp_send_recv() {
        let mut server = UdpTransport::new("127.0.0.1:0");
        server.bind().await.unwrap();
        let server_addr = server.local_addr().unwrap();

        let mut client = UdpTransport::new("127.0.0.1:0");
        client.bind().await.unwrap();

        let payload = b"hello udp";
        client.send_to(payload, &server_addr).await.unwrap();

        let (data, from) = server.recv_from().await.unwrap();
        assert_eq!(data.as_ref(), payload);
    }
}
