//! TaskGraph — DAG-based task scheduling (copper-rs inspired).
//!
//! Parses node connections to build a directed acyclic graph,
//! then computes a topological execution order for deterministic scheduling.

use anyhow::{bail, Context, Result};
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;

use rox_protocol::{ConnectionConfig, NodeConfig};

/// A directed acyclic graph of task nodes with computed execution order.
pub struct TaskGraph {
    graph: DiGraph<String, ConnectionEdge>,
    node_indices: HashMap<String, NodeIndex>,
    node_configs: HashMap<String, NodeConfig>,
    execution_order: Vec<String>,
}

/// Edge metadata in the task graph.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ConnectionEdge {
    pub from_port: String,
    pub to_port: String,
}

impl TaskGraph {
    /// Build a TaskGraph from node and connection configurations.
    pub fn build(nodes: &[NodeConfig], connections: &[ConnectionConfig]) -> Result<Self> {
        let mut graph = DiGraph::new();
        let mut node_indices = HashMap::new();
        let mut node_configs = HashMap::new();

        // Add nodes
        for node in nodes {
            let idx = graph.add_node(node.id.clone());
            node_indices.insert(node.id.clone(), idx);
            node_configs.insert(node.id.clone(), node.clone());
        }

        // Add edges from connections
        for conn in connections {
            let (from_node, from_port) = parse_endpoint(&conn.from)
                .with_context(|| format!("invalid 'from' endpoint: {}", conn.from))?;
            let (to_node, to_port) = parse_endpoint(&conn.to)
                .with_context(|| format!("invalid 'to' endpoint: {}", conn.to))?;

            let from_idx = node_indices
                .get(from_node)
                .ok_or_else(|| anyhow::anyhow!("unknown node in connection: {from_node}"))?;
            let to_idx = node_indices
                .get(to_node)
                .ok_or_else(|| anyhow::anyhow!("unknown node in connection: {to_node}"))?;

            graph.add_edge(
                *from_idx,
                *to_idx,
                ConnectionEdge {
                    from_port: from_port.to_string(),
                    to_port: to_port.to_string(),
                },
            );
        }

        // Compute topological sort for deterministic execution order
        let sorted = toposort(&graph, None).map_err(|cycle| {
            let node_id = &graph[cycle.node_id()];
            anyhow::anyhow!("cycle detected in task graph at node: {node_id}")
        })?;

        // Sort by priority within each topological level
        let execution_order: Vec<String> =
            sorted.into_iter().map(|idx| graph[idx].clone()).collect();

        Ok(Self {
            graph,
            node_indices,
            node_configs,
            execution_order,
        })
    }

    /// Get the deterministic execution order.
    pub fn execution_order(&self) -> &[String] {
        &self.execution_order
    }

    /// Get configuration for a specific node.
    pub fn node_config(&self, id: &str) -> Option<&NodeConfig> {
        self.node_configs.get(id)
    }

    /// Get all node IDs.
    pub fn node_ids(&self) -> Vec<&str> {
        self.node_configs.keys().map(|s| s.as_str()).collect()
    }

    /// Number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Number of connections in the graph.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Get upstream dependencies for a node.
    pub fn dependencies(&self, node_id: &str) -> Vec<&str> {
        if let Some(&idx) = self.node_indices.get(node_id) {
            self.graph
                .neighbors_directed(idx, petgraph::Direction::Incoming)
                .map(|n| self.graph[n].as_str())
                .collect()
        } else {
            vec![]
        }
    }

    /// Get downstream dependents of a node.
    pub fn dependents(&self, node_id: &str) -> Vec<&str> {
        if let Some(&idx) = self.node_indices.get(node_id) {
            self.graph
                .neighbors_directed(idx, petgraph::Direction::Outgoing)
                .map(|n| self.graph[n].as_str())
                .collect()
        } else {
            vec![]
        }
    }
}

/// Parse "node_id/port_name" into (node_id, port_name).
fn parse_endpoint(endpoint: &str) -> Result<(&str, &str)> {
    let parts: Vec<&str> = endpoint.splitn(2, '/').collect();
    if parts.len() != 2 {
        bail!("endpoint must be 'node_id/port_name', got: {endpoint}");
    }
    Ok((parts[0], parts[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_nodes() -> Vec<NodeConfig> {
        vec![
            NodeConfig {
                id: "lidar_driver".to_string(),
                node_type: "drivers::Lidar".to_string(),
                rate_hz: Some(10.0),
                priority: 0,
                background: false,
            },
            NodeConfig {
                id: "filter".to_string(),
                node_type: "perception::VoxelFilter".to_string(),
                rate_hz: Some(10.0),
                priority: 1,
                background: false,
            },
            NodeConfig {
                id: "planner".to_string(),
                node_type: "planning::AStar".to_string(),
                rate_hz: Some(5.0),
                priority: 2,
                background: false,
            },
        ]
    }

    fn test_connections() -> Vec<ConnectionConfig> {
        vec![
            ConnectionConfig {
                from: "lidar_driver/scan".to_string(),
                to: "filter/input".to_string(),
                qos: None,
            },
            ConnectionConfig {
                from: "filter/output".to_string(),
                to: "planner/pointcloud".to_string(),
                qos: None,
            },
        ]
    }

    #[test]
    fn test_build_graph() {
        let graph = TaskGraph::build(&test_nodes(), &test_connections()).unwrap();
        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2);
    }

    #[test]
    fn test_execution_order() {
        let graph = TaskGraph::build(&test_nodes(), &test_connections()).unwrap();
        let order = graph.execution_order();

        // lidar_driver must come before filter, filter before planner
        let lidar_pos = order.iter().position(|n| n == "lidar_driver").unwrap();
        let filter_pos = order.iter().position(|n| n == "filter").unwrap();
        let planner_pos = order.iter().position(|n| n == "planner").unwrap();

        assert!(lidar_pos < filter_pos);
        assert!(filter_pos < planner_pos);
    }

    #[test]
    fn test_dependencies() {
        let graph = TaskGraph::build(&test_nodes(), &test_connections()).unwrap();

        assert!(graph.dependencies("lidar_driver").is_empty());
        assert_eq!(graph.dependencies("filter"), vec!["lidar_driver"]);
        assert_eq!(graph.dependencies("planner"), vec!["filter"]);
    }

    #[test]
    fn test_cycle_detection() {
        let nodes = vec![
            NodeConfig {
                id: "a".to_string(),
                node_type: "A".to_string(),
                rate_hz: None,
                priority: 0,
                background: false,
            },
            NodeConfig {
                id: "b".to_string(),
                node_type: "B".to_string(),
                rate_hz: None,
                priority: 0,
                background: false,
            },
        ];
        let connections = vec![
            ConnectionConfig {
                from: "a/out".to_string(),
                to: "b/in".to_string(),
                qos: None,
            },
            ConnectionConfig {
                from: "b/out".to_string(),
                to: "a/in".to_string(),
                qos: None,
            },
        ];

        let result = TaskGraph::build(&nodes, &connections);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("cycle"));
    }
}
