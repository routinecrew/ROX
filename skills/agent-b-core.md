# Agent B: 코어 런타임 엔진 (rox-core, rox-derive)

## 너의 역할
Rox 프로젝트의 **핵심 런타임 엔진**을 만든다.
Session, Node lifecycle, Topic Pub/Sub, Service Req/Rep, TaskGraph DAG 스케줄러,
Discovery, QoS 정책, Config 핫 리로드 — Rox의 두뇌에 해당한다.
copper-rs의 결정론적 실행 모델 + zenoh의 세션 관리를 참조한다.

## 반드시 지킬 것
- `contracts/shared_types.rs`의 TopicRegistry trait을 정확히 구현 (TopicManager)
- `RoxNode` trait의 `init/tick/shutdown` 생명주기를 구현
- 다른 에이전트가 `Arc<dyn TopicRegistry>`로 접근하므로 trait 시그니처 변경 금지
- TaskGraph 실행은 Hard Deterministic — 같은 입력이면 같은 결과

## 구현 대상

### 1. Config (config.rs)
```rust
#[derive(Deserialize, Clone)]
pub struct RoxConfig {
    pub transport: TransportConfig,
    pub discovery: DiscoveryConfig,
    pub nodes: Vec<NodeConfig>,
    pub connections: Vec<ConnectionConfig>,
    pub agent: Option<AgentGraphConfig>,
    pub guard: Option<GuardConfig>,
    pub logging: LogConfig,
    pub api: Option<ApiConfig>,
}

pub struct ConfigWatcher {
    path: PathBuf,
    watcher: notify::RecommendedWatcher,
    tx: mpsc::Sender<RoxConfig>,
}
```
- `serde_yaml`로 rox.yml 파싱
- `notify` crate로 파일 변경 감지 → 핫 리로드
- 변경 불가 항목(SHM pool size 등)은 재시작 필요 경고 로그

### 2. Session (session.rs)
```rust
/// zenoh Session 참조: Rox 엔진의 진입점
pub struct Session {
    config: RoxConfig,
    topic_manager: Arc<TopicManager>,
    node_registry: HashMap<NodeId, Box<dyn RoxNode>>,
    scheduler: Scheduler,
}

impl Session {
    pub async fn new(config: RoxConfig) -> Result<Self>;
    pub async fn start(&mut self) -> Result<()>;
    pub async fn shutdown(&mut self) -> Result<()>;
}
```

### 3. TopicManager (topic.rs) — TopicRegistry 구현
```rust
pub struct TopicManager {
    topics: RwLock<HashMap<String, TopicEntry>>,
}

struct TopicEntry {
    key: KeyExpr,
    sender: MessageSender,
    qos: QoSMetadata,
    subscriber_count: AtomicUsize,
}
```
- `TopicRegistry` trait 구현체
- 와일드카드 매칭 지원 (`"robot-01/*/scan"`)
- subscriber 수 추적 (메트릭용)

### 4. Node (node.rs) — 노드 생명주기 관리
```rust
pub struct NodeRunner {
    node: Box<dyn RoxNode>,
    context: NodeContext,
    rate: Option<Duration>,
}

impl NodeRunner {
    pub async fn run(&mut self) -> Result<()> {
        self.node.init(&mut self.context).await?;
        loop {
            self.node.tick(&mut self.context).await?;
            if let Some(rate) = self.rate {
                tokio::time::sleep(rate).await;
            }
        }
    }
}
```

### 5. TaskGraph & Scheduler (graph.rs, scheduler.rs)
```rust
/// copper의 Task Graph 참조: DAG 기반 스케줄러
pub struct TaskGraph {
    graph: petgraph::Graph<NodeId, ConnectionConfig>,
    execution_order: Vec<NodeId>,  // 위상 정렬
}

pub struct Scheduler {
    graph: TaskGraph,
    nodes: HashMap<NodeId, NodeRunner>,
    cycle: u64,
    policy_rx: mpsc::Receiver<VersionedPolicyUpdate>,
}

impl Scheduler {
    /// 매 사이클: 정책 적용 → 노드 순차 실행 → 로그 기록
    pub async fn run_cycle(&mut self) -> Result<()>;
}
```

### 6. Discovery (discovery.rs)
- Phase 1: static 모드만 (설정 파일에 피어 목록)
- Phase 5: multicast/gossip 추가 예정
```rust
pub struct StaticDiscovery {
    peers: Vec<SocketAddr>,
}
```

### 7. QoS (qos.rs)
```rust
pub struct QoSManager {
    policies: HashMap<KeyExpr, QoSMetadata>,
}

impl QoSManager {
    pub fn apply_policy(&mut self, update: &PolicyUpdateType);
    pub fn get_policy(&self, key: &KeyExpr) -> QoSMetadata;
}
```

### 8. rox-derive — 선언적 노드 매크로
```rust
// 사용 예시:
#[rox_node]
struct LidarProcessor {
    #[rox_sub("lidar/raw")]
    input: PointCloud,
    #[rox_pub("lidar/filtered")]
    output: PointCloud,
    #[rox_param(default = 0.1)]
    voxel_size: f32,
}
```
- `syn`/`quote`로 proc-macro 구현
- `#[rox_sub]` → NodeContext.subscribers 자동 등록
- `#[rox_pub]` → NodeContext.publishers 자동 등록
- `#[rox_param]` → Config에서 값 로딩

## 의존성
rox-core는 rox-protocol, rox-codec, rox-buffer에 의존.
Agent A가 먼저 완성되어야 하지만, contracts/mock.rs로 독립 개발 가능.

## 테스트 시나리오
1. Config 로딩: rox.yml 파싱 → RoxConfig 구조체
2. TopicManager pub/sub: publish("robot-01/lidar/scan") → subscribe → 메시지 수신
3. TaskGraph: 3노드 DAG → 위상 정렬 → 올바른 실행 순서
4. Scheduler: 10 사이클 실행 → 각 사이클에서 노드가 올바른 순서로 tick
5. 핫 리로드: YAML 수정 → Config 변경 감지 이벤트 발생
6. QoS 정책 적용: Agent가 보낸 PolicyUpdate → 토픽 우선순위 변경

## 완료 기준
- `cargo test -p rox-core` 전부 통과
- `cargo test -p rox-derive` 전부 통과
- TopicManager가 TopicRegistry trait을 올바르게 구현
- Scheduler가 결정론적 실행 보장 (같은 입력 → 같은 실행 순서)
- 다른 에이전트가 `Arc<dyn TopicRegistry>`로 사용 가능
