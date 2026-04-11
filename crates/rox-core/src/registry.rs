//! NodeRegistry — factory-based node registration and instantiation.
//!
//! Users register their RoxNode implementations by type name,
//! then the Engine creates instances from config at startup.

use anyhow::{bail, Result};
use std::collections::HashMap;

use rox_protocol::{NodeConfig, RoxNode};

/// Factory function that creates a boxed RoxNode instance.
pub type NodeFactory = Box<dyn Fn() -> Box<dyn RoxNode> + Send + Sync>;

/// Registry of node factories, keyed by type name.
///
/// # Example
///
/// ```ignore
/// let mut registry = NodeRegistry::new();
/// registry.register("my_robot::SensorDriver", || Box::new(SensorDriver::new()));
///
/// let node = registry.create(&config)?;
/// ```
pub struct NodeRegistry {
    factories: HashMap<String, NodeFactory>,
}

impl NodeRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Register a node factory for a given type name.
    pub fn register<F>(&mut self, type_name: &str, factory: F)
    where
        F: Fn() -> Box<dyn RoxNode> + Send + Sync + 'static,
    {
        self.factories
            .insert(type_name.to_string(), Box::new(factory));
    }

    /// Create a node instance from a NodeConfig.
    ///
    /// Looks up the factory by `node_config.node_type` and calls it.
    pub fn create(&self, node_config: &NodeConfig) -> Result<Box<dyn RoxNode>> {
        let factory = self.factories.get(&node_config.node_type).ok_or_else(|| {
            anyhow::anyhow!(
                "no factory registered for node type '{}' (node id: '{}')",
                node_config.node_type,
                node_config.id,
            )
        })?;
        Ok(factory())
    }

    /// Check if a factory is registered for a type name.
    pub fn has(&self, type_name: &str) -> bool {
        self.factories.contains_key(type_name)
    }

    /// Number of registered factories.
    pub fn len(&self) -> usize {
        self.factories.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.factories.is_empty()
    }

    /// List all registered type names.
    pub fn registered_types(&self) -> Vec<&str> {
        self.factories.keys().map(|s| s.as_str()).collect()
    }

    /// Validate that all node types in a config have registered factories.
    pub fn validate_config(&self, nodes: &[NodeConfig]) -> Result<()> {
        let mut missing = Vec::new();
        for node in nodes {
            if !self.has(&node.node_type) {
                missing.push(format!("'{}' (node '{}')", node.node_type, node.id));
            }
        }
        if !missing.is_empty() {
            bail!(
                "missing node factories: {}",
                missing.join(", ")
            );
        }
        Ok(())
    }
}

impl Default for NodeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result as AnyhowResult;
    use async_trait::async_trait;
    use rox_protocol::NodeContext;

    struct DummyNode {
        name: String,
    }

    impl DummyNode {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
            }
        }
    }

    #[async_trait]
    impl RoxNode for DummyNode {
        fn name(&self) -> &str {
            &self.name
        }
        async fn init(&mut self, _ctx: &mut NodeContext) -> AnyhowResult<()> {
            Ok(())
        }
        async fn tick(&mut self, _ctx: &mut NodeContext) -> AnyhowResult<()> {
            Ok(())
        }
        async fn shutdown(&mut self, _ctx: &mut NodeContext) -> AnyhowResult<()> {
            Ok(())
        }
    }

    #[test]
    fn test_register_and_create() {
        let mut registry = NodeRegistry::new();
        registry.register("test::Dummy", || Box::new(DummyNode::new("dummy")));

        assert!(registry.has("test::Dummy"));
        assert_eq!(registry.len(), 1);

        let config = NodeConfig {
            id: "node1".to_string(),
            node_type: "test::Dummy".to_string(),
            rate_hz: Some(10.0),
            priority: 0,
            background: false,
        };

        let node = registry.create(&config).unwrap();
        assert_eq!(node.name(), "dummy");
    }

    #[test]
    fn test_missing_factory() {
        let registry = NodeRegistry::new();
        let config = NodeConfig {
            id: "node1".to_string(),
            node_type: "nonexistent::Type".to_string(),
            rate_hz: None,
            priority: 0,
            background: false,
        };

        assert!(registry.create(&config).is_err());
    }

    #[test]
    fn test_validate_config() {
        let mut registry = NodeRegistry::new();
        registry.register("test::A", || Box::new(DummyNode::new("a")));

        let nodes = vec![
            NodeConfig {
                id: "a".to_string(),
                node_type: "test::A".to_string(),
                rate_hz: None,
                priority: 0,
                background: false,
            },
            NodeConfig {
                id: "b".to_string(),
                node_type: "test::B".to_string(),
                rate_hz: None,
                priority: 0,
                background: false,
            },
        ];

        let result = registry.validate_config(&nodes);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("test::B"));
    }
}
