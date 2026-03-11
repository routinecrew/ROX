# Agent C: 통신 & 로깅 (rox-transport, rox-log, rox-replay)

## 너의 역할
Rox 프로젝트의 **통신 인프라와 로깅/리플레이 시스템**을 만든다.
멀티 트랜스포트(SHM, TCP, UDP, Serial), Agent 연동 동적 전환,
결정론적 로깅, 비트 단위 리플레이 — Rox의 통신 백본이다.
zenoh의 transport 계층 + iceoryx2의 SHM + copper의 deterministic log를 참조한다.

## 반드시 지킬 것
- `contracts/shared_types.rs`의 `TransportKind`, `TransportMetrics` 타입 사용
- `Transport` trait을 정의하고, 각 전송 방식이 이를 구현
- TransportSelector는 Agent의 힌트를 받아 동적 전환 가능해야 함
- 로그는 결정론적: 같은 로그 파일 → 같은 리플레이 결과

## 구현 대상

### 1. rox-transport — 멀티 트랜스포트

#### lib.rs — Transport trait
```rust
#[async_trait]
pub trait Transport: Send + Sync + 'static {
    async fn send(&self, msg: &RoxMessage) -> Result<()>;
    async fn recv(&self) -> Result<RoxMessage>;
    fn kind(&self) -> TransportKind;
    fn estimated_latency_us(&self) -> u64;
    fn bandwidth_usage(&self) -> f32;
}
```

#### tcp.rs — TCP/TLS 전송
```rust
pub struct TcpTransport {
    listener: Option<TcpListener>,
    connections: HashMap<NodeId, TcpStream>,
}
```
- tokio TCP 비동기 I/O
- 메시지 프레이밍: length-prefixed
- TLS 옵션 (Phase 3)

#### udp.rs — UDP 전송
```rust
pub struct UdpTransport {
    socket: UdpSocket,
}
```
- 저지연 센서 데이터용
- 손실 허용 (BestEffort QoS)

#### serial.rs — Serial 전송
```rust
pub struct SerialTransport {
    port: tokio_serial::SerialPort,
    baud_rate: u32,
}
```
- 임베디드 MCU 통신용
- UART/SPI 프로토콜

#### shm.rs — SHM 전송 (iceoryx2 래핑)
```rust
#[cfg(feature = "shm")]
pub struct Iceoryx2ShmTransport {
    node: iceoryx2::node::Node<iceoryx2::service::ipc::Service>,
    publishers: HashMap<KeyExpr, Iceoryx2Publisher>,
    subscribers: HashMap<KeyExpr, Iceoryx2Subscriber>,
}
```
- iceoryx2 래핑 (feature flag `shm`)
- Phase 1에서는 mock SHM으로 구현, Phase 4에서 iceoryx2 실제 연동

#### selector.rs — TransportSelector (Agent 연동)
```rust
pub struct TransportSelector {
    transports: Vec<Box<dyn Transport>>,
    active: HashMap<NodeId, TransportKind>,
    agent_hint: Option<AgentTransportHint>,
}

impl TransportSelector {
    pub async fn select(&self, target: &NodeId) -> &dyn Transport;
    /// 4단계 전환 프로토콜: drain → buffer → switch → replay
    pub async fn switch_transport(
        &mut self, target: &NodeId, from: TransportKind, to: TransportKind,
    ) -> Result<TransportSwitchReport>;
}
```

### 2. rox-log — 결정론적 로깅

#### logger.rs — 구조화 로거
```rust
pub struct RoxLogger {
    writer: BufWriter<File>,
    codec: bincode::Serializer,
}

impl RoxLogger {
    pub fn record_message(&mut self, msg: &RoxMessage) -> Result<()>;
    pub fn record_cycle_complete(&mut self, cycle: u64) -> Result<()>;
    pub fn record_policy_applied(&mut self, cycle: u64, update: &VersionedPolicyUpdate) -> Result<()>;
}
```

#### log_entry.rs — 로그 엔트리 정의
```rust
#[derive(Serialize, Deserialize)]
pub enum LogEntry {
    Message { cycle: u64, msg: RoxMessageCompact },
    CycleComplete { cycle: u64, timestamp: u64 },
    PolicyApplied { cycle: u64, update: VersionedPolicyUpdate },
    NodeTick { cycle: u64, node_id: NodeId, duration_us: u64 },
}
```

#### mmap_writer.rs — mmap 기반 고성능 쓰기
- `memmap2`로 memory-mapped file 쓰기
- 로그 로테이션 (파일 크기 제한)

### 3. rox-replay — 비트 단위 리플레이

#### reader.rs — 로그 리더
```rust
pub struct LogReader {
    mmap: Mmap,
    position: usize,
}

impl LogReader {
    pub fn open(path: &Path) -> Result<Self>;
    pub fn next_entry(&mut self) -> Result<Option<LogEntry>>;
    pub fn seek_to_cycle(&mut self, cycle: u64) -> Result<()>;
}
```

#### replay_engine.rs — 리플레이 엔진
```rust
pub struct ReplayEngine {
    reader: LogReader,
    clock: ReplayClock,
}

impl ReplayEngine {
    /// 로그에서 메시지를 읽어 원래 타이밍으로 재생
    pub async fn replay(&mut self) -> Result<()>;
    /// Agent 정책은 로그에서 기록된 값 주입 (ML 재실행 안 함)
    pub fn inject_policy(&self, cycle: u64) -> Option<VersionedPolicyUpdate>;
}
```

## rox-core 없이 먼저 개발하는 방법
- `contracts/mock.rs`의 `MockTopicRegistry`를 사용
- Transport trait 구현체들은 rox-core 없이 독립 테스트 가능
- 로거는 RoxMessage를 직접 생성하여 기록/읽기 테스트

## 테스트 시나리오
1. TcpTransport: send → recv 라운드트립 (localhost)
2. UdpTransport: send → recv (BestEffort)
3. TransportSelector: 로컬 대상 → SHM 선택, 원격 대상 → TCP 선택
4. Transport 전환: TCP → SHM 4단계 프로토콜, drained/replayed 카운트 검증
5. RoxLogger: 10개 메시지 기록 → LogReader로 읽기 → 원본과 동일
6. ReplayEngine: 기록된 로그 → 재생 → 타임스탬프 순서 검증
7. mmap writer: 1M 엔트리 쓰기 → 읽기 → 무결성 검증

## 완료 기준
- `cargo test -p rox-transport` 전부 통과
- `cargo test -p rox-log` 전부 통과
- `cargo test -p rox-replay` 전부 통과
- TcpTransport 벤치마크: 100K msg/sec (localhost)
- 로그 쓰기 벤치마크: 1M entries/sec
- Transport 전환 시 메시지 유실 제로
