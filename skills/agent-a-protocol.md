# Agent A: 프로토콜 & 직렬화 기반 (rox-protocol, rox-codec, rox-buffer)

## 너의 역할
Rox 프로젝트의 **데이터 포맷 기반 계층**을 만든다.
와이어 프로토콜, 직렬화 엔진, zero-copy 버퍼 — 모든 크레이트가 의존하는 최하위 계층이다.
너는 가장 먼저 시작하고, 다른 에이전트들에게 데이터 타입과 인코딩을 제공한다.

## 반드시 지킬 것
- `contracts/shared_types.rs`의 RoxMessage, MessageHeader, KeyExpr, QoSMetadata를 정확히 구현
- KeyExpr의 네임스페이스 규칙 (`{robot_id}/{subsystem}/{topic}`) 반드시 강제
- 다른 에이전트가 `use rox_protocol::*;`로 접근하므로 public API 시그니처 변경 시 모든 에이전트에게 알릴 것

## 구현 대상

### 1. rox-protocol — 와이어 프로토콜 정의

#### message.rs — RoxMessage 구현
```rust
pub struct RoxMessage {
    pub header: MessageHeader,
    pub payload: Bytes,
}

impl RoxMessage {
    pub fn new(key: KeyExpr, payload: Bytes, source: NodeId) -> Self;
    pub fn with_qos(self, qos: QoSMetadata) -> Self;
    pub fn size(&self) -> usize;
}
```

#### header.rs — 메시지 헤더
```rust
pub struct MessageHeader {
    pub key: KeyExpr,
    pub timestamp: RoxTimestamp,
    pub qos: QoSMetadata,
    pub source_id: NodeId,
    pub sequence: u64,
}
```

#### keyexpr.rs — 키 표현식 (zenoh 참조)
```rust
impl KeyExpr {
    pub fn new(robot_id: &str, subsystem: &str, topic: &str) -> Self;
    pub fn global(topic: &str) -> Self;
    pub fn robot_id(&self) -> Option<&str>;
    pub fn matches(&self, pattern: &str) -> bool;  // 와일드카드 매칭
    pub fn validate(&self) -> Result<()>;  // 네임스페이스 규칙 검증
}
```

#### wire.rs — 와이어 포맷 인코딩/디코딩
- `nom` 크레이트로 바이너리 파싱
- 메시지 프레이밍: `[length:4][header:variable][payload:variable]`
- CRC32 체크섬 옵션

### 2. rox-codec — 직렬화/역직렬화

#### bincode.rs — 기본 바이너리 직렬화
```rust
pub trait RoxSerialize: Send + Sync + 'static {
    fn serialize(&self) -> Result<Bytes>;
    fn deserialize(data: &[u8]) -> Result<Self> where Self: Sized;
}
```

#### robotics_types.rs — 로보틱스 특화 타입
- `PointCloud` — 3D 점군 데이터 (LiDAR)
- `CmdVel` — 속도 명령 (선속도 + 각속도)
- `LaserScan` — 2D 레이저 스캔
- `Imu` — 관성 측정 데이터
- `Image` — 이미지 프레임 (width, height, encoding, data)

### 3. rox-buffer — zero-copy 버퍼

#### zbuf.rs — ZBuf (zenoh 참조)
```rust
/// 비연속 zero-copy 버퍼
pub struct ZBuf {
    slices: Vec<ZSlice>,
}

pub struct ZSlice {
    buf: Arc<dyn AsRef<[u8]> + Send + Sync>,
    start: usize,
    end: usize,
}

impl ZBuf {
    pub fn new() -> Self;
    pub fn push(&mut self, slice: ZSlice);
    pub fn len(&self) -> usize;
    pub fn contiguous(&self) -> Cow<[u8]>;  // 필요시 단일 버퍼로 병합
}
```

#### pool.rs — Memory Pool (copper 참조)
```rust
/// Pre-allocated 메모리 풀 (GC 없는 재사용)
pub struct MemoryPool {
    buffers: Vec<Arc<Mutex<Vec<u8>>>>,
    free_list: crossbeam_queue::ArrayQueue<usize>,
    buffer_size: usize,
}

impl MemoryPool {
    pub fn new(capacity: usize, buffer_size: usize) -> Self;
    pub fn alloc(&self) -> Result<PooledBuffer>;
    pub fn free(&self, buf: PooledBuffer);
}
```

#### shm.rs — 공유 메모리 관리
- `memmap2` 기반 mmap 래핑
- Phase 4에서 iceoryx2 SHM과 통합

## 의존성 (이미 Cargo.toml에 정의됨)
- rox-protocol: bytes, serde, nom, async-trait, anyhow, tracing
- rox-codec: rox-protocol, bytes, serde, bincode, anyhow
- rox-buffer: bytes, serde, anyhow, memmap2

## 테스트 시나리오
1. KeyExpr 생성 + 네임스페이스 검증: `KeyExpr::new("robot-01", "lidar", "scan")` → 유효
2. KeyExpr 와일드카드 매칭: `"robot-01/*/scan".matches("robot-01/lidar/scan")` → true
3. RoxMessage 직렬화 → 역직렬화 → 원본과 동일
4. 와이어 인코딩: encode → decode 라운드트립 검증
5. ZBuf: push 3개 슬라이스 → contiguous() → 올바른 단일 버퍼
6. MemoryPool: 할당 → 사용 → 반환 → 재할당 (같은 버퍼 재사용)
7. 로보틱스 타입: CmdVel, PointCloud 직렬화/역직렬화

## 완료 기준
- `cargo test -p rox-protocol` 전부 통과
- `cargo test -p rox-codec` 전부 통과
- `cargo test -p rox-buffer` 전부 통과
- 다른 에이전트가 `use rox_protocol::*;`로 KeyExpr, RoxMessage 사용 가능
- 와이어 인코딩 벤치마크: 1M messages/sec 이상
