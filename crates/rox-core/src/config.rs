//! Configuration loading and hot-reload.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tokio::sync::watch;

use rox_protocol::{ConnectionConfig, NodeConfig};

/// Top-level Rox configuration.
#[derive(Clone, Debug, Deserialize)]
pub struct RoxConfig {
    pub transport: TransportConfig,
    #[serde(default)]
    pub discovery: DiscoveryConfig,
    #[serde(default)]
    pub nodes: Vec<NodeConfig>,
    #[serde(default)]
    pub connections: Vec<ConnectionConfig>,
    #[serde(default)]
    pub agent: Option<AgentConfig>,
    #[serde(default)]
    pub guard: Option<GuardConfig>,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub api: Option<ApiConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TransportConfig {
    #[serde(default)]
    pub shm: ShmConfig,
    #[serde(default)]
    pub tcp: TcpConfig,
    #[serde(default)]
    pub udp: Option<UdpConfig>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ShmConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_pool_size")]
    pub pool_size_mb: usize,
}

fn default_pool_size() -> usize {
    256
}

#[derive(Clone, Debug, Deserialize)]
pub struct TcpConfig {
    #[serde(default = "default_tcp_bind")]
    pub bind: String,
    #[serde(default)]
    pub tls: bool,
}

impl Default for TcpConfig {
    fn default() -> Self {
        Self {
            bind: default_tcp_bind(),
            tls: false,
        }
    }
}

fn default_tcp_bind() -> String {
    "0.0.0.0:7447".to_string()
}

#[derive(Clone, Debug, Deserialize)]
pub struct UdpConfig {
    pub bind: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct DiscoveryConfig {
    #[serde(default = "default_discovery_mode")]
    pub mode: String,
    #[serde(default)]
    pub peers: Vec<String>,
}

fn default_discovery_mode() -> String {
    "static".to_string()
}

#[derive(Clone, Debug, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub enabled: bool,
    pub start_mode: Option<AgentStartModeConfig>,
    pub qos_prediction: Option<QosPredictionConfig>,
    pub anomaly_detection: Option<AnomalyDetectionConfig>,
    pub healing: Option<HealingConfig>,
    pub throttling: Option<ThrottlingConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AgentStartModeConfig {
    #[serde(rename = "type", default = "default_agent_mode")]
    pub mode_type: String,
    #[serde(default = "default_observation_hours")]
    pub min_observation_hours: u32,
    #[serde(default = "default_data_points")]
    pub min_data_points: u64,
    #[serde(default = "default_topic_coverage")]
    pub min_topic_coverage: f32,
    #[serde(default = "default_anomaly_ratio")]
    pub max_anomaly_ratio: f32,
}

fn default_agent_mode() -> String {
    "observer".to_string()
}
fn default_observation_hours() -> u32 {
    24
}
fn default_data_points() -> u64 {
    100_000
}
fn default_topic_coverage() -> f32 {
    0.8
}
fn default_anomaly_ratio() -> f32 {
    0.05
}

#[derive(Clone, Debug, Deserialize)]
pub struct QosPredictionConfig {
    pub enabled: bool,
    #[serde(default)]
    pub level: u8,
    #[serde(default = "default_agent_interval")]
    pub interval_ms: u64,
}

fn default_agent_interval() -> u64 {
    100
}

#[derive(Clone, Debug, Deserialize)]
pub struct AnomalyDetectionConfig {
    pub enabled: bool,
    #[serde(default = "default_jitter_threshold")]
    pub jitter_threshold_us: u64,
    #[serde(default = "default_packet_loss_threshold")]
    pub packet_loss_threshold: f32,
}

fn default_jitter_threshold() -> u64 {
    500
}
fn default_packet_loss_threshold() -> f32 {
    0.01
}

#[derive(Clone, Debug, Deserialize)]
pub struct HealingConfig {
    pub enabled: bool,
    #[serde(default = "default_failover_timeout")]
    pub failover_timeout_ms: u64,
}

fn default_failover_timeout() -> u64 {
    50
}

#[derive(Clone, Debug, Deserialize)]
pub struct ThrottlingConfig {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GuardConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_safety_level")]
    pub safety_level: String,
    #[serde(default)]
    pub process_isolation: bool,
    pub supervisor: Option<SupervisorConfig>,
    pub watchdog: Option<WatchdogConfig>,
    pub failsafe: Option<FailsafeConfig>,
    #[serde(default)]
    pub boundaries: Vec<BoundaryConfig>,
    pub audit_log: Option<AuditLogConfig>,
}

fn default_safety_level() -> String {
    "qm".to_string()
}

#[derive(Clone, Debug, Deserialize)]
pub struct SupervisorConfig {
    #[serde(default = "default_max_restart")]
    pub max_restart_attempts: u32,
    #[serde(default = "default_restart_cooldown")]
    pub restart_cooldown_sec: u64,
    #[serde(default = "default_interim_policy")]
    pub interim_policy: String,
}

fn default_max_restart() -> u32 {
    3
}
fn default_restart_cooldown() -> u64 {
    5
}
fn default_interim_policy() -> String {
    "block_all_commands".to_string()
}

#[derive(Clone, Debug, Deserialize)]
pub struct WatchdogConfig {
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_ms: u64,
    #[serde(default = "default_watchdog_timeout")]
    pub timeout_ms: u64,
}

fn default_heartbeat_interval() -> u64 {
    100
}
fn default_watchdog_timeout() -> u64 {
    500
}

#[derive(Clone, Debug, Deserialize)]
pub struct FailsafeConfig {
    #[serde(default = "default_on_guard_failure")]
    pub on_guard_failure: String,
    #[serde(default = "default_on_validation_timeout")]
    pub on_validation_timeout: String,
}

fn default_on_guard_failure() -> String {
    "block_all_commands".to_string()
}
fn default_on_validation_timeout() -> String {
    "reject".to_string()
}

#[derive(Clone, Debug, Deserialize)]
pub struct BoundaryConfig {
    pub node: String,
    pub max_velocity: Option<f64>,
    pub max_acceleration: Option<f64>,
    pub geofence: Option<GeoFenceConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GeoFenceConfig {
    pub min_x: f64,
    pub max_x: f64,
    pub min_y: f64,
    pub max_y: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AuditLogConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_audit_path")]
    pub path: String,
    #[serde(default = "default_rotation_mb")]
    pub rotation_mb: usize,
}

fn default_audit_path() -> String {
    "./logs/audit/".to_string()
}
fn default_rotation_mb() -> usize {
    100
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default)]
    pub deterministic: bool,
    #[serde(default = "default_log_dir")]
    pub log_dir: String,
    #[serde(default = "default_rotation_mb")]
    pub rotation_mb: usize,
}

fn default_log_level() -> String {
    "info".to_string()
}
fn default_log_dir() -> String {
    "./logs/".to_string()
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_api_bind")]
    pub bind: String,
    #[serde(default)]
    pub cors_origins: Vec<String>,
}

fn default_api_bind() -> String {
    "0.0.0.0:9090".to_string()
}

impl RoxConfig {
    /// Load configuration from a YAML file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config: {}", path.display()))?;
        let config: Self = serde_yaml::from_str(&content)
            .with_context(|| format!("failed to parse config: {}", path.display()))?;
        Ok(config)
    }

