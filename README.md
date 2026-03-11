<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-orange?style=flat-square&logo=rust" alt="Rust" />
  <img src="https://img.shields.io/badge/status-Active%20Development-brightgreen?style=flat-square" alt="Status" />
  <img src="https://img.shields.io/badge/license-Apache--2.0-blue?style=flat-square" alt="License" />
  <img src="https://img.shields.io/github/stars/routinecrew/ROX?style=flat-square" alt="Stars" />
</p>

<h1 align="center">ROX</h1>
<h3 align="center">Intelligent Nerve System for Robotics</h3>

<p align="center">
  <b>zenoh-grade networking</b> + <b>copper-grade determinism</b> + <b>AI-native intelligence</b><br/>
  The robotics middleware that thinks before it acts.
</p>

---

## Why ROX?

Every robotics middleware forces you to choose: **fast networking** (zenoh), **deterministic execution** (copper-rs), or **safe IPC** (iceoryx2). None of them offer intelligence.

ROX combines all three — and adds an AI Agent layer that **predicts congestion before it happens**, **heals failures before they cascade**, and **validates every actuator command before it executes**.

```
            ┌─────────────────────────────────────────────────┐
            │                 ROX Runtime                      │
            │                                                  │
 [Sensor] ──▶ Publisher ──▶ TransportSelector ──▶ Subscriber ──▶ [Actuator]
            │                    │                     │       │
            │               hot path (μs)         Guard (μs)   │
            └────────────────────┼─────────────────────┼───────┘
                                 │                     │
                            metrics               validated
                                 │                     │
            ┌────────────────────▼─────────────────────▼───────┐
            │              AI Agent Runtime                     │
            │  QoS Prediction | Self-healing | Anomaly | Guard │
            └──────────────────────────────────────────────────┘
```

### What makes ROX different

| Feature | zenoh | copper-rs | iceoryx2 | **ROX** |
|---------|-------|-----------|----------|---------|
| Multi-transport (SHM/TCP/UDP) | Partial | No | SHM only | **SHM + TCP + UDP + Serial** |
| Deterministic execution | No | Yes | No | **Yes (TaskGraph scheduler)** |
| Lock-free SHM IPC | No | No | Yes | **Yes (iceoryx2 backend)** |
| AI-driven QoS prediction | No | No | No | **Yes** |
| Self-healing failover | No | No | No | **Yes** |
| Anomaly detection | No | No | No | **Yes** |
| Command safety validation | No | No | No | **Yes (Guard process)** |
| Tamper-proof audit log | No | No | No | **Yes (blake3 hash chain)** |
| Bit-exact replay | No | Yes | No | **Yes** |
| Multi-robot namespace | Yes | No | No | **Yes** |

---

## Architecture

ROX is built as a **14-crate Rust workspace**, designed for modularity and parallel development:

```
rox (unified entry crate with feature flags)
  └─ Engine
       ├─ rox-protocol           Wire protocol (KeyExpr, RoxMessage, wire encoding)
       ├─ rox-codec              Serialization (bincode default, Arrow optional)
       ├─ rox-buffer             Zero-copy buffers (ZBuf, MemoryPool, SHM)
       ├─ rox-transport          Multi-transport (SHM/TCP/UDP/Serial + dynamic switching)
       ├─ rox-core               Core runtime (Session, Node, Topic, TaskGraph, Scheduler)
       ├─ rox-log                Deterministic structured logging
       ├─ rox-replay             Bit-exact replay engine
       ├─ rox-derive             Proc-macros (#[rox_node], #[rox_sub], #[rox_pub])
       ├─ rox-agent              AI communication control agents
       │    ├─ QoS Prediction         Predict congestion, pre-adjust priorities
       │    ├─ Self-healing            Detect failures, compute backup routes
       │    ├─ Anomaly Detection       Statistical + ML anomaly detection
       │    └─ Throttle                Frequency limiting for bandwidth savings
       ├─ rox-guard              Safety validation layer
       │    ├─ CommandValidator        Range/GeoFence checks (<10μs)
       │    ├─ Watchdog                Process liveness monitoring
       │    ├─ Fail-safe               BlockAll / EmergencyStop policies
       │    └─ AuditLogger             blake3 hash-chained audit trail
       ├─ rox-bridge             ROS 2 / Zenoh bridge
       ├─ rox-api                REST API + SSE event streaming
       └─ rox-cli                CLI tool (rox new / run / monitor / replay)
```

---

## Quick Start

```bash
# Clone
git clone https://github.com/routinecrew/ROX.git
cd ROX

# Build
cargo build --workspace

# Run tests
cargo test --workspace

# Run with default config
cargo run -- --config config/rox.yml
```

### Define a Node

