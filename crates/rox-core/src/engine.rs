//! RoxEngine — bootstrap and lifecycle management for the Rox runtime.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::config::RoxConfig;
use crate::graph::TaskGraph;
use crate::registry::NodeRegistry;
use crate::scheduler::Scheduler;
use crate::session::Session;
use crate::topic::TopicManager;
use rox_protocol::{
    KeyExpr, MessageReceiver, MessageSender, NodeContext, NodeId, QoSMetadata, RoxNode,
    TopicRegistry, VersionedPolicyUpdate,
};

/// Per-node rate tracking for cycle-based execution.
struct NodeExecutionState {
    /// How many base cycles between executions (derived from rate_hz).
    cycle_interval: u64,
    /// Last cycle this node was executed.
    last_executed: u64,
}

/// The Rox engine — orchestrates all components.
pub struct RoxEngine {
    config: RoxConfig,
    session: Session,
    task_graph: TaskGraph,
    policy_tx: mpsc::Sender<VersionedPolicyUpdate>,
    scheduler: Scheduler,
    /// Live node instances, keyed by node id.
    node_instances: HashMap<String, Box<dyn RoxNode>>,
    /// Per-node pub/sub context.
    node_contexts: HashMap<String, NodeContext>,
    /// Per-node rate tracking.
    execution_states: HashMap<String, NodeExecutionState>,
    /// Whether `init_nodes` has been called.
    nodes_initialized: bool,
    /// Base tick rate in Hz (derived from the fastest node).
    base_rate_hz: f64,
}

impl RoxEngine {
    /// Bootstrap a RoxEngine from a config file.
    pub fn from_config_file(path: &Path) -> Result<Self> {
        let config = RoxConfig::from_file(path)?;
        Self::from_config(config)
    }

    /// Bootstrap a RoxEngine from a config struct.
    pub fn from_config(config: RoxConfig) -> Result<Self> {
        let topic_manager = Arc::new(TopicManager::new());
        let session = Session::with_topic_manager("rox-main", topic_manager);

        let task_graph = TaskGraph::build(&config.nodes, &config.connections)
            .context("failed to build task graph")?;

        let execution_order: Vec<String> = task_graph.execution_order().to_vec();
        let (policy_tx, policy_rx) = mpsc::channel(256);
        let scheduler = Scheduler::new(execution_order, policy_rx);

        // Determine base rate from fastest node (or default 100Hz)
        let base_rate_hz = config
            .nodes
            .iter()
            .filter_map(|n| n.rate_hz)
            .fold(0.0f64, f64::max)
            .max(1.0);

        info!(
            nodes = task_graph.node_count(),
            edges = task_graph.edge_count(),
            base_rate_hz = base_rate_hz,
            "rox engine initialized"
        );

        Ok(Self {
            config,
            session,
            task_graph,
            policy_tx,
            scheduler,
            node_instances: HashMap::new(),
            node_contexts: HashMap::new(),
            execution_states: HashMap::new(),
            nodes_initialized: false,
            base_rate_hz,
        })
    }

    /// Initialize nodes from a registry.
    ///
    /// Creates node instances, builds their pub/sub contexts,
    /// and calls `init()` on each node in topological order.
    pub async fn init_nodes(&mut self, registry: &NodeRegistry) -> Result<()> {
        registry.validate_config(&self.config.nodes)?;

        let topic_manager = self.session.topic_manager().clone();

        for node_id in self.task_graph.execution_order().to_vec() {
            let node_config = self
                .task_graph
                .node_config(&node_id)
                .ok_or_else(|| anyhow::anyhow!("node config not found: {node_id}"))?
                .clone();

            let mut node = registry.create(&node_config)?;

            // Build NodeContext with publishers and subscribers from connections
            let ctx = self
                .build_node_context(&node_id, &topic_manager)
                .await?;

            // Compute cycle interval from rate_hz
            let cycle_interval = if let Some(rate_hz) = node_config.rate_hz {
                (self.base_rate_hz / rate_hz).round().max(1.0) as u64
            } else {
                1 // execute every cycle
            };

            self.execution_states.insert(
                node_id.clone(),
                NodeExecutionState {
                    cycle_interval,
                    last_executed: 0,
                },
            );

            // Store context temporarily, call init, then store both
            let mut ctx = ctx;
            node.init(&mut ctx)
                .await
                .with_context(|| format!("failed to init node '{node_id}'"))?;

            info!(node = %node_id, name = node.name(), "node initialized");
            self.node_instances.insert(node_id.clone(), node);
            self.node_contexts.insert(node_id, ctx);
        }

        self.nodes_initialized = true;
        info!(count = self.node_instances.len(), "all nodes initialized");
        Ok(())
    }

