# ROX 100% Completion Design

## Current State: 55-60% (Early Alpha)

**동작하는 것:** 각 크레이트가 독립적으로 빌드/테스트 통과 (75개 테스트, 0 실패)
**동작하지 않는 것:** 크레이트 간 연결. End-to-End 시나리오가 없다.

### Gap Map — 끊어진 연결 7개

```
[1] RoxNode 구현체를 Engine에 등록/실행하는 메커니즘 없음
    - engine.tick()이 trace log만 찍고, 실제 RoxNode.tick()을 호출하지 않음
    - NodeRegistry가 없어서 사용자 노드를 런타임에 등록할 수 없음

[2] Transport <-> TopicManager 연결 없음
    - TCP/UDP는 raw bytes만 주고받음
    - TopicManager는 in-process broadcast channel만 사용
    - 결과: 크로스 머신 메시지 전달 불가

[3] Transport -> Agent 메트릭 파이프라인 없음
    - Transport에 metrics_tx 필드는 있지만, 실제로 메트릭을 보내지 않음
    - Agent가 받을 데이터가 없어서 모든 에이전트가 빈 상태로 돌아감

[4] Guard가 메시지 경로에 삽입되지 않음
    - CommandValidator, AuditLogger 각각 동작하지만
    - 실제 메시지 흐름에 Guard가 끼어들지 않음

[5] Scheduler가 실제 노드를 실행하지 않음
    - run_cycle의 execute_node 콜백이 현재 트레이스 로그만
    - rate_hz 기반 주기 실행 미구현

[6] 4-phase Transport Switch가 골격만
    - switch_transport()에서 Phase 1(Drain)/2(Buffer)/4(Replay)가 주석

[7] SHM Transport 미구현
    - shm.rs 파일 존재하지만 iceoryx2 백엔드 없음
```

---

## Phase 설계 (의존성 순서)

### Phase 1: Node Registry + Engine Integration
**목표:** 사용자 정의 RoxNode가 실제로 실행되는 것
**의존성:** 없음 (첫 번째)
**영향 크레이트:** `rox-core`

#### 1.1 NodeRegistry 추가 (`rox-core/src/registry.rs`)

```rust
/// 노드 팩토리 — 타입 이름으로 노드 인스턴스를 생성
pub type NodeFactory = Box<dyn Fn() -> Box<dyn RoxNode> + Send + Sync>;

pub struct NodeRegistry {
    factories: HashMap<String, NodeFactory>,
}

impl NodeRegistry {
    pub fn new() -> Self { ... }

    /// 노드 타입 등록
    /// registry.register("my_robot::LidarDriver", || Box::new(LidarDriver::new()));
    pub fn register(&mut self, type_name: &str, factory: NodeFactory) { ... }

    /// 설정에서 노드 인스턴스 생성
    pub fn create(&self, node_config: &NodeConfig) -> Result<Box<dyn RoxNode>> { ... }
}
```

#### 1.2 RoxEngine 수정

```rust
pub struct RoxEngine {
    config: RoxConfig,
    session: Session,
    task_graph: TaskGraph,
    scheduler: Scheduler,
    policy_tx: mpsc::Sender<VersionedPolicyUpdate>,
    // 추가
    node_instances: HashMap<String, Box<dyn RoxNode>>,
    node_contexts: HashMap<String, NodeContext>,
}

impl RoxEngine {
    /// 노드 레지스트리로부터 모든 노드를 인스턴스화하고 init() 호출
    pub async fn init_nodes(&mut self, registry: &NodeRegistry) -> Result<()> {
        for node_config in &self.config.nodes {
            let mut node = registry.create(node_config)?;
            let mut ctx = self.build_node_context(node_config).await?;
            node.init(&mut ctx).await?;
            self.node_instances.insert(node_config.id.clone(), node);
            self.node_contexts.insert(node_config.id.clone(), ctx);
        }
        Ok(())
    }

    /// tick() 수정 — 실제 노드 실행
    pub async fn tick(&mut self) -> Result<()> {
        self.scheduler.run_cycle(|node_id, _cycle| {
            if let (Some(node), Some(ctx)) = (
                self.node_instances.get_mut(node_id),
                self.node_contexts.get_mut(node_id),
            ) {
                // rate_hz 체크 후 실행
                node.tick(ctx)?;
            }
            Ok(())
        }).await?;
        Ok(())
    }
}
```

핵심: `run_cycle`의 콜백이 `FnMut(&str, u64) -> Result<()>`인데, 이 안에서 `self`의 가변 참조가 필요. 현재 시그니처로는 불가능하므로 **Scheduler가 execution_order만 반환하고, Engine이 직접 루프를 돌도록** 변경:

```rust
// Scheduler 변경
impl Scheduler {
    /// 이번 사이클에 실행할 노드 목록과 적용할 정책 반환
    pub fn prepare_cycle(&mut self) -> (u64, &[String], Vec<VersionedPolicyUpdate>) {
        // policy drain + pending 처리
        // execution_order 반환
    }

    /// 사이클 완료 기록
    pub fn complete_cycle(&mut self, metrics: CycleMetrics) { ... }
}

// Engine에서 직접 루프
pub async fn tick(&mut self) -> Result<()> {
    let (cycle, order, policies) = self.scheduler.prepare_cycle();

    // 정책 적용
    for policy in policies {
        self.apply_policy(&policy).await?;
    }

    // 노드 실행
    for node_id in order {
        if let (Some(node), Some(ctx)) = (
            self.node_instances.get_mut(node_id),
            self.node_contexts.get_mut(node_id),
        ) {
            // rate_hz 체크
            if self.should_execute(node_id, cycle) {
                node.tick(ctx).await?;
            }
        }
    }

    self.scheduler.complete_cycle(CycleMetrics { ... });
    Ok(())
}
```

