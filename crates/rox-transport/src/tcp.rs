//! TCP transport implementation.

use anyhow::{Context, Result};
use bytes::{Bytes, BytesMut};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::{debug, info};

use rox_protocol::{TransportKind, TransportMetrics};

/// TCP transport for cross-machine communication.
pub struct TcpTransport {
    bind_addr: String,
    /// Metrics emitted for the agent to consume.
    metrics_tx: Option<mpsc::Sender<TransportMetrics>>,
    /// Total bytes sent (for bandwidth tracking).
    bytes_sent: Arc<std::sync::atomic::AtomicU64>,
    bytes_recv: Arc<std::sync::atomic::AtomicU64>,
}

impl TcpTransport {
    /// Create a new TCP transport bound to the given address.
    pub fn new(bind_addr: impl Into<String>) -> Self {
        Self {
            bind_addr: bind_addr.into(),
            metrics_tx: None,
            bytes_sent: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            bytes_recv: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Attach a metrics channel for the Agent runtime.
    pub fn with_metrics(mut self, tx: mpsc::Sender<TransportMetrics>) -> Self {
        self.metrics_tx = Some(tx);
        self
    }

    /// Start listening for incoming connections.
    /// Returns a handle to manage the listener.
    pub async fn listen(&self) -> Result<TcpListenerHandle> {
        let listener = TcpListener::bind(&self.bind_addr)
            .await
            .with_context(|| format!("failed to bind TCP on {}", self.bind_addr))?;
        let local_addr = listener.local_addr()?;
        info!(addr = %local_addr, "TCP transport listening");

        let (shutdown_tx, _shutdown_rx) = broadcast::channel::<()>(1);
        let bytes_recv = Arc::clone(&self.bytes_recv);

        Ok(TcpListenerHandle {
            local_addr: local_addr.to_string(),
            listener,
            shutdown_tx,
            bytes_recv,
        })
    }

    /// Connect to a remote TCP peer.
    pub async fn connect(&self, addr: &str) -> Result<TcpConnection> {
        let stream = TcpStream::connect(addr)
            .await
            .with_context(|| format!("failed to connect to {addr}"))?;
        info!(peer = addr, "TCP connected");

        Ok(TcpConnection {
            stream: Arc::new(Mutex::new(stream)),
            peer_addr: addr.to_string(),
            bytes_sent: Arc::clone(&self.bytes_sent),
        })
    }

    /// Transport kind identifier.
    pub fn kind(&self) -> TransportKind {
        TransportKind::Tcp
    }

    /// Estimated latency in microseconds.
    pub fn estimated_latency_us(&self) -> u64 {
        200 // Typical TCP localhost: ~200us
    }

    /// Bandwidth usage ratio (0.0 to 1.0).
    pub fn bandwidth_usage(&self) -> f32 {
        0.0 // TODO: compute from bytes_sent rate
    }
}

/// Handle for an active TCP listener.
pub struct TcpListenerHandle {
    pub local_addr: String,
    listener: TcpListener,
    shutdown_tx: broadcast::Sender<()>,
    bytes_recv: Arc<std::sync::atomic::AtomicU64>,
}

impl TcpListenerHandle {
    /// Accept the next incoming connection.
    pub async fn accept(&self) -> Result<TcpConnection> {
        let (stream, peer) = self.listener.accept().await?;
        debug!(peer = %peer, "TCP connection accepted");
        Ok(TcpConnection {
            stream: Arc::new(Mutex::new(stream)),
            peer_addr: peer.to_string(),
            bytes_sent: Arc::clone(&self.bytes_recv),
        })
    }

    /// Signal shutdown.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }
}

/// An active TCP connection for sending/receiving wire-encoded messages.
pub struct TcpConnection {
    stream: Arc<Mutex<TcpStream>>,
    peer_addr: String,
    bytes_sent: Arc<std::sync::atomic::AtomicU64>,
}

impl TcpConnection {
    /// Send raw bytes over the connection.
    /// Wire format: [length: 4B LE] [data: variable]
    pub async fn send_bytes(&self, data: &[u8]) -> Result<()> {
        let mut stream = self.stream.lock().await;
        let len = (data.len() as u32).to_le_bytes();
        stream.write_all(&len).await?;
        stream.write_all(data).await?;
        stream.flush().await?;
        self.bytes_sent
            .fetch_add(data.len() as u64 + 4, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// Receive raw bytes from the connection.
    pub async fn recv_bytes(&self) -> Result<Bytes> {
        let mut stream = self.stream.lock().await;
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;
        let len = u32::from_le_bytes(len_buf) as usize;

        let mut data = BytesMut::zeroed(len);
        stream.read_exact(&mut data).await?;
        Ok(data.freeze())
    }

    /// Peer address.
    pub fn peer_addr(&self) -> &str {
        &self.peer_addr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tcp_connect_and_send() {
        let server = TcpTransport::new("127.0.0.1:0");
        let handle = server.listen().await.unwrap();
        let addr = handle.local_addr.clone();

        let client_transport = TcpTransport::new("127.0.0.1:0");

        let server_task = tokio::spawn(async move {
            let conn = handle.accept().await.unwrap();
            let data = conn.recv_bytes().await.unwrap();
            assert_eq!(data.as_ref(), b"hello rox");
        });

        let client = client_transport.connect(&addr).await.unwrap();
        client.send_bytes(b"hello rox").await.unwrap();

        server_task.await.unwrap();
    }
}
