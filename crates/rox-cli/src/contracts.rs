// ============================================================
// ROX 공유 계약 (Shared Contracts)
// ============================================================
// 모든 에이전트는 이 파일의 타입과 trait을 기준으로 개발한다.
// 이 파일을 수정하려면 반드시 모든 에이전트에게 알려야 한다.
// ============================================================

// ----- 의존성 -----
// bytes = "1"
// tokio = { version = "1", features = ["full"] }
// serde = { version = "1", features = ["derive"] }
// async-trait = "0.1"

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

// ============================================================
// 1. KeyExpr — 계층적 키 표현식 (zenoh 참조)
// ============================================================

/// 멀티 로봇 네임스페이스 강제 규칙:
///   형식: "{robot_id}/{subsystem}/{topic}"
///   글로벌: "_global/{topic}"
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyExpr(pub String);

impl KeyExpr {
    /// 네임스페이스 규칙을 강제하는 생성자
    pub fn new(robot_id: &str, subsystem: &str, topic: &str) -> Self {
        Self(format!("{robot_id}/{subsystem}/{topic}"))
    }

    /// 글로벌 토픽 (로봇 ID 불필요)
    pub fn global(topic: &str) -> Self {
        Self(format!("_global/{topic}"))
    }

    /// 로봇 ID 추출
    pub fn robot_id(&self) -> Option<&str> {
        let first = self.0.split('/').next()?;
        if first == "_global" {
            None
        } else {
            Some(first)
        }
    }

    /// 원본 문자열 반환
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for KeyExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ============================================================
// 2. RoxMessage — 프로토콜 독립 메시지
// ============================================================

/// Rox의 기본 메시지 단위
#[derive(Clone, Debug)]
pub struct RoxMessage {
    pub header: MessageHeader,
    /// Phase 1: Bytes 단일 타입. Phase 4에서 SHM/Arrow feature flag 확장 가능.
    pub payload: Bytes,
}

#[derive(Clone, Debug)]
pub struct MessageHeader {
    /// 계층적 키 ("robot-01/lidar/scan")
    pub key: KeyExpr,
    /// 결정론적 타임스탬프
    pub timestamp: RoxTimestamp,
    /// QoS 메타데이터
    pub qos: QoSMetadata,
    /// 발행자 식별
    pub source_id: NodeId,
    /// 순서 번호 (리플레이용)
    pub sequence: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RoxTimestamp {
    /// 하드웨어 클럭 (나노초)
    pub hw_time: u64,
    /// 논리 클럭 (HLC)
    pub logical_time: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QoSMetadata {
    pub priority: Priority,
    pub reliability: Reliability,
    pub durability: Durability,
    pub deadline_us: Option<u64>,
    pub lifespan_us: Option<u64>,
}

impl Default for QoSMetadata {
    fn default() -> Self {
        Self {
            priority: Priority::Normal,
            reliability: Reliability::BestEffort,
            durability: Durability::Volatile,
            deadline_us: None,
            lifespan_us: None,
        }
    }
}

// ============================================================
// 3. QoS 열거형
// ============================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    Highest = 0,
    High = 1,
    AboveNormal = 2,
    Normal = 3,
    BelowNormal = 4,
    Low = 5,
    Background = 6,
    Lowest = 7,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reliability {
    BestEffort,
    Reliable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Durability {
    Volatile,
    TransientLocal,
}

// ============================================================
// 4. NodeId — 노드 식별자
// ============================================================

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ============================================================
// 5. Transport 관련 타입
// ============================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportKind {
    SharedMemory,
    Tcp,
    Udp,
    Quic,
    Serial,
}

/// Transport 메트릭 — Agent가 소비
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransportMetrics {
    pub timestamp: u64,
    pub topic: KeyExpr,
    pub latency_us: u64,
    pub jitter_us: u64,
    pub throughput_bps: u64,
    pub packet_loss_ratio: f32,
    pub queue_depth: u32,
    pub transport_kind: TransportKind,
}

// ============================================================
// 6. Node 관련 타입
// ============================================================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeConfig {
    pub id: String,
    pub node_type: String,
    pub rate_hz: Option<f64>,
    pub priority: u8,
    pub background: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub from: String,
    pub to: String,
    pub qos: Option<QoSConfigOverride>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QoSConfigOverride {
    pub reliability: Option<Reliability>,
    pub priority: Option<Priority>,
    pub deadline_us: Option<u64>,
}

// ============================================================
// 7. MessageBus — tokio broadcast 채널 타입 별칭
// ============================================================

pub type MessageSender = tokio::sync::broadcast::Sender<Arc<RoxMessage>>;
pub type MessageReceiver = tokio::sync::broadcast::Receiver<Arc<RoxMessage>>;

/// 새 메시지 버스 생성
pub fn new_message_bus() -> (MessageSender, MessageReceiver) {
    let (tx, rx) = tokio::sync::broadcast::channel(1024);
    (tx, rx)
}

// ============================================================
// 8. TopicRegistry trait — 모든 크레이트의 공통 인터페이스
// ============================================================

#[async_trait::async_trait]
pub trait TopicRegistry: Send + Sync + 'static {
    /// 토픽에 발행자 등록
    async fn publish(&self, key: &KeyExpr, qos: QoSMetadata) -> anyhow::Result<MessageSender>;

    /// 토픽 구독
    async fn subscribe(&self, key: &KeyExpr) -> anyhow::Result<MessageReceiver>;

    /// 발행 해제
    async fn unpublish(&self, key: &KeyExpr) -> anyhow::Result<()>;

    /// 활성 토픽 목록
    async fn list_topics(&self) -> Vec<KeyExpr>;
}

// ============================================================
// 9. RoxNode trait — 태스크 노드 인터페이스
// ============================================================

/// 노드 컨텍스트 (Publisher/Subscriber 접근)
pub struct NodeContext {
    pub publishers: HashMap<String, MessageSender>,
    pub subscribers: HashMap<String, MessageReceiver>,
    pub node_id: NodeId,
}

#[async_trait::async_trait]
pub trait RoxNode: Send + Sync + 'static {
    /// 노드 이름
    fn name(&self) -> &str;