#### 1.3 Rate-based 실행

```rust
struct NodeExecutionState {
    last_executed_cycle: u64,
    cycle_interval: u64,  // rate_hz -> 사이클 간격 변환
}

impl RoxEngine {
    fn should_execute(&self, node_id: &str, cycle: u64) -> bool {
        if let Some(state) = self.execution_states.get(node_id) {
            cycle - state.last_executed_cycle >= state.cycle_interval
        } else {
            true
        }
    }
}
```

#### 1.4 검증 기준
- `cargo test -p rox-core` — 기존 테스트 유지
- 새 통합 테스트: 2개 노드(Counter -> Logger) 정의, Engine에 등록, 10 사이클 실행, Logger가 Counter의 메시지를 수신했는지 확인

---

### Phase 2: Transport <-> TopicManager 통합
**목표:** 크로스 머신 메시지 전달
**의존성:** Phase 1
**영향 크레이트:** `rox-core`, `rox-transport`

#### 2.1 TransportManager 추가 (`rox-transport/src/manager.rs`)

```rust
/// Transport Manager — 여러 Transport를 관리하고 TopicManager와 연결
pub struct TransportManager {
    selector: TransportSelector,
    tcp: Option<TcpTransport>,
    udp: Option<UdpTransport>,
    #[cfg(feature = "shm")]
    shm: Option<ShmTransport>,
    /// Transport에서 수신한 메시지를 TopicManager에 주입
    topic_injector: Arc<dyn TopicRegistry>,
    /// TopicManager에서 발행된 메시지를 Transport로 전달
    outbound_rx: mpsc::Receiver<(NodeId, Arc<RoxMessage>)>,
    /// 메트릭 채널 (Agent로 전달)
    metrics_tx: Option<mpsc::Sender<TransportMetrics>>,
}
```

#### 2.2 메시지 흐름 설계

```
[Local Node] -> TopicManager.publish() -> broadcast channel
                                            |
                              ┌─────────────┤
                              v             v
                    [Local Subscriber]  [TransportManager.outbound_loop]
                                            |
                                      WireEncoder.encode()
                                            |
                                      TransportSelector.select(target)
                                            |
                              ┌─────────────┼─────────────┐
                              v             v             v
                           TCP.send()    UDP.send()   SHM.write()
                              |             |             |
                        [Remote Machine]    |             |
                              |             |             |
                        TCP.recv()       UDP.recv()   SHM.read()
                              |             |             |
                              └─────────────┼─────────────┘
                                            |
                                      WireDecoder.decode()
                                            |
                                      TopicManager.inject()
                                            |
                                      broadcast channel
                                            |
                                    [Remote Subscriber]
```

#### 2.3 TopicManager 확장

```rust
impl TopicManager {
    /// Transport에서 수신한 메시지를 로컬 구독자에게 주입
    /// (publish와 달리 발행자 등록 없이 직접 채널에 전송)
    pub async fn inject(&self, msg: Arc<RoxMessage>) -> Result<()> {
        let topics = self.topics.read().await;
        if let Some(topic) = topics.get(msg.header.key.as_str()) {
            let _ = topic.sender.send(msg);
        }
        Ok(())
    }

    /// 로컬 발행 메시지를 Transport로 전달하기 위한 탭
    /// (broadcast subscriber가 outbound loop에 연결)
    pub async fn tap_outbound(&self, key: &KeyExpr) -> Result<MessageReceiver> {
        self.subscribe(key).await
    }
}
```

#### 2.4 Peer Discovery

```rust
/// 정적 피어 목록 기반 (Phase 1 scope)
pub struct StaticDiscovery {
    peers: Vec<SocketAddr>,
}

/// 나중에 mDNS/gossip 기반으로 확장 가능
#[async_trait]
pub trait PeerDiscovery: Send + Sync {
    async fn discover(&self) -> Vec<PeerInfo>;
    async fn announce(&self, info: &PeerInfo) -> Result<()>;
}
```

#### 2.5 Transport 메시지 수신 루프

```rust
impl TransportManager {
    /// TCP 리스너에서 메시지를 받아 TopicManager에 주입
    async fn inbound_loop(&self, mut handle: TcpListenerHandle) {
        loop {
            if let Ok(conn) = handle.accept().await {
                let topic_injector = Arc::clone(&self.topic_injector);
                let metrics_tx = self.metrics_tx.clone();

                tokio::spawn(async move {
                    loop {
                        match conn.recv_bytes().await {
                            Ok(data) => {
                                let start = Instant::now();
                                match WireDecoder::decode(data.clone().into()) {
                                    Ok(msg) => {
                                        let _ = topic_injector.inject(Arc::new(msg)).await;

                                        // [Gap 3 해결] 메트릭 전송
                                        if let Some(tx) = &metrics_tx {
                                            let _ = tx.send(TransportMetrics {
                                                timestamp: now_ns(),
                                                topic: msg.header.key.clone(),
                                                latency_us: start.elapsed().as_micros() as u64,
                                                jitter_us: 0,  // 이전 메시지와 비교하여 계산
                                                throughput_bps: data.len() as u64 * 8,
                                                packet_loss_ratio: 0.0,
                                                queue_depth: 0,
                                                transport_kind: TransportKind::Tcp,
                                            }).await;
                                        }
                                    }
                                    Err(e) => tracing::warn!("decode error: {e}"),
                                }
                            }
                            Err(_) => break,
                        }
                    }
                });
            }
        }
    }
}
```

