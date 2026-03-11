# Agent E: 인터페이스 & 통합 (rox-api, rox-cli, rox-bridge)

## 너의 역할
Rox 프로젝트의 **사용자 대면 인터페이스**를 만든다.
REST API + SSE 이벤트 스트리밍, CLI 도구, ROS 2 브릿지 —
운영자가 Rox를 관리하고, 기존 ROS 2 생태계와 연동할 수 있게 한다.

## 반드시 지킬 것
- `contracts/shared_types.rs`의 AgentEvent, NodeConfig, HealthStatus 타입을 API 응답에 사용
- `TopicRegistry`를 통해 토픽 정보 조회
- API는 `axum`, 실시간 이벤트는 SSE (Server-Sent Events)
- CLI는 `clap` 기반 서브커맨드 구조

## 구현 대상

### 1. rox-api — REST/gRPC 관리 API

#### state.rs — 앱 상태
```rust
pub struct AppState {
    pub registry: Arc<dyn TopicRegistry>,
    pub agent_event_bus: broadcast::Sender<AgentEvent>,
    pub config: Arc<RwLock<RoxConfig>>,
    pub node_health: Arc<RwLock<HashMap<NodeId, HealthStatus>>>,
}
```

#### routes.rs — 라우터 구성
```rust
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // 노드 관리
        .route("/v1/nodes", get(list_nodes))
        .route("/v1/nodes/:id", get(get_node))
        .route("/v1/nodes/:id/health", get(get_node_health))
        // 토픽 관리
        .route("/v1/topics", get(list_topics))
        .route("/v1/topics/:key", get(get_topic_detail))
        // Agent 이벤트
        .route("/v1/events", get(list_events))
        .route("/v1/events/stream", get(event_sse_stream))
        .route("/v1/events/stats", get(event_stats))
        // Guard 감사 로그
        .route("/v1/audit", get(list_audit_entries))
        // 설정
        .route("/v1/config", get(get_config).patch(update_config))
        // 시스템
        .route("/v1/health", get(health_check))
        .route("/v1/metrics", get(prometheus_metrics))
        .with_state(state)
}
```

#### handler/nodes.rs — 노드 핸들러
```
GET  /v1/nodes           → 활성 노드 목록 (id, type, rate_hz, health)
GET  /v1/nodes/:id       → 노드 상세 (config, metrics, connections)
GET  /v1/nodes/:id/health → 노드 건강 상태
```

#### handler/topics.rs — 토픽 핸들러
```
GET  /v1/topics           → 활성 토픽 목록 (key, publisher 수, subscriber 수, QoS)
GET  /v1/topics/:key      → 토픽 상세 (메시지 rate, latency, 구독자 목록)
```

#### handler/events.rs — Agent 이벤트 핸들러
```
GET  /v1/events           → 최근 이벤트 목록 (페이지네이션)
GET  /v1/events/stream    → SSE 실시간 이벤트 스트림
GET  /v1/events/stats     → 이벤트 통계 (타입별, 토픽별)
```

#### handler/config.rs — 설정 핸들러
```
GET   /v1/config          → 현재 설정 반환
PATCH /v1/config          → 설정 부분 업데이트 → 핫 리로드 트리거
```

#### sse.rs — SSE 스트리밍
```rust
async fn event_sse_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.agent_event_bus.subscribe();
    let stream = async_stream::stream! {
        while let Ok(event) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&event) {
                yield Ok(Event::default().data(json));
            }
        }
    };
    Sse::new(stream)
}
```

#### store.rs — 이벤트 저장소
```rust
pub struct EventStore {
    events: RwLock<VecDeque<AgentEvent>>,
    max_size: usize,  // 기본 10,000건
}
```
- 인메모리 링 버퍼
- cursor 기반 페이지네이션
- 나중에 trait으로 추상화하여 SQLite 교체 가능

### 2. rox-cli — CLI 도구