    /// 초기화
    async fn init(&mut self, ctx: &mut NodeContext) -> anyhow::Result<()>;

    /// 주기적 실행
    async fn tick(&mut self, ctx: &mut NodeContext) -> anyhow::Result<()>;

    /// 종료
    async fn shutdown(&mut self, ctx: &mut NodeContext) -> anyhow::Result<()>;
}

// ============================================================
// 10. Guard 관련 타입
// ============================================================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ValidationResult {
    Passed,
    Clamped {
        original: f64,
        clamped: f64,
        field: String,
    },
    Rejected {
        reason: String,
    },
    EmergencyStop {
        reason: String,
    },
}

// ============================================================
// 11. Agent 관련 타입
// ============================================================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AgentEvent {
    CongestionWarning {
        topic: KeyExpr,
        probability: f32,
        recommended_priority: Priority,
    },
    NodeDegraded {
        node_id: NodeId,
    },
    FailoverActivated {
        failed_node: NodeId,
        backup_route: Vec<NodeId>,
    },
    AnomalyDetected {
        topic: KeyExpr,
        anomaly_type: String,
        severity: f32,
    },
    ThrottleApplied {
        topic: KeyExpr,
        new_interval_ms: u64,
    },
    PolicyApplied {
        version: u64,
        update_type: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PolicyUpdateType {
    UpdateQoS {
        topic: KeyExpr,
        new_priority: Priority,
    },
    SuggestTransport {
        target: NodeId,
        recommended: TransportKind,
    },
    ThrottleTopic {
        topic: KeyExpr,
        new_interval_ms: u64,
    },
    Alert {
        level: AlertLevel,
        message: String,
    },
    CreateFailover {
        failed_node: NodeId,
        backup_route: Vec<NodeId>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertLevel {
    Info,
    Warning,
    Critical,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VersionedPolicyUpdate {
    pub version: u64,
    pub apply_at_cycle: u64,
    pub confidence: f32,
    pub update: PolicyUpdateType,
}

// ============================================================
// 12. HealthStatus — 노드 건강 상태
// ============================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Suspected,
    Failed,
}

// ============================================================
// 13. Agent 모드 관련
// ============================================================

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AgentStartMode {
    Observer { duration_hours: u32 },
    Suggestion,
    Autonomous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MlLevel {
    RuleBased,
    SelfLearning,
    PreTrained,
}