#### 2.6 검증 기준
- 통합 테스트: 같은 프로세스에서 2개 Transport (server + client), 메시지를 TCP로 송수신하고 TopicManager 구독자가 수신 확인
- 메트릭 채널에 데이터가 실제로 도착하는지 확인

---

### Phase 3: Agent 메트릭 파이프라인 연결
**목표:** Transport 메트릭이 Agent까지 흘러서 실제 판단이 일어남
**의존성:** Phase 2
**영향 크레이트:** `rox-transport`, `rox-agent`, `rox-core`

#### 3.1 메트릭 수집기 (`rox-transport/src/metrics_collector.rs`)

```rust
/// Transport별 메트릭을 수집하여 Agent에 전달
pub struct MetricsCollector {
    tx: mpsc::Sender<TransportMetrics>,
    /// 이전 메시지 타임스탬프 (jitter 계산용)
    prev_timestamps: HashMap<String, u64>,
    /// 윈도우 기반 throughput 계산
    throughput_windows: HashMap<String, VecDeque<(u64, u64)>>,  // (timestamp, bytes)
}

impl MetricsCollector {
    /// 메시지 수신 시 호출
    pub fn record_recv(&mut self, topic: &KeyExpr, bytes: u64, transport: TransportKind) {
        let now = now_ns();
        let prev = self.prev_timestamps.insert(topic.as_str().to_string(), now);

        let jitter = prev.map(|p| {
            let interval = now - p;
            // 기대 간격 대비 편차
            interval.abs_diff(self.expected_interval(topic))
        }).unwrap_or(0) / 1000;  // ns -> us

        let throughput = self.compute_throughput(topic, now, bytes);

        let _ = self.tx.try_send(TransportMetrics {
            timestamp: now,
            topic: topic.clone(),
            latency_us: 0,  // RTT 측정은 별도 ping/pong
            jitter_us: jitter,
            throughput_bps: throughput,
            packet_loss_ratio: 0.0,  // sequence gap 기반
            queue_depth: 0,
            transport_kind: transport,
        });
    }

    /// sequence 기반 패킷 손실률 계산
    pub fn record_sequence(&mut self, topic: &KeyExpr, seq: u64) {
        // expected_seq vs actual_seq gap으로 loss ratio 계산
    }
}
```

#### 3.2 Engine에서 Agent 연결

```rust
// main.rs의 Run 커맨드 수정
let (metrics_tx, metrics_rx) = mpsc::channel(4096);

// TransportManager에 metrics_tx 연결
let transport_mgr = TransportManager::new(topic_manager.clone())
    .with_metrics(metrics_tx.clone());

// Agent에 metrics_rx 연결 (기존 코드와 동일, 이미 구현됨)
let mut agent_runtime = AgentRuntime::new(metrics_rx, policy_tx);
```

#### 3.3 Agent Auto-Transition (Observer -> Suggestion -> Autonomous)

현재 `AgentRuntime.run()`에서 모드 전환 로직 추가:

```rust
// run() 루프 끝에 추가
fn check_mode_transition(&mut self) {
    if let AgentStartMode::Observer { duration_hours } = &self.mode {
        let stats = &self.observation_stats;
        if stats.observation_hours >= *duration_hours
            && stats.total_data_points >= 100_000
            && stats.topic_coverage >= 0.8
            && stats.anomaly_ratio < 0.05
        {
            self.mode = AgentStartMode::Suggestion;
            info!("agent transitioned to Suggestion mode");
        }
    }
}
```

`topic_coverage` 계산:

```rust
fn compute_topic_coverage(&self) -> f32 {
    // 현재 관측된 토픽 수 / 전체 등록된 토픽 수
    let observed = self.qos_agent.observed_topics();
    let total = self.known_topics;
    if total == 0 { return 0.0; }
    observed as f32 / total as f32
}
```

#### 3.4 Auto-Rollback

```rust
/// Autonomous 모드에서 정책 적용 후 5초 이내 성능 하락 시 롤백
struct PolicyRollbackGuard {
    applied_policy: VersionedPolicyUpdate,
    baseline_metrics: HashMap<String, f64>,  // topic -> avg_latency
    applied_at: Instant,
    rollback_window: Duration,  // 5초
}

impl PolicyRollbackGuard {
    fn should_rollback(&self, current_metrics: &TransportMetrics) -> bool {
        if self.applied_at.elapsed() > self.rollback_window {
            return false;  // 안정화됨
        }
        if let Some(baseline) = self.baseline_metrics.get(current_metrics.topic.as_str()) {
            let latency_increase = current_metrics.latency_us as f64 / baseline;
            latency_increase > 1.2  // 20% 증가
        } else {
            false
        }
    }
}
```

#### 3.5 검증 기준
- 통합 테스트: Transport로 메시지 100개 전송, Agent의 `observation_stats.total_data_points >= 100` 확인
- QoS Agent가 고지연 메트릭에 대해 `CongestionWarning` 이벤트 생성 확인
- Rollback 테스트: 정책 적용 후 지연 급증 시 rollback 동작 확인