#### main.rs — Clap 서브커맨드
```rust
#[derive(Parser)]
#[command(name = "rox", about = "Rox — Intelligent Nerve System for Robotics")]
enum Cli {
    /// 새 Rox 프로젝트 생성
    New { name: String },
    /// Rox 엔진 실행
    Run {
        #[arg(short, long, default_value = "config/rox.yml")]
        config: PathBuf,
    },
    /// 실시간 모니터링
    Monitor {
        #[arg(short, long, default_value = "http://localhost:9090")]
        api_url: String,
    },
    /// 로그 리플레이
    Replay {
        #[arg(short, long)]
        log_file: PathBuf,
    },
    /// 노드 목록 조회
    Nodes {
        #[arg(short, long, default_value = "http://localhost:9090")]
        api_url: String,
    },
    /// 토픽 목록 조회
    Topics {
        #[arg(short, long, default_value = "http://localhost:9090")]
        api_url: String,
    },
}
```

#### commands/new.rs — 프로젝트 생성
- `rox new my-robot` → 템플릿 프로젝트 구조 생성
- Cargo.toml + rox.yml + 예제 노드

#### commands/monitor.rs — 터미널 모니터링
- SSE 연결 → 실시간 이벤트 표시
- 노드 건강 상태 대시보드 (TUI)

### 3. rox-bridge — ROS 2 / Zenoh 브릿지

#### ros2.rs — ROS 2 브릿지
```rust
/// zenoh-plugin-ros2dds 래핑 방식
pub struct Ros2Bridge {
    // Rox Topic → Zenoh → ROS 2 DDS 경로
    topic_mappings: HashMap<KeyExpr, String>,  // Rox key → ROS 2 topic
}

impl Ros2Bridge {
    pub async fn start(&self, registry: Arc<dyn TopicRegistry>) -> Result<()>;
    pub fn map_topic(&mut self, rox_key: KeyExpr, ros2_topic: &str);
}
```
- Phase 1: 설정 파일 기반 토픽 매핑
- Phase 5: 자동 디스커버리

#### zenoh.rs — Zenoh 호환 브릿지
- Rox ↔ Zenoh 프로토콜 변환
- KeyExpr 호환성 유지

## rox-core 없이 먼저 개발하는 방법
```rust
// mock_state.rs
pub fn mock_app_state() -> AppState {
    let registry = Arc::new(MockTopicRegistry::new());
    let event_bus = mock_agent_event_bus();
    AppState {
        registry,
        agent_event_bus: event_bus,
        config: Arc::new(RwLock::new(RoxConfig::default())),
        node_health: Arc::new(RwLock::new(HashMap::new())),
    }
}
```

## 테스트 시나리오

### rox-api
1. GET /v1/health → 200 OK
2. GET /v1/topics → 빈 배열 (토픽 없을 때)
3. Mock 이벤트 발행 → GET /v1/events → 이벤트 목록 반환
4. SSE 연결 → Mock 이벤트 수신 확인
5. PATCH /v1/config → 설정 변경 반영 확인
6. GET /v1/nodes → 노드 목록 + 건강 상태
7. GET /v1/metrics → Prometheus 형식 메트릭

### rox-cli
1. `rox new test-project` → 프로젝트 구조 생성 확인
2. `rox nodes` → API에서 노드 목록 가져오기
3. `rox topics` → 토픽 목록 출력

### rox-bridge
1. 토픽 매핑: Rox "robot-01/lidar/scan" → ROS 2 "/lidar/scan"
2. 메시지 변환: RoxMessage → ROS 2 메시지 (구조체 변환)

## 완료 기준
- `cargo test -p rox-api` 전부 통과
- `cargo test -p rox-cli` 전부 통과
- `cargo test -p rox-bridge` 전부 통과
- 모든 API 엔드포인트 정상 응답
- SSE로 실시간 이벤트 수신 동작
- CLI 서브커맨드 전부 동작