    /// Build a NodeContext for a node based on its connections in the task graph.
    async fn build_node_context(
        &self,
        node_id: &str,
        topic_manager: &Arc<TopicManager>,
    ) -> Result<NodeContext> {
        let mut publishers: HashMap<String, MessageSender> = HashMap::new();
        let mut subscribers: HashMap<String, MessageReceiver> = HashMap::new();

        // Find outgoing connections (this node publishes)
        for dependent in self.task_graph.dependents(node_id) {
            // Connection format: "node_id/port" -> "dependent/port"
            // We need the topic key for the pub/sub
            let topic_key = format!("{node_id}/{dependent}");
            let key = KeyExpr(topic_key.clone());
            let sender = topic_manager.publish(&key, QoSMetadata::default()).await?;
            publishers.insert(topic_key, sender);
        }

        // Find incoming connections (this node subscribes)
        for dependency in self.task_graph.dependencies(node_id) {
            let topic_key = format!("{dependency}/{node_id}");
            let key = KeyExpr(topic_key.clone());
            let receiver = topic_manager.subscribe(&key).await?;
            subscribers.insert(topic_key, receiver);
        }

        Ok(NodeContext {
            publishers,
            subscribers,
            node_id: NodeId(node_id.to_string()),
        })
    }

    /// Run a single scheduler cycle, executing nodes that are due.
    ///
    /// If `init_nodes` has not been called, falls back to trace-only mode
    /// (backward-compatible with existing tests).
    pub async fn tick(&mut self) -> Result<()> {
        let cycle_start = Instant::now();
        let plan = self.scheduler.prepare_cycle();
        let cycle = plan.cycle;

        if self.nodes_initialized {
            let mut nodes_executed = 0usize;

            for node_id in &plan.execution_order {
                if self.should_execute(node_id, cycle) {
                    if let (Some(node), Some(ctx)) = (
                        self.node_instances.get_mut(node_id),
                        self.node_contexts.get_mut(node_id),
                    ) {
                        node.tick(ctx).await.with_context(|| {
                            format!("node '{node_id}' tick failed at cycle {cycle}")
                        })?;
                        nodes_executed += 1;

                        if let Some(state) = self.execution_states.get_mut(node_id) {
                            state.last_executed = cycle;
                        }
                    }
                }
            }

            debug!(cycle, nodes_executed, "cycle complete");
            self.scheduler.complete_cycle(nodes_executed, cycle_start);
        } else {
            // Fallback: no nodes registered, use run_cycle with trace callback
            let _metrics = self
                .scheduler
                .run_cycle(|node_id, cycle| {
                    tracing::trace!(node = node_id, cycle = cycle, "tick");
                    Ok(())
                })
                .await?;
        }

        Ok(())
    }

    /// Check if a node should execute this cycle based on its rate_hz.
    fn should_execute(&self, node_id: &str, cycle: u64) -> bool {
        match self.execution_states.get(node_id) {
            Some(state) => {
                if cycle == 0 {
                    return true;
                }
                cycle - state.last_executed >= state.cycle_interval
            }
            None => true,
        }
    }