---

### Phase 4: Guard 메시지 경로 삽입
**목표:** actuator 커맨드가 Guard를 거쳐 검증된 후 전달
**의존성:** Phase 1, Phase 2
**영향 크레이트:** `rox-guard`, `rox-core`

#### 4.1 Guard 삽입 지점

```
Publisher.send(cmd_msg)
    |
    v
TopicManager.publish()
    |
    v
[Guard intercept]  <-- 여기에 삽입
    |
    ├── ValidationResult::Passed -> broadcast channel
    ├── ValidationResult::Clamped -> modify payload, broadcast
    ├── ValidationResult::Rejected -> drop, log audit
    └── ValidationResult::EmergencyStop -> block all, log audit
    |
    v
broadcast channel -> Subscriber
```

#### 4.2 GuardedTopicManager

```rust
/// TopicManager를 감싸서 특정 토픽에 Guard 검증을 추가
pub struct GuardedTopicManager {
    inner: Arc<TopicManager>,
    validator: Mutex<CommandValidator>,
    audit_logger: Mutex<AuditLogger>,
    /// Guard 대상 토픽 패턴 (예: "*/motor/cmd", "*/actuator/*")
    guarded_patterns: Vec<String>,
}

#[async_trait]
impl TopicRegistry for GuardedTopicManager {
    async fn publish(&self, key: &KeyExpr, qos: QoSMetadata) -> Result<MessageSender> {
        // Guard 대상 토픽이면 intercepting sender 반환
        if self.is_guarded(key) {
            let real_sender = self.inner.publish(key, qos).await?;
            Ok(self.create_guarded_sender(key.clone(), real_sender))
        } else {
            self.inner.publish(key, qos).await
        }
    }
    // subscribe, unpublish, list_topics은 inner에 위임
}
```

#### 4.3 Guard 프로세스 격리 (선택적)

README의 설계대로 Guard를 별도 프로세스로 격리하는 옵션:

```rust
/// Guard를 별도 프로세스로 실행 (process_isolation: true)
/// IPC: Unix domain socket 또는 shared memory
pub struct GuardProcess {
    child: Option<Child>,
    ipc: UnixStream,  // 또는 SHM
    watchdog: GuardWatchdog,
}

impl GuardProcess {
    /// 커맨드 검증 요청 (IPC를 통해)
    pub async fn validate(&self, topic: &KeyExpr, payload: &[u8]) -> Result<ValidationResult> {
        // topic + payload를 IPC로 전송
        // Guard 프로세스가 검증 후 결과 반환
        // timeout 시 failsafe 정책 적용
    }

    /// Watchdog 헬스체크
    pub async fn health_check(&mut self) -> bool {
        if self.watchdog.check_timeout() {
            self.attempt_restart().await
        } else {
            true
        }
    }

    /// 3회 재시작 실패 시 EmergencyStop
    async fn attempt_restart(&mut self) -> bool { ... }
}
```

#### 4.4 검증 기준
- 테스트: velocity 5.0인 메시지 발행 -> Guard가 2.0으로 clamp -> 구독자가 2.0 수신
- Audit log 무결성: `verify_audit_log()` 통과
- EmergencyStop 테스트: Guard 프로세스 kill -> Watchdog 감지 -> 3회 재시작 -> EmergencyStop

---

### Phase 5: 4-Phase Transport Switch 완성
**목표:** 무손실 전송 계층 전환
**의존성:** Phase 2
**영향 크레이트:** `rox-transport`

#### 5.1 설계

```rust
pub struct TransportSwitcher {
    buffer: VecDeque<Arc<RoxMessage>>,
    state: SwitchState,
}

enum SwitchState {
    Idle,
    Draining { remaining: usize },
    Buffering,
    Switching,
    Replaying { remaining: usize },
}

impl TransportSwitcher {
    /// Phase 1: Drain — 기존 Transport의 in-flight 메시지 완료 대기
    pub async fn drain(&mut self, transport: &dyn Transport, timeout: Duration) -> Result<usize> {
        self.state = SwitchState::Draining { remaining: 0 };
        // 기존 Transport의 pending send 완료 대기
        // timeout 시 남은 메시지는 buffer로 이동
        let drained = transport.flush(timeout).await?;
        self.state = SwitchState::Buffering;
        Ok(drained)
    }

    /// Phase 2: Buffer — 전환 중 새 메시지를 메모리에 버퍼링
    pub fn buffer_message(&mut self, msg: Arc<RoxMessage>) {
        self.buffer.push_back(msg);
    }

    /// Phase 3: Switch — 새 Transport 활성화
    pub async fn switch(&mut self, new_transport: TransportKind) -> Result<()> {
        self.state = SwitchState::Switching;
        // selector에서 active transport 변경
        self.state = SwitchState::Replaying { remaining: self.buffer.len() };
        Ok(())
    }

    /// Phase 4: Replay — 버퍼링된 메시지를 새 Transport로 전송
    pub async fn replay(&mut self, transport: &dyn Transport) -> Result<usize> {
        let count = self.buffer.len();
        while let Some(msg) = self.buffer.pop_front() {
            transport.send(msg).await?;
        }
        self.state = SwitchState::Idle;
        Ok(count)
    }
}
```

#### 5.2 Transport trait 통합

