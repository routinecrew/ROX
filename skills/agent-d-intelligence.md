# Agent D: AI 에이전트 & 안전 (rox-agent, rox-guard)

## 너의 역할
Rox 프로젝트의 **지능형 통신 관제와 안전 검증 레이어**를 만든다.
QoS 예측, 자가 치유, 이상 탐지, 주파수 제한 에이전트 + 명령 검증, 안전 경계, 감사 로그.
이것이 경쟁사(zenoh, copper, iceoryx2)와의 핵심 차별점이다.

**원칙: Agent는 데이터 경로(hot path) 밖에서 동작하며, 통신 메타데이터만 관리한다.**

## 반드시 지킬 것
- `contracts/shared_types.rs`의 AgentEvent, PolicyUpdateType, ValidationResult 타입 사용
- Agent는 별도 스레드/낮은 우선순위로 실행 — 실시간 태스크에 영향 없음
- Guard는 별도 프로세스 격리 가능하도록 설계 (SHM 통신)
- ML 모델은 Level 0(규칙 기반)부터 시작. ONNX/tract은 나중에 연동

## 구현 대상

### 1. rox-agent — 지능형 통신 관제

#### runtime.rs — Agent 런타임
```rust
pub struct AgentRuntime {
    config: AgentGraphConfig,
    mode: AgentStartMode,        // Observer → Suggestion → Autonomous
    ml_level: MlLevel,           // Level 0=규칙 → 1=자기학습 → 2=사전학습
    policy_history: VecDeque<PolicySnapshot>,
    qos_agent: Option<QoSPredictionAgent>,
    healing_agent: Option<SelfHealingAgent>,
    anomaly_agent: Option<AnomalyDetectionAgent>,
    throttle_agent: Option<ThrottleAgent>,
    rule_engine: RuleEngine,
}

impl AgentRuntime {
    pub async fn run(&mut self);
    async fn check_and_rollback(&mut self, applied: &VersionedPolicyUpdate);
}
```

#### qos_agent.rs — Predictive QoS Agent
```rust
pub struct QoSPredictionAgent {
    history: VecDeque<TransportMetrics>,
    window_size: usize,
    latency_threshold_us: u64,
    jitter_threshold_us: u64,
}

impl QoSPredictionAgent {
    pub fn observe(&mut self, metrics: &TransportMetrics);
    pub async fn evaluate(&mut self) -> Vec<AgentEvent>;
}
```
- Level 0: 통계 임계치 (평균 latency, jitter 비교)
- Level 1: 온라인 AutoEncoder (Phase 3)
- Level 2: ONNX 모델 (Phase 6+)

#### healing_agent.rs — Self-healing Agent
```rust
pub struct SelfHealingAgent {
    node_health: HashMap<NodeId, NodeHealthState>,
    topology: petgraph::Graph<NodeId, ConnectionInfo>,
    failover_timeout: Duration,
}
```
- HealthStatus: Healthy → Degraded → Suspected → Failed
- petgraph로 대체 경로 계산 (Dijkstra)
- 장애 확정 시 FailoverActivated 이벤트

#### anomaly_agent.rs — 이상 탐지
```rust
pub struct AnomalyDetectionAgent {
    baseline: HashMap<KeyExpr, BaselineStats>,
    threshold_sigma: f64,  // 표준편차 배수
}
```
- 정상 상태 baseline 학습
- Z-score 기반 이상 탐지

#### throttle_agent.rs — 주파수 제한 (Pruning 대체)
```rust
pub struct ThrottleAgent {
    topic_rates: HashMap<KeyExpr, ThrottleConfig>,
}
```
- 토픽별 발행 주파수 제한
- 대역폭 절약을 위한 데이터 간격 조정

#### rule_engine.rs — 조건 → 액션 매핑
```rust
pub struct RuleEngine {
    rules: Vec<CompiledRule>,
}

pub struct CompiledRule {
    pub condition: Condition,
    pub action: PolicyUpdateType,
    pub cooldown: Duration,
    pub last_fired: Option<Instant>,
}
```
- `nom`으로 조건 표현식 파싱
- 지원 조건: `latency > N`, `jitter > N`, `loss > N`, `queue_depth > N`

