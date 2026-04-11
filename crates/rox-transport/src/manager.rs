//! TransportManager — bridges Transport layer with TopicManager.
//!
//! Handles:
//! - Outbound: local topic messages → WireEncoder → Transport.send
//! - Inbound: Transport.recv → WireDecoder → TopicManager.inject
//! - Peer management: static peer list with connect/disconnect
//! - Metrics collection for the Agent pipeline

use anyhow::{Context, Result};
use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use rox_protocol::{KeyExpr, RoxMessage, TransportMetrics, WireDecoder, WireEncoder};

use crate::tcp::{TcpConnection, TcpListenerHandle, TcpTransport};

/// Trait for injecting remote messages into the local topic bus.
///
/// This abstracts over TopicManager so rox-transport doesn't depend on rox-core directly.
#[async_trait::async_trait]
pub trait MessageInjector: Send + Sync + 'static {
    /// Inject a message received from a remote peer into the local topic bus.
    async fn inject(&self, msg: Arc<RoxMessage>) -> Result<usize>;
}

/// Configuration for a remote peer.
#[derive(Clone, Debug)]
pub struct PeerConfig {
    /// Peer address (e.g., "192.168.1.10:7447").
    pub addr: String,
    /// Optional topics to forward to this peer (empty = all).
    pub topics: Vec<String>,
}

/// Active connection to a remote peer.
struct PeerConnection {
    conn: Arc<TcpConnection>,
    /// Background task receiving from this peer.
    recv_task: JoinHandle<()>,
}

/// Manages transport connections and bridges them with the local topic bus.
///
/// # Architecture
///
/// ```text
/// Local Node → TopicManager → TransportManager → WireEncoder → TCP → Remote Peer
/// Remote Peer → TCP → WireDecoder → TransportManager → TopicManager.inject → Local Subscriber
/// ```
pub struct TransportManager {
    /// TCP transport instance.
    tcp: TcpTransport,
    /// Active peer connections, keyed by address.
    peers: HashMap<String, PeerConnection>,
    /// Message injector (TopicManager behind trait).
    injector: Arc<dyn MessageInjector>,
    /// Channel for outbound messages (local topics → remote peers).
    outbound_tx: mpsc::Sender<Arc<RoxMessage>>,
    outbound_rx: Option<mpsc::Receiver<Arc<RoxMessage>>>,
    /// Metrics channel for Agent consumption.
    metrics_tx: Option<mpsc::Sender<TransportMetrics>>,
    /// Listener handle (if listening).
    listener_handle: Option<TcpListenerHandle>,
    /// Background tasks.
    tasks: Vec<JoinHandle<()>>,
    /// Topics subscribed for outbound forwarding (future: topic-based routing).
    _forwarded_topics: Vec<KeyExpr>,
}

impl TransportManager {
    /// Create a new TransportManager.
    pub fn new(
        bind_addr: impl Into<String>,
        injector: Arc<dyn MessageInjector>,
    ) -> Self {
        let (outbound_tx, outbound_rx) = mpsc::channel(4096);
        Self {
            tcp: TcpTransport::new(bind_addr),
            peers: HashMap::new(),
            injector,
            outbound_tx,
            outbound_rx: Some(outbound_rx),
            metrics_tx: None,
            listener_handle: None,
            tasks: Vec::new(),
            _forwarded_topics: Vec::new(),
        }
    }

    /// Attach a metrics channel for Agent consumption.
    pub fn with_metrics(mut self, tx: mpsc::Sender<TransportMetrics>) -> Self {
        self.metrics_tx = Some(tx);
        self
    }

    /// Get the outbound sender for publishing messages to remote peers.
    ///
    /// Send messages to this channel and they'll be wire-encoded and forwarded
    /// to all connected peers.
    pub fn outbound_sender(&self) -> mpsc::Sender<Arc<RoxMessage>> {
        self.outbound_tx.clone()
    }