```rust
#[async_trait]
pub trait Transport: Send + Sync {
    fn kind(&self) -> TransportKind;
    async fn send(&self, msg: Arc<RoxMessage>) -> Result<()>;
    async fn recv(&self) -> Result<Arc<RoxMessage>>;
    async fn flush(&self, timeout: Duration) -> Result<usize>;
    fn metrics(&self) -> TransportMetrics;
}
```

현재 TCP/UDP는 raw bytes만 다루는데, `RoxMessage`를 직접 다루도록 WireEncoder/Decoder를 내장:

```rust
impl Transport for TcpTransport {
    async fn send(&self, msg: Arc<RoxMessage>) -> Result<()> {
        let wire = WireEncoder::encode(&msg)?;
        self.connection.send_bytes(&wire).await
    }

    async fn recv(&self) -> Result<Arc<RoxMessage>> {
        let data = self.connection.recv_bytes().await?;
        let msg = WireDecoder::decode(data.into())?;
        Ok(Arc::new(msg))
    }
}
```

#### 5.3 검증 기준
- 테스트: TCP 전송 중 UDP로 전환, 메시지 손실 0개 확인
- switch_duration_us < 1000 (1ms 이하)
- buffer에 쌓인 메시지가 전부 replay 됨

---

### Phase 6: SHM Transport
**목표:** 같은 머신 내 sub-microsecond IPC
**의존성:** Phase 5 (Transport trait 통합)
**영향 크레이트:** `rox-transport`, `rox-buffer`

#### 6.1 SHM 백엔드 선택

iceoryx2는 Rust-native이지만 API가 아직 불안정. 현실적 접근:

**Option A: `shared_memory` crate 기반 직접 구현**
- `memmap2`(이미 의존성)로 memory-mapped file 생성
- lock-free ring buffer 구현 (SPSC)
- 장점: 의존성 최소화, 완전 제어
- 단점: lock-free 구현 난이도

**Option B: iceoryx2 연동**
- `iceoryx2` crate 사용
- Service discovery + zero-copy publish/subscribe
- 장점: 검증된 구현, 고성능
- 단점: 외부 의존성, API 변동

**권장: Option A (Phase 6A) -> Option B (Phase 6B)**

#### 6.2 Phase 6A: memmap2 기반 SHM

```rust
/// Lock-free SPSC ring buffer over shared memory
pub struct ShmRingBuffer {
    mmap: MmapMut,
    /// Header layout: [write_pos: 8B] [read_pos: 8B] [capacity: 8B]
    /// Data: [slot_0] [slot_1] ... [slot_N]
    capacity: usize,
    slot_size: usize,
}

impl ShmRingBuffer {
    pub fn create(path: &Path, capacity: usize, slot_size: usize) -> Result<Self> {
        let total = HEADER_SIZE + capacity * slot_size;
        let file = OpenOptions::new().read(true).write(true).create(true).open(path)?;
        file.set_len(total as u64)?;
        let mmap = unsafe { MmapMut::map_mut(&file)? };
        // Initialize header
        Ok(Self { mmap, capacity, slot_size })
    }

    pub fn open(path: &Path) -> Result<Self> { ... }

    /// Zero-copy write: 반환된 슬롯에 직접 쓰기
    pub fn try_write(&self) -> Option<&mut [u8]> {
        let write_pos = self.read_write_pos();
        let read_pos = self.read_read_pos();
        if (write_pos + 1) % self.capacity == read_pos {
            return None;  // Full
        }
        let offset = HEADER_SIZE + write_pos * self.slot_size;
        Some(&mut self.mmap[offset..offset + self.slot_size])
    }

    /// Zero-copy read
    pub fn try_read(&self) -> Option<&[u8]> { ... }

    /// Commit write (atomic write_pos advance)
    pub fn commit_write(&self) { ... }

    /// Commit read (atomic read_pos advance)
    pub fn commit_read(&self) { ... }
}
```

#### 6.3 ShmTransport 구현

```rust
pub struct ShmTransport {
    pool_size_mb: usize,
    ring_buffers: HashMap<String, ShmRingBuffer>,  // topic -> ring buffer
}

#[async_trait]
impl Transport for ShmTransport {
    fn kind(&self) -> TransportKind { TransportKind::SharedMemory }

    async fn send(&self, msg: Arc<RoxMessage>) -> Result<()> {
        let key = msg.header.key.as_str();
        let rb = self.ring_buffers.get(key)
            .ok_or_else(|| anyhow!("no SHM buffer for {key}"))?;

        let wire = WireEncoder::encode(&msg)?;
        let slot = rb.try_write()
            .ok_or_else(|| anyhow!("SHM buffer full for {key}"))?;

        // [len: 4B] [data]
        slot[..4].copy_from_slice(&(wire.len() as u32).to_le_bytes());
        slot[4..4 + wire.len()].copy_from_slice(&wire);
        rb.commit_write();
        Ok(())
    }

    async fn recv(&self) -> Result<Arc<RoxMessage>> {
        // poll all ring buffers (or use eventfd for notification)
        todo!()
    }
}
```

#### 6.4 검증 기준
- 벤치마크: SHM pub/sub latency < 5us (목표)
- 테스트: 2개 프로세스 간 SHM 메시지 전달
- MemoryPool과 SHM 조합 테스트

---