    /// Get a policy sender for the Agent runtime to send policy updates.
    pub fn policy_sender(&self) -> mpsc::Sender<VersionedPolicyUpdate> {
        self.policy_tx.clone()
    }

    /// Get a reference to the session.
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Get a reference to the config.
    pub fn config(&self) -> &RoxConfig {
        &self.config
    }

    /// Get a reference to the task graph.
    pub fn task_graph(&self) -> &TaskGraph {
        &self.task_graph
    }

    /// Get a mutable reference to the scheduler.
    pub fn scheduler_mut(&mut self) -> &mut Scheduler {
        &mut self.scheduler
    }

    /// Base tick rate in Hz.
    pub fn base_rate_hz(&self) -> f64 {
        self.base_rate_hz
    }

    /// Whether nodes have been initialized.
    pub fn is_initialized(&self) -> bool {
        self.nodes_initialized
    }

    /// Number of registered node instances.
    pub fn node_count(&self) -> usize {
        self.node_instances.len()
    }

    /// Shutdown the engine, calling shutdown() on all nodes.
    pub async fn shutdown(mut self) -> Result<()> {
        info!("rox engine shutting down");

        // Shutdown nodes in reverse order
        let order: Vec<String> = self
            .task_graph
            .execution_order()
            .iter()
            .rev()
            .cloned()
            .collect();

        for node_id in &order {
            if let (Some(node), Some(ctx)) = (
                self.node_instances.get_mut(node_id),
                self.node_contexts.get_mut(node_id),
            ) {
                if let Err(e) = node.shutdown(ctx).await {
                    tracing::warn!(node = %node_id, error = %e, "node shutdown error");
                }
            }
        }

        self.session.close().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use bytes::Bytes;
    use rox_protocol::RoxMessage;
    use std::sync::atomic::{AtomicU64, Ordering};

    // --- Backward compatibility test (no nodes registered) ---

    #[tokio::test]
    async fn test_engine_from_config() {
        let yaml = r#"
transport:
  tcp:
    bind: "0.0.0.0:7447"
nodes:
  - id: sensor
    node_type: "test::Sensor"
    rate_hz: 10.0
    priority: 0
    background: false
  - id: processor
    node_type: "test::Processor"
    rate_hz: 10.0
    priority: 1
    background: false
connections:
  - from: "sensor/output"
    to: "processor/input"
"#;

        let config = RoxConfig::from_str(yaml).unwrap();
        let mut engine = RoxEngine::from_config(config).unwrap();

        assert_eq!(engine.task_graph().node_count(), 2);

        engine.tick().await.unwrap();
        assert_eq!(engine.scheduler_mut().current_cycle(), 1);
    }

    // --- Node execution test ---

    struct CounterNode {
        ticks: Arc<AtomicU64>,
    }

    #[async_trait]
    impl RoxNode for CounterNode {
        fn name(&self) -> &str {
            "counter"
        }
        async fn init(&mut self, _ctx: &mut NodeContext) -> Result<()> {
            Ok(())
        }
        async fn tick(&mut self, ctx: &mut NodeContext) -> Result<()> {
            let seq = self.ticks.fetch_add(1, Ordering::Relaxed);
            for (_, sender) in &ctx.publishers {
                let msg = Arc::new(RoxMessage::new(
                    KeyExpr::new("test", "counter", "out"),
                    ctx.node_id.clone(),
                    seq,
                    Bytes::from(seq.to_le_bytes().to_vec()),
                ));
                let _ = sender.send(msg);
            }
            Ok(())
        }
        async fn shutdown(&mut self, _ctx: &mut NodeContext) -> Result<()> {
            Ok(())
        }
    }

    struct LoggerNode {
        ticks: Arc<AtomicU64>,
    }

    #[async_trait]
    impl RoxNode for LoggerNode {
        fn name(&self) -> &str {
            "logger"
        }
        async fn init(&mut self, _ctx: &mut NodeContext) -> Result<()> {
            Ok(())
        }
        async fn tick(&mut self, ctx: &mut NodeContext) -> Result<()> {
            self.ticks.fetch_add(1, Ordering::Relaxed);
            for (_, rx) in &mut ctx.subscribers {
                while let Ok(_msg) = rx.try_recv() {}
            }
            Ok(())
        }
        async fn shutdown(&mut self, _ctx: &mut NodeContext) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_engine_with_nodes() {
        let counter_ticks = Arc::new(AtomicU64::new(0));
        let logger_ticks = Arc::new(AtomicU64::new(0));
        let ct = counter_ticks.clone();
        let lt = logger_ticks.clone();

        let yaml = r#"
transport:
  tcp:
    bind: "0.0.0.0:7447"
nodes:
  - id: counter
    node_type: "test::Counter"
    rate_hz: 10.0
    priority: 0
    background: false
  - id: logger
    node_type: "test::Logger"
    rate_hz: 10.0
    priority: 1
    background: false
connections:
  - from: "counter/output"
    to: "logger/input"
"#;

        let config = RoxConfig::from_str(yaml).unwrap();
        let mut engine = RoxEngine::from_config(config).unwrap();

        let mut registry = NodeRegistry::new();
        registry.register("test::Counter", move || {
            Box::new(CounterNode {
                ticks: ct.clone(),
            })
        });
        registry.register("test::Logger", move || {
            Box::new(LoggerNode {
                ticks: lt.clone(),
            })
        });

        engine.init_nodes(&registry).await.unwrap();
        assert!(engine.is_initialized());
        assert_eq!(engine.node_count(), 2);

        for _ in 0..5 {
            engine.tick().await.unwrap();
        }

        assert_eq!(counter_ticks.load(Ordering::Relaxed), 5);
        assert_eq!(logger_ticks.load(Ordering::Relaxed), 5);
    }

    #[tokio::test]
    async fn test_engine_rate_limiting() {
        let counter_ticks = Arc::new(AtomicU64::new(0));
        let logger_ticks = Arc::new(AtomicU64::new(0));
        let ct = counter_ticks.clone();
        let lt = logger_ticks.clone();

        let yaml = r#"
transport:
  tcp:
    bind: "0.0.0.0:7447"
nodes:
  - id: counter
    node_type: "test::Counter"
    rate_hz: 100.0
    priority: 0
    background: false
  - id: logger
    node_type: "test::Logger"
    rate_hz: 50.0
    priority: 1
    background: false
connections:
  - from: "counter/output"
    to: "logger/input"
"#;

        let config = RoxConfig::from_str(yaml).unwrap();
        let mut engine = RoxEngine::from_config(config).unwrap();

        let mut registry = NodeRegistry::new();
        registry.register("test::Counter", move || {
            Box::new(CounterNode {
                ticks: ct.clone(),
            })
        });
        registry.register("test::Logger", move || {
            Box::new(LoggerNode {
                ticks: lt.clone(),
            })
        });

        engine.init_nodes(&registry).await.unwrap();

        // base_rate = 100Hz, counter every 1 cycle, logger every 2 cycles
        for _ in 0..10 {
            engine.tick().await.unwrap();
        }

        assert_eq!(counter_ticks.load(Ordering::Relaxed), 10);
        assert_eq!(logger_ticks.load(Ordering::Relaxed), 5);
    }

    #[tokio::test]
    async fn test_missing_factory_error() {
        let yaml = r#"
transport:
  tcp:
    bind: "0.0.0.0:7447"
nodes:
  - id: node1
    node_type: "test::Missing"
    rate_hz: 10.0
    priority: 0
    background: false
"#;

        let config = RoxConfig::from_str(yaml).unwrap();
        let mut engine = RoxEngine::from_config(config).unwrap();

        let registry = NodeRegistry::new();
        let result = engine.init_nodes(&registry).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("test::Missing"));
    }
}