```rust
use rox::prelude::*;

#[rox_node]
struct LidarProcessor {
    #[rox_sub("lidar/raw")]
    input: PointCloud,

    #[rox_pub("lidar/filtered")]
    output: PointCloud,

    #[rox_param(default = 0.1)]
    voxel_size: f32,
}

#[async_trait]
impl RoxNode for LidarProcessor {
    fn name(&self) -> &str { "lidar_processor" }

    async fn tick(&mut self, ctx: &mut NodeContext) -> Result<()> {
        if let Some(cloud) = ctx.subscribers["lidar/raw"].try_recv()? {
            let filtered = voxel_filter(&cloud, self.voxel_size);
            ctx.publishers["lidar/filtered"].publish(filtered).await?;
        }
        Ok(())
    }

    async fn init(&mut self, _ctx: &mut NodeContext) -> Result<()> { Ok(()) }
    async fn shutdown(&mut self, _ctx: &mut NodeContext) -> Result<()> { Ok(()) }
}
```

### Configure Your Robot

```yaml
# config/rox.yml
transport:
  shm:
    enabled: true
    pool_size_mb: 256
  tcp:
    bind: "0.0.0.0:7447"

nodes:
  - id: lidar_driver
    type: "my_robot::LidarDriver"
    rate_hz: 10.0
    priority: 0

  - id: path_planner
    type: "my_robot::PathPlanner"
    rate_hz: 5.0
    priority: 2

  - id: motor_controller
    type: "my_robot::MotorController"
    rate_hz: 100.0
    priority: 0

connections:
  - from: "lidar_driver/scan"
    to: "path_planner/pointcloud"
  - from: "path_planner/velocity_cmd"
    to: "motor_controller/command"
    qos:
      reliability: reliable
      deadline_us: 10000

# AI Agent — operates OUTSIDE the hot path
agent:
  enabled: true
  start_mode: observer    # observer → suggestion → autonomous
  qos_prediction: true
  anomaly_detection: true
  healing: true

# Safety Guard — separate process
guard:
  enabled: true
  boundaries:
    - node: "motor_controller"
      max_velocity: 2.0
      max_acceleration: 5.0
      geofence: { min_x: -10, max_x: 10, min_y: -10, max_y: 10 }
```

---

## Key Concepts

### Multi-Robot Namespacing

ROX enforces structured topic namespaces to prevent collisions in multi-robot deployments:

```rust
// Per-robot topics
let key = KeyExpr::new("robot-01", "lidar", "scan");
// → "robot-01/lidar/scan"

// Global shared topics
let key = KeyExpr::global("map/occupancy");
// → "_global/map/occupancy"

// Wildcard matching
let pattern = "robot-01/*/scan";  // matches all scan topics from robot-01
```

### Agent Cold Start Protocol

ROX Agents don't make decisions blindly. They follow a careful progression:

| Mode | Behavior | Transition |
|------|----------|------------|
| **Observer** | Collect metrics only. No policy changes. | 24h + 100K data points + 80% topic coverage |
| **Suggestion** | Propose changes. Operator approval required. | Manual approval |
| **Autonomous** | Auto-apply + auto-rollback on degradation. | — |

Auto-rollback triggers if any metric degrades within 5 seconds:
- Latency increase > 20%
- Packet loss doubles
- Jitter variance increase > 50%

### Guard Safety Validation

Every actuator command passes through the Guard process — a separate, isolated process that validates commands in **<10 microseconds**:

```
Command → Schema Check → Range Clamp → GeoFence → Audit Log → Actuator
                                                        │
                                              blake3 hash chain
                                           (tamper-proof evidence)
```

If Guard fails (3 restart attempts), the system enters **EmergencyStop** — all commands blocked.

### Transport Auto-Switching

ROX dynamically selects the optimal transport based on network conditions:

```
Same machine  →  SHM (iceoryx2, ~1μs)
Cross machine →  TCP/TLS
Low-latency   →  UDP
Embedded MCU  →  Serial (UART/SPI)
```

Switching uses a 4-phase protocol to prevent message loss:
**Drain → Buffer → Switch → Replay**

---

## Deterministic Execution

ROX guarantees bit-exact replay for debugging and certification:

```rust
// Record
let logger = RoxLogger::new("session.rlog")?;
// ... run your robot ...

// Replay — same inputs produce same outputs
let engine = ReplayEngine::open("session.rlog")?;
engine.replay().await?;  // Uses recorded timestamps, not wall clock
```

Agent decisions are **not re-executed** during replay — the recorded policy values are injected at the exact cycle they were originally applied.

---

## Monitoring

### REST API (port 9090)

```bash
# Health check
curl http://localhost:9090/v1/health

# List active nodes
curl http://localhost:9090/v1/nodes

# List topics with QoS info
curl http://localhost:9090/v1/topics

# Stream real-time agent events (SSE)
curl http://localhost:9090/v1/events/stream

# View audit log
curl http://localhost:9090/v1/audit
```

### CLI

```bash
# Create a new ROX project
rox new my-robot

# Run with config
rox run --config config/rox.yml

# Monitor in terminal
rox monitor

# Replay a session log
rox replay --log-file session.rlog
```

---

## Roadmap