### Phase 7: CLI Monitor + Hot Reload
**목표:** 운영 도구 완성
**의존성:** Phase 2 (API 서버), Phase 3 (Agent 이벤트)
**영향 크레이트:** `rox-cli`, `rox-api`, `rox-core`

#### 7.1 CLI Monitor (`rox monitor`)

```rust
Commands::Monitor { endpoint } => {
    let client = reqwest::Client::new();

    // 1. 초기 상태 조회
    let nodes: Vec<NodeInfo> = client.get(format!("{endpoint}/v1/nodes"))
        .send().await?.json().await?;
    let topics: Vec<TopicInfo> = client.get(format!("{endpoint}/v1/topics"))
        .send().await?.json().await?;

    // 2. SSE 스트림 연결
    let mut stream = client.get(format!("{endpoint}/v1/events/stream"))
        .send().await?;

    // 3. TUI 렌더링 (간단한 터미널 출력)
    println!("=== ROX Monitor ({endpoint}) ===");
    println!("Nodes: {}", nodes.len());
    println!("Topics: {}", topics.len());
    println!("---");
    println!("Listening for events (Ctrl+C to stop)...\n");

    while let Some(chunk) = stream.chunk().await? {
        let text = String::from_utf8_lossy(&chunk);
        for line in text.lines() {
            if line.starts_with("data: ") {
                let data = &line["data: ".len()..];
                if let Ok(event) = serde_json::from_str::<AgentEvent>(data) {
                    print_event(&event);
                }
            }
        }
    }
}
```

#### 7.2 API 확장

현재 API에 누락된 엔드포인트:

```rust
// rox-api/src/routes.rs 추가
Router::new()
    .route("/v1/health", get(health))
    .route("/v1/topics", get(list_topics))
    .route("/v1/nodes", get(list_nodes))
    .route("/v1/events/stream", get(event_stream))
    // 추가
    .route("/v1/audit", get(audit_log))         // Guard 감사 로그
    .route("/v1/metrics", get(metrics))          // Agent 메트릭 스냅샷
    .route("/v1/graph", get(task_graph))          // TaskGraph 시각화용
    .route("/v1/agent/status", get(agent_status)) // Agent 모드/통계
    .route("/v1/config", get(config))             // 현재 설정
    .route("/v1/config", put(update_config))      // Hot-reload 트리거
```

#### 7.3 Hot-Reload 통합

현재 `ConfigWatcher`는 구현되어 있지만 Engine과 연결이 안 됨:

```rust
// main.rs의 Run 커맨드에 추가
let config_watcher = ConfigWatcher::new(config_path.to_path_buf())?;
let mut config_rx = config_watcher.subscribe();
config_watcher.watch().await?;

// main loop에 config change 처리 추가
loop {
    tokio::select! {
        _ = interval.tick() => {
            engine.tick().await?;
        }
        Ok(()) = config_rx.changed() => {
            let new_config = config_rx.borrow().clone();
            engine.apply_config_update(&new_config).await?;
        }
        _ = tokio::signal::ctrl_c() => {
            break;
        }
    }
}
```

Hot-reload 가능한 항목:
- Agent 활성화/비활성화, 임계값 변경
- Guard boundary 추가/변경
- QoS 설정 변경
- 새 노드 연결 추가 (노드 자체의 추가/제거는 재시작 필요)

#### 7.4 검증 기준
- `rox monitor` 실행 -> health/nodes/topics 출력 확인
- SSE 이벤트 수신 확인
- config 파일 수정 -> Engine에 반영 확인 (테스트)

---

### Phase 8: rox-derive 완성 + Examples
**목표:** 사용자 경험 완성
**의존성:** Phase 1
**영향 크레이트:** `rox-derive`, `examples/`

#### 8.1 rox-derive 확장

현재 `#[rox_param(default = 0.1)]`이 파싱되지만 default 값이 적용 안 됨:

```rust
// 생성 코드에 Default impl 추가
impl Default for #name {
    fn default() -> Self {
        Self {
            #(#default_fields),*
        }
    }
}
```

`#[rox_sub]`, `#[rox_pub]` 필드를 실제 `MessageReceiver`, `MessageSender`로 자동 연결:

```rust
// 자동 생성될 init() 보일러플레이트
impl RoxNode for #name {
    async fn init(&mut self, ctx: &mut NodeContext) -> Result<()> {
        // #[rox_sub("lidar/raw")]가 있으면:
        // ctx.subscribers에서 "lidar/raw" 키로 receiver를 꺼내와서 필드에 연결
        // #[rox_pub("lidar/filtered")]가 있으면:
        // ctx.publishers에서 "lidar/filtered" 키로 sender를 꺼내와서 필드에 연결
    }
}
```

#### 8.2 Example: Simple Robot Pipeline

```
examples/
├── simple_pipeline/     # Sensor -> Filter -> Actuator (단일 머신)
├── multi_robot/         # 2 로봇 + 글로벌 맵 토픽
├── guard_demo/          # Guard boundary violation + audit log
└── replay_demo/         # 로깅 -> 리플레이 라운드트립
```

**simple_pipeline/main.rs:**