#### mode_transition.rs — Observer → Suggestion → Autonomous 전환
```rust
pub struct ModeTransitionCriteria {
    pub min_observation_hours: u32,
    pub min_data_points: u64,
    pub min_topic_coverage: f32,
    pub max_anomaly_ratio: f32,
}
```

### 2. rox-guard — 안전 검증 레이어

#### validator.rs — 명령 검증기
```rust
pub struct CommandValidator {
    schemas: HashMap<String, CommandSchema>,
    boundaries: Vec<SafetyBoundary>,
}

pub struct CommandSchema {
    pub velocity_range: Range<f64>,
    pub acceleration_range: Range<f64>,
    pub torque_range: Range<f64>,
    pub valid_zones: Vec<GeoFence>,
}

impl CommandValidator {
    pub fn validate(&self, topic: &KeyExpr, msg: &RoxMessage) -> ValidationResult;
}
```

#### schema.rs — Type-safe 명령 스키마
- 설정 파일(rox.yml)에서 안전 경계 로딩
- 노드별 허용 범위 정의

#### boundary.rs — 물리적 안전 경계
```rust
pub struct SafetyBoundary {
    pub node_id: NodeId,
    pub max_velocity: f64,
    pub max_acceleration: f64,
    pub geofence: GeoFence,
}

pub struct GeoFence {
    pub min_x: f64, pub max_x: f64,
    pub min_y: f64, pub max_y: f64,
}

impl SafetyBoundary {
    pub fn check(&self, cmd: &CmdVel, position: &Position) -> ValidationResult;
}
```

#### audit.rs — 불변 감사 로그
```rust
pub struct AuditLogger {
    writer: BufWriter<File>,
    hasher: blake3::Hasher,  // 해시 체인 무결성
    sequence: AtomicU64,
}

pub struct AuditEntry {
    pub sequence: u64,
    pub timestamp: u64,
    pub entry_type: AuditEntryType,
    pub prev_hash: [u8; 32],
}
```
- blake3 해시 체인으로 변조 방지
- Guard 검증, Agent 정책 변경, 비상 정지 모두 기록

#### watchdog.rs — Guard 프로세스 생존 감시
```rust
pub struct GuardWatchdog {
    heartbeat_interval: Duration,
    timeout: Duration,
    last_heartbeat: Instant,
}
```

#### failsafe.rs — Fail-safe 정책
```rust
pub enum FailsafePolicy {
    BlockAllCommands,
    RepeatLastSafe { last_safe: Option<RoxMessage> },
    EmergencyStop,
}
```

## 테스트 시나리오

### rox-agent
1. QoSAgent: latency 메트릭 임계치 초과 → CongestionWarning 이벤트
2. HealingAgent: 노드 5초 미응답 → Failed → 대체 경로 생성
3. AnomalyAgent: 정상 baseline 학습 → 3σ 이상 → AnomalyDetected
4. ThrottleAgent: 토픽 주파수 200ms → ThrottleApplied 이벤트
5. RuleEngine: "latency > 500 && loss > 0.01" → UpdateQoS 액션
6. 콜드 스타트: Observer 모드 → 정책 변경 없이 로그만 기록
7. 자동 롤백: 정책 적용 후 latency 20% 증가 → 자동 롤백

### rox-guard
1. CommandValidator: velocity 2.0 m/s 제한 → 3.0 요청 → Clamped(3.0 → 2.0)
2. GeoFence: 영역 밖 명령 → Rejected
3. AuditLogger: 10건 기록 → 해시 체인 무결성 검증
4. Watchdog: heartbeat 500ms 타임아웃 → FailsafePolicy 발동
5. EmergencyStop: Guard 3회 재시작 실패 → EmergencyStop

## 완료 기준
- `cargo test -p rox-agent` 전부 통과
- `cargo test -p rox-guard` 전부 통과
- Agent가 hot path에 영향 없음 확인 (별도 스레드)
- Guard 검증 지연: < 10μs (인라인 최적화)
- 감사 로그 해시 체인 100% 무결성