| Phase | Timeline | Deliverables |
|-------|----------|-------------|
| **1A** | Month 1-2 | Core Pub/Sub, SHM (iceoryx2), TCP, KeyExpr |
| **1B** | Month 3-4 | TaskGraph, Scheduler, Deterministic Log, Replay, `#[rox_node]` |
| **2** | Month 5-7 | Agent Level 0 (rule-based), Guard (isolated process), REST API |
| **3** | Month 8-9 | Agent Level 1 (self-learning), Auto-rollback, Healing |
| **4** | Month 10-11 | SHM optimization decision, Arrow codec |
| **5** | Month 12-14 | Multi-machine networking, QUIC, ROS 2 bridge |
| **6+** | Month 15+ | Agent Level 2 (pre-trained ML), SIL certification roadmap |

---

## Performance Targets

| Metric | Target | Reference |
|--------|--------|-----------|
| SHM Pub/Sub latency | < 5 μs | iceoryx2: ~100ns raw |
| TCP throughput (localhost) | > 100K msg/sec | zenoh: ~150K |
| Guard validation | < 10 μs | Inline, branch-predicted |
| Wire encoding | > 1M msg/sec | bincode + nom |
| Log write throughput | > 1M entries/sec | mmap + bincode |
| Agent cycle | 100ms | Non-blocking, low priority |
| Healing failover | < 50ms | petgraph shortest path |

---

## Design Principles

1. **Communication at zenoh-grade, execution at copper-grade, intelligence is ours alone**
   — No competitor offers infrastructure-level AI for robotics communication.

2. **Agent never touches the hot path**
   — AI operates on metadata only. Your real-time guarantees are preserved.

3. **Incremental adoption**
   — Start with `rox = "0.1"`, enable features as needed: `agent`, `guard`, `bridge-ros2`, `shm`.

4. **Two-layer determinism**
   — Hard layer (TaskGraph): bit-exact reproducibility. Soft layer (Agent): recorded decisions replayed.

5. **Safety is QM-grade first, SIL-certified later**
   — Honest positioning. Guard is a safety advisory tool, not a certified safety system (yet).

---

## Project Structure

```
ROX/
├── Cargo.toml                    Workspace root
├── rox/                          Unified entry crate (feature flags)
├── crates/
│   ├── rox-protocol/             Wire protocol
│   ├── rox-codec/                Serialization
│   ├── rox-buffer/               Zero-copy buffers
│   ├── rox-transport/            Multi-transport
│   ├── rox-core/                 Core runtime
│   ├── rox-log/                  Deterministic logging
│   ├── rox-replay/               Bit-exact replay
│   ├── rox-derive/               Proc-macros
│   ├── rox-agent/                AI agents
│   ├── rox-guard/                Safety validation
│   ├── rox-bridge/               ROS 2 / Zenoh bridge
│   ├── rox-api/                  REST API + SSE
│   └── rox-cli/                  CLI tool
├── contracts/
│   ├── shared_types.rs           Shared type definitions (source of truth)
│   └── mock.rs                   Mock implementations for independent dev
├── config/
│   └── rox.yml                   Default configuration
├── skills/                       Agent development instructions
└── run-agents.sh                 Multi-agent parallel development launcher
```

---

## Contributing

ROX uses a **5-agent parallel development** model. Each agent specializes in a domain:

| Agent | Crates | Domain |
|-------|--------|--------|
| A | protocol, codec, buffer | Data formats & serialization |
| B | core, derive | Runtime engine |
| C | transport, log, replay | Communication & logging |
| D | agent, guard | AI intelligence & safety |
| E | api, cli, bridge | User interfaces & integration |

```bash
# Run a specific agent
./run-agents.sh a    # Protocol specialist
./run-agents.sh b    # Core runtime specialist
./run-agents.sh d    # AI/Safety specialist
```

See [PARALLEL_DEV_GUIDE.md](PARALLEL_DEV_GUIDE.md) for the full coordination protocol.

---

## Inspired By

ROX stands on the shoulders of giants:

- **[zenoh](https://github.com/eclipse-zenoh/zenoh)** — Multi-transport protocol architecture, KeyExpr design
- **[copper-rs](https://github.com/copper-project/copper-rs)** — Deterministic TaskGraph execution, CopperList, replay
- **[iceoryx2](https://github.com/eclipse-iceoryx/iceoryx2)** — Lock-free SHM IPC, Service discovery
- **[dora-rs](https://github.com/dora-rs/dora)** — Apache Arrow messages, dataflow paradigm
- **[HORUS](https://github.com/softmata/horus)** — DX-first design, `node!` macro, Hub auto-switching

---

<p align="center">
  <b>ROX</b> — Because your robot deserves a nervous system that thinks.<br/><br/>
  <a href="https://github.com/routinecrew/ROX">GitHub</a> ·
  <a href="https://github.com/routinecrew/ROX/blob/main/Rox_System_Design.md">Design Document</a> ·
  <a href="https://github.com/routinecrew/ROX/issues">Issues</a>
</p>