```rust
use rox::prelude::*;

#[rox_node]
struct SensorDriver {
    #[rox_pub("robot-01/sensor/data")]
    output: Vec<f32>,
    counter: u64,
}

#[async_trait]
impl RoxNode for SensorDriver {
    fn name(&self) -> &str { "sensor_driver" }
    async fn init(&mut self, _ctx: &mut NodeContext) -> Result<()> { Ok(()) }
    async fn tick(&mut self, ctx: &mut NodeContext) -> Result<()> {
        let data = vec![self.counter as f32; 10];
        let msg = Arc::new(RoxMessage::new(
            KeyExpr::new("robot-01", "sensor", "data"),
            ctx.node_id.clone(),
            self.counter,
            data.to_bytes()?,
        ));
        ctx.publishers["robot-01/sensor/data"].send(msg)?;
        self.counter += 1;
        Ok(())
    }
    async fn shutdown(&mut self, _ctx: &mut NodeContext) -> Result<()> { Ok(()) }
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = RoxConfig::from_file("config/rox.yml")?;
    let mut engine = RoxEngine::from_config(config)?;

    let mut registry = NodeRegistry::new();
    registry.register("SensorDriver", || Box::new(SensorDriver::default()));
    registry.register("FilterNode", || Box::new(FilterNode::default()));
    registry.register("ActuatorNode", || Box::new(ActuatorNode::default()));

    engine.init_nodes(&registry).await?;
    engine.run().await?;
    Ok(())
}
```

#### 8.3 검증 기준
- `cargo run --example simple_pipeline` 이 5초 동안 실행되고 정상 종료
- 로그에 "sensor -> filter -> actuator" 데이터 흐름 확인

---

### Phase 9: ROS 2 Bridge 실제 연결
**목표:** ROS 2 토픽과 ROX 토픽 간 실시간 브릿지
**의존성:** Phase 2, Phase 5
**영향 크레이트:** `rox-bridge`

#### 9.1 현재 상태

`TopicBridge`는 매핑 테이블 + export/import 함수가 있지만, 실제 ROS 2 연결이 없다.

#### 9.2 ROS 2 연결 방식

**Option A: `r2r` crate (Rust ROS 2 client)**
```toml
[dependencies]
r2r = { version = "0.9", optional = true }
```

**Option B: Zenoh-ROS 2 bridge 경유**
- ROX -> Zenoh bridge -> zenoh-plugin-ros2dds -> ROS 2
- 장점: Zenoh 생태계 활용
- 단점: 간접 경로

**권장: Option A** (직접 연결)

```rust
pub struct Ros2Bridge {
    topic_bridge: TopicBridge,
    ros_node: r2r::Node,
    publishers: HashMap<String, r2r::Publisher<r2r::std_msgs::msg::Bytes>>,
    subscribers: Vec<r2r::Subscription<r2r::std_msgs::msg::Bytes>>,
}

impl Ros2Bridge {
    pub async fn start(&mut self, topic_manager: Arc<TopicManager>) -> Result<()> {
        // 1. 매핑된 Export 토픽: ROX 구독 -> ROS 2 발행
        for mapping in self.topic_bridge.export_mappings() {
            let mut rx = topic_manager.subscribe(&KeyExpr(mapping.rox_topic.clone())).await?;
            let pub_ = self.ros_node.create_publisher(&mapping.external_topic)?;

            tokio::spawn(async move {
                while let Ok(msg) = rx.recv().await {
                    let ros_msg = r2r::std_msgs::msg::Bytes { data: msg.payload.to_vec() };
                    pub_.publish(&ros_msg)?;
                }
            });
        }

        // 2. 매핑된 Import 토픽: ROS 2 구독 -> ROX 발행
        for mapping in self.topic_bridge.import_mappings() {
            let sub = self.ros_node.subscribe(&mapping.external_topic)?;
            let tm = Arc::clone(&topic_manager);
            let rox_topic = mapping.rox_topic.clone();

            tokio::spawn(async move {
                while let Some(msg) = sub.next().await {
                    let rox_msg = Arc::new(RoxMessage::new(
                        KeyExpr(rox_topic.clone()),
                        NodeId("ros2_bridge".to_string()),
                        0,
                        Bytes::from(msg.data),
                    ));
                    tm.inject(rox_msg).await.ok();
                }
            });
        }

        Ok(())
    }
}
```

#### 9.3 검증 기준
- ROS 2가 설치된 환경에서 `cargo test -p rox-bridge --features ros2`
- ROX publisher -> ROS 2 subscriber 메시지 도달 확인
- feature flag `bridge-ros2`가 없으면 컴파일에서 제외

---

### Phase 10: 성능 최적화
**목표:** README의 Performance Targets 달성
**의존성:** Phase 1-6 전부
**영향 크레이트:** 전체

#### 10.1 목표 vs 현재

| Metric | Target | 현재 | Phase |
|--------|--------|------|-------|
| SHM Pub/Sub latency | < 5 us | N/A (미구현) | Phase 6 |
| TCP throughput | > 100K msg/sec | 미측정 | Phase 10 |
| Guard validation | < 10 us | ~구현됨 (inline) | Phase 4 |
| Wire encoding | > 1M msg/sec | 미측정 | Phase 10 |
| Log write throughput | > 1M entries/sec | 미측정 | Phase 10 |
| Agent cycle | 100ms | 100ms (설정됨) | Phase 3 |
| Healing failover | < 50ms | 미측정 | Phase 3 |

#### 10.2 벤치마크 스위트