    /// Start listening for incoming peer connections.
    pub async fn start_listening(&mut self) -> Result<()> {
        let handle = self.tcp.listen().await?;
        info!(addr = %handle.local_addr, "transport manager listening");

        let _injector = self.injector.clone();
        let listener_addr = handle.local_addr.clone();

        // Spawn accept loop
        let task = tokio::spawn(async move {
            // Note: The accept loop runs until the listener is dropped.
            // In a full implementation, this would be integrated with shutdown signaling.
            debug!(addr = %listener_addr, "accept loop started");
        });
        self.tasks.push(task);

        self.listener_handle = Some(handle);
        Ok(())
    }

    /// Accept an incoming connection and start receiving messages from it.
    pub async fn accept_peer(&mut self) -> Result<String> {
        let handle = self
            .listener_handle
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("not listening"))?;

        let conn = handle.accept().await?;
        let peer_addr = conn.peer_addr().to_string();
        let conn = Arc::new(conn);

        let recv_task = self.spawn_recv_loop(peer_addr.clone(), conn.clone());

        self.peers.insert(
            peer_addr.clone(),
            PeerConnection {
                conn,
                recv_task,
            },
        );

        info!(peer = %peer_addr, "peer accepted");
        Ok(peer_addr)
    }

    /// Connect to a remote peer.
    pub async fn connect_peer(&mut self, addr: &str) -> Result<()> {
        let conn = self.tcp.connect(addr).await?;
        let conn = Arc::new(conn);

        let recv_task = self.spawn_recv_loop(addr.to_string(), conn.clone());

        self.peers.insert(
            addr.to_string(),
            PeerConnection {
                conn,
                recv_task,
            },
        );

        info!(peer = addr, "connected to peer");
        Ok(())
    }

    /// Disconnect from a peer.
    pub fn disconnect_peer(&mut self, addr: &str) {
        if let Some(peer) = self.peers.remove(addr) {
            peer.recv_task.abort();
            info!(peer = addr, "peer disconnected");
        }
    }

    /// Send a message to all connected peers.
    pub async fn broadcast(&self, msg: &Arc<RoxMessage>) -> Result<usize> {
        let wire_bytes = WireEncoder::encode(msg)?;
        let mut sent_count = 0;

        for (addr, peer) in &self.peers {
            if let Err(e) = peer.conn.send_bytes(&wire_bytes).await {
                warn!(peer = %addr, error = %e, "failed to send to peer");
            } else {
                sent_count += 1;
            }
        }

        debug!(peers = sent_count, key = %msg.header.key, "message broadcast");
        Ok(sent_count)
    }

    /// Send a message to a specific peer.
    pub async fn send_to(&self, addr: &str, msg: &Arc<RoxMessage>) -> Result<()> {
        let peer = self
            .peers
            .get(addr)
            .ok_or_else(|| anyhow::anyhow!("peer not connected: {addr}"))?;

        let wire_bytes = WireEncoder::encode(msg)?;
        peer.conn
            .send_bytes(&wire_bytes)
            .await
            .with_context(|| format!("failed to send to peer {addr}"))?;
        Ok(())
    }

    /// Start the outbound forwarding loop.
    ///
    /// Takes messages from the outbound channel and broadcasts them to all peers.
    pub fn start_outbound_loop(&mut self) -> Result<()> {
        let mut outbound_rx = self
            .outbound_rx
            .take()
            .ok_or_else(|| anyhow::anyhow!("outbound loop already started"))?;

        let peers = Arc::new(Mutex::new(HashMap::<String, Arc<TcpConnection>>::new()));

        // Copy current peer connections
        let peers_clone = peers.clone();
        for (addr, peer) in &self.peers {
            let peers_inner = peers_clone.clone();
            let addr = addr.clone();
            let conn = peer.conn.clone();
            tokio::spawn(async move {
                peers_inner.lock().await.insert(addr, conn);
            });
        }

        let task = tokio::spawn(async move {
            while let Some(msg) = outbound_rx.recv().await {
                match WireEncoder::encode(&msg) {
                    Ok(wire_bytes) => {
                        let conns = peers.lock().await;
                        for (addr, conn) in conns.iter() {
                            if let Err(e) = conn.send_bytes(&wire_bytes).await {
                                warn!(peer = %addr, error = %e, "outbound send failed");
                            }
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "wire encode failed");
                    }
                }
            }
        });
        self.tasks.push(task);
        Ok(())
    }

    /// Number of connected peers.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// List connected peer addresses.
    pub fn peer_addrs(&self) -> Vec<&str> {
        self.peers.keys().map(|s| s.as_str()).collect()
    }

    /// Shutdown all connections and background tasks.
    pub async fn shutdown(mut self) {
        for (addr, peer) in self.peers.drain() {
            peer.recv_task.abort();
            debug!(peer = %addr, "peer connection closed");
        }
        for task in self.tasks.drain(..) {
            task.abort();
        }
        if let Some(handle) = self.listener_handle.take() {
            handle.shutdown();
        }
        info!("transport manager shut down");
    }

    /// Spawn a receive loop for a peer connection.
    fn spawn_recv_loop(
        &self,
        peer_addr: String,
        conn: Arc<TcpConnection>,
    ) -> JoinHandle<()> {
        let injector = self.injector.clone();

        tokio::spawn(async move {
            loop {
                match conn.recv_bytes().await {
                    Ok(wire_bytes) => {
                        match WireDecoder::decode(wire_bytes) {
                            Ok(msg) => {
                                let key = msg.header.key.clone();
                                let msg = Arc::new(msg);

                                if let Err(e) = injector.inject(msg).await {
                                    warn!(
                                        peer = %peer_addr,
                                        topic = %key,
                                        error = %e,
                                        "failed to inject remote message"
                                    );
                                }
                            }
                            Err(e) => {
                                warn!(
                                    peer = %peer_addr,
                                    error = %e,
                                    "wire decode failed"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        info!(
                            peer = %peer_addr,
                            error = %e,
                            "peer connection closed"
                        );
                        break;
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rox_protocol::{NodeId, QoSMetadata};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Test injector that counts injected messages.
    struct CountingInjector {
        count: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl MessageInjector for CountingInjector {
        async fn inject(&self, _msg: Arc<RoxMessage>) -> Result<usize> {
            self.count.fetch_add(1, Ordering::Relaxed);
            Ok(1)
        }
    }

    #[tokio::test]
    async fn test_peer_connect_and_send() {
        let inject_count = Arc::new(AtomicUsize::new(0));

        // Server
        let server_injector = Arc::new(CountingInjector {
            count: inject_count.clone(),
        });
        let mut server = TransportManager::new("127.0.0.1:0", server_injector);
        server.start_listening().await.unwrap();

        let server_addr = server
            .listener_handle
            .as_ref()
            .unwrap()
            .local_addr
            .clone();

        // Client
        let client_injector = Arc::new(CountingInjector {
            count: Arc::new(AtomicUsize::new(0)),
        });
        let mut client = TransportManager::new("127.0.0.1:0", client_injector);

        // Connect client → server
        client.connect_peer(&server_addr).await.unwrap();
        assert_eq!(client.peer_count(), 1);

        // Accept on server side
        let peer_addr = server.accept_peer().await.unwrap();
        assert_eq!(server.peer_count(), 1);

        // Client sends a message to server
        let msg = Arc::new(RoxMessage::new(
            KeyExpr::new("robot-01", "lidar", "scan"),
            NodeId("sensor".to_string()),
            0,
            Bytes::from(vec![1, 2, 3]),
        ));

        client.send_to(&server_addr, &msg).await.unwrap();

        // Give the recv loop time to process
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert_eq!(inject_count.load(Ordering::Relaxed), 1);

        // Cleanup
        client.disconnect_peer(&server_addr);
        server.disconnect_peer(&peer_addr);
    }

    #[tokio::test]
    async fn test_broadcast_to_multiple_peers() {
        let inject_count = Arc::new(AtomicUsize::new(0));

        // Create two "servers" (listeners that will accept)
        let injector1 = Arc::new(CountingInjector {
            count: inject_count.clone(),
        });
        let mut server1 = TransportManager::new("127.0.0.1:0", injector1);
        server1.start_listening().await.unwrap();
        let addr1 = server1.listener_handle.as_ref().unwrap().local_addr.clone();

        let injector2 = Arc::new(CountingInjector {
            count: inject_count.clone(),
        });
        let mut server2 = TransportManager::new("127.0.0.1:0", injector2);
        server2.start_listening().await.unwrap();
        let addr2 = server2.listener_handle.as_ref().unwrap().local_addr.clone();

        // Client connects to both
        let client_injector = Arc::new(CountingInjector {
            count: Arc::new(AtomicUsize::new(0)),
        });
        let mut client = TransportManager::new("127.0.0.1:0", client_injector);
        client.connect_peer(&addr1).await.unwrap();
        client.connect_peer(&addr2).await.unwrap();
        assert_eq!(client.peer_count(), 2);

        // Accept on both servers
        server1.accept_peer().await.unwrap();
        server2.accept_peer().await.unwrap();

        // Broadcast a message
        let msg = Arc::new(RoxMessage::new(
            KeyExpr::new("robot-01", "motor", "cmd"),
            NodeId("planner".to_string()),
            1,
            Bytes::from(vec![10, 20]),
        ));

        let sent = client.broadcast(&msg).await.unwrap();
        assert_eq!(sent, 2);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(inject_count.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn test_wire_roundtrip_through_transport() {
        // Verify wire encode/decode integrity through actual TCP
        let received_msgs: Arc<Mutex<Vec<Arc<RoxMessage>>>> = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received_msgs.clone();

        struct CollectingInjector {
            msgs: Arc<Mutex<Vec<Arc<RoxMessage>>>>,
        }

        #[async_trait::async_trait]
        impl MessageInjector for CollectingInjector {
            async fn inject(&self, msg: Arc<RoxMessage>) -> Result<usize> {
                self.msgs.lock().await.push(msg);
                Ok(1)
            }
        }

        let server_injector = Arc::new(CollectingInjector {
            msgs: received_clone,
        });
        let mut server = TransportManager::new("127.0.0.1:0", server_injector);
        server.start_listening().await.unwrap();
        let server_addr = server.listener_handle.as_ref().unwrap().local_addr.clone();

        let client_injector = Arc::new(CountingInjector {
            count: Arc::new(AtomicUsize::new(0)),
        });
        let mut client = TransportManager::new("127.0.0.1:0", client_injector);
        client.connect_peer(&server_addr).await.unwrap();
        server.accept_peer().await.unwrap();

        // Send a message with full QoS
        let msg = Arc::new(
            RoxMessage::new(
                KeyExpr::new("arm-01", "joint", "position"),
                NodeId("controller".to_string()),
                42,
                Bytes::from(vec![0xDE, 0xAD, 0xBE, 0xEF]),
            )
            .with_qos(rox_protocol::QoSMetadata {
                priority: rox_protocol::Priority::High,
                reliability: rox_protocol::Reliability::Reliable,
                durability: rox_protocol::Durability::TransientLocal,
                deadline_us: Some(5000),
                lifespan_us: Some(1_000_000),
            }),
        );

        client.send_to(&server_addr, &msg).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let msgs = received_msgs.lock().await;
        assert_eq!(msgs.len(), 1);

        let received = &msgs[0];
        assert_eq!(received.header.key.as_str(), "arm-01/joint/position");
        assert_eq!(received.header.source_id.0, "controller");
        assert_eq!(received.header.sequence, 42);
        assert_eq!(received.payload.as_ref(), &[0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(received.header.qos.priority, rox_protocol::Priority::High);
        assert_eq!(received.header.qos.deadline_us, Some(5000));
    }
}