    /// Load from a YAML string.
    pub fn from_str(yaml: &str) -> Result<Self> {
        serde_yaml::from_str(yaml).context("failed to parse config YAML")
    }
}

/// Watches a config file for changes and broadcasts updates.
pub struct ConfigWatcher {
    path: PathBuf,
    tx: watch::Sender<RoxConfig>,
    rx: watch::Receiver<RoxConfig>,
}

impl ConfigWatcher {
    /// Create a new ConfigWatcher for the given path.
    pub fn new(path: PathBuf) -> Result<Self> {
        let config = RoxConfig::from_file(&path)?;
        let (tx, rx) = watch::channel(config);
        Ok(Self { path, tx, rx })
    }

    /// Get a receiver that will be notified on config changes.
    pub fn subscribe(&self) -> watch::Receiver<RoxConfig> {
        self.rx.clone()
    }

    /// Start watching for file changes. Runs until cancelled.
    pub async fn watch(&self) -> Result<()> {
        use notify::{Event, EventKind, RecursiveMode, Watcher};
        use std::sync::mpsc;

        let (notify_tx, notify_rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher =
            notify::recommended_watcher(notify_tx).context("failed to create file watcher")?;

        watcher
            .watch(&self.path, RecursiveMode::NonRecursive)
            .with_context(|| format!("failed to watch: {}", self.path.display()))?;

        let path = self.path.clone();
        let config_tx = self.tx.clone();

        tokio::task::spawn_blocking(move || {
            for event in notify_rx {
                if let Ok(event) = event {
                    if matches!(event.kind, EventKind::Modify(_)) {
                        match RoxConfig::from_file(&path) {
                            Ok(new_config) => {
                                tracing::info!("config reloaded: {}", path.display());
                                let _ = config_tx.send(new_config);
                            }
                            Err(e) => {
                                tracing::warn!("config reload failed: {e}");
                            }
                        }
                    }
                }
            }
        });

        // Keep the watcher alive
        std::mem::forget(watcher);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_str() {
        let yaml = r#"
transport:
  shm:
    enabled: false
    pool_size_mb: 128
  tcp:
    bind: "0.0.0.0:7447"
    tls: false

nodes:
  - id: test_node
    node_type: "test::Node"
    rate_hz: 10.0
    priority: 0
    background: false
"#;

        let config = RoxConfig::from_str(yaml).unwrap();
        assert!(!config.transport.shm.enabled);
        assert_eq!(config.transport.shm.pool_size_mb, 128);
        assert_eq!(config.nodes.len(), 1);
        assert_eq!(config.nodes[0].id, "test_node");
    }
}