```rust
// benches/throughput.rs
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_wire_encoding(c: &mut Criterion) {
    let msg = RoxMessage::new(...);
    c.bench_function("wire_encode", |b| {
        b.iter(|| WireEncoder::encode(&msg))
    });
}

fn bench_wire_decoding(c: &mut Criterion) {
    let encoded = WireEncoder::encode(&msg).unwrap();
    c.bench_function("wire_decode", |b| {
        b.iter(|| WireDecoder::decode(encoded.clone()))
    });
}

fn bench_bincode_roundtrip(c: &mut Criterion) {
    let data = PointCloud { points: vec![[1.0; 3]; 1000], intensity: vec![0.5; 1000] };
    c.bench_function("bincode_roundtrip", |b| {
        b.iter(|| {
            let bytes = BincodeCodec::encode(&data).unwrap();
            let _: PointCloud = BincodeCodec::decode(&bytes).unwrap();
        })
    });
}

fn bench_topic_pubsub(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    c.bench_function("topic_pubsub_100k", |b| {
        b.iter(|| rt.block_on(pubsub_100k()))
    });
}

fn bench_guard_validation(c: &mut Criterion) {
    let mut validator = CommandValidator::new();
    validator.add_boundary(SafetyBoundary { ... });
    let msg = RoxMessage::new(...);
    c.bench_function("guard_validate", |b| {
        b.iter(|| validator.validate(&key, &msg))
    });
}
```

#### 10.3 mmap 로그 최적화

현재 `RoxLogger`는 `BufWriter<File>` 사용. > 1M entries/sec 달성을 위해:

```rust
pub struct MmapLogger {
    mmap: MmapMut,
    write_offset: usize,
    capacity: usize,
    file_index: u32,  // rotation
}

impl MmapLogger {
    /// 사전 할당된 mmap에 직접 쓰기 (syscall 최소화)
    pub fn write_entry(&mut self, entry: &[u8]) -> Result<()> {
        if self.write_offset + entry.len() > self.capacity {
            self.rotate()?;
        }
        self.mmap[self.write_offset..self.write_offset + entry.len()]
            .copy_from_slice(entry);
        self.write_offset += entry.len();
        Ok(())
    }

    fn rotate(&mut self) -> Result<()> {
        self.mmap.flush()?;
        self.file_index += 1;
        // 새 mmap 파일 생성
        Ok(())
    }
}
```

#### 10.4 TCP Zero-Copy

현재 TCP는 메시지마다 `write_all` + `flush`. 개선:

```rust
/// Batch writer: 여러 메시지를 하나의 TCP write로 전송
pub struct BatchTcpWriter {
    buffer: BytesMut,
    max_batch_size: usize,
    flush_interval: Duration,
}

impl BatchTcpWriter {
    pub fn queue(&mut self, data: &[u8]) {
        self.buffer.put_u32_le(data.len() as u32);
        self.buffer.extend_from_slice(data);
    }

    pub async fn flush(&mut self, stream: &mut TcpStream) -> Result<()> {
        if !self.buffer.is_empty() {
            stream.write_all(&self.buffer).await?;
            self.buffer.clear();
        }
        Ok(())
    }
}
```

---

## 실행 순서 (의존성 DAG)

```
Phase 1 (Node Registry) ─────────┬──── Phase 4 (Guard Integration)
                                  │
Phase 2 (Transport Integration) ──┼──── Phase 5 (4-Phase Switch) ──── Phase 6 (SHM)
                                  │
Phase 3 (Metrics Pipeline) ───────┘
                                       Phase 7 (CLI + Hot Reload)
Phase 8 (Derive + Examples) ──────────── Phase 9 (ROS 2 Bridge)

Phase 10 (Performance) ── depends on all above
```

**병렬 가능:**
- Phase 1 + Phase 2 (독립)
- Phase 4 + Phase 5 (Phase 2 완료 후 병렬)
- Phase 7 + Phase 8 (Phase 2 완료 후 병렬)

**크리티컬 패스:** Phase 1 -> Phase 2 -> Phase 3 -> Phase 10

---

## 예상 LOC 추가량

| Phase | 예상 LOC | 난이도 |
|-------|----------|--------|
| 1. Node Registry | ~400 | Medium |
| 2. Transport Integration | ~800 | Hard |
| 3. Metrics Pipeline | ~300 | Medium |
| 4. Guard Integration | ~500 | Medium |
| 5. 4-Phase Switch | ~400 | Hard |
| 6. SHM Transport | ~600 | Hard |
| 7. CLI + Hot Reload | ~300 | Easy |
| 8. Derive + Examples | ~500 | Medium |
| 9. ROS 2 Bridge | ~400 | Medium (외부 의존) |
| 10. Performance | ~500 | Hard |
| **Total** | **~4,700** | |

현재 6,700 LOC + 4,700 = **~11,400 LOC** (최종)

---

## MVP 정의 (Phase 1-3 완료 시)

Phase 1-3이 완료되면 다음 데모가 동작해야 한다:

```bash
# Terminal 1: ROX 실행
cargo run -- run --config examples/simple_pipeline/config.yml

# Terminal 2: Monitor
cargo run -- monitor

# Expected output:
# [INFO] rox engine initialized (3 nodes, 2 edges)
# [INFO] sensor_driver: publishing sensor/data (10Hz)
# [INFO] filter_node: received 10 points, filtered to 7
# [INFO] actuator_node: executing velocity command 1.2 m/s
# [INFO] agent: observing (0h, 150 data points, 100% coverage)
# [INFO] agent: CongestionWarning on sensor/data (85%)
```

이것이 동작하면 "프로토타입"에서 "동작하는 미들웨어"로 전환된다.
