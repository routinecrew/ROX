# Rox — 시스템 설계서 v3

> Intelligent Nerve System for Robotics  
> zenoh + iceoryx2 + copper-rs의 강점 통합 + AI Agent 레이어
> ⚡ v3: 2차 전문가 검증 결과 반영 — 설계 불일치 해소 + 세부 설계 보완 (8건)

---

## 변경 이력

| 버전 | 일자 | 변경 내용 |
|------|------|----------|
| v1 | 2026-03-11 | 초기 설계 |
| v2 | 2026-03-11 | 전문가 패널 검증 결과 반영 — 10개 핵심 개선 적용 |
| **v3** | **2026-03-11** | **2차 검증 결과 반영 — 설계 불일치 해소 + 세부 설계 보완 (8건)** |

### v3 주요 변경 사항 요약

| # | 개선 항목 | v2 잔존 문제 | v3 해결 |
|---|----------|-------------|---------|
| 1 | RoxPayload 불일치 | 변경 요약에는 Bytes 단일이나 본문은 3종 enum | 본문을 `Bytes` 단일로 통일, SHM/Arrow는 feature flag 주석 |
| 2 | Agent 다이어그램 Pruning 잔존 | throttling으로 대체했으나 다이어그램 미갱신 | Throttle Agent로 교체 |
| 3 | rox/ 통합 크레이트 미반영 | 의사결정에만 기록, 디렉토리 구조에 없음 | `rox/` 디렉토리 + Cargo.toml feature 구조 추가 |
| 4 | KeyExpr 네임스페이스 미반영 | 멀티 로봇 충돌 방지 규칙 없음 | `{robot_id}/{subsystem}/{topic}` 강제 + 검증 메서드 |
| 5 | Guard 이중화 부재 | 단일 프로세스 장애점 | 자동 재시작 + 재시작 중 BlockAllCommands 명시 |
| 6 | PolicyUpdate 적용 시점 모호 | apply_at_cycle의 스케줄러 동기화 미정의 | cycle boundary drain 슈도코드 추가 |
| 7 | Transport 전환 메시지 유실 | 전환 프로토콜 미설계 | drain→buffer→switch→replay 4단계 프로토콜 |
| 8 | 롤백 판단 기준 부재 | "5초 후 비교"만 언급 | 정량 기준 + Observer 전환 조건 복합화 |

### v2 주요 변경 사항 요약 (참조)

| # | 개선 항목 | 변경 전 | 변경 후 |
|---|----------|--------|--------|
| 1 | SHM 구현 전략 | 자체 lock-free SHM 구현 | iceoryx2 래핑, Phase 4+에서 자체 구현 검토 |
| 2 | 결정론성 범위 | 전체 시스템 결정론적 | Hard/Soft 2계층 분리, PolicyUpdate 버저닝 |
| 3 | Guard 프로세스 | rox-core 내 동일 프로세스 | **별도 프로세스 격리** + watchdog + fail-safe |
| 4 | Agent ML 전략 | ONNX 모델 직접 사용 | **규칙기반→자기학습→사전학습** 3단계 진화 |
| 5 | Agent 초기 동작 | 즉시 자동 정책 변경 | **Observer→Suggestion→Autonomous** 콜드 스타트 |
| 6 | ROS 2 Bridge | 자체 구현 | **zenoh-plugin-ros2dds 래핑** |
| 7 | 크레이트 구조 | 21개 독립 크레이트 | **`rox` 단일 진입점** + feature flag |
| 8 | 안전 등급 | 미정의 | **QM 등급 명시**, SIL 인증은 별도 로드맵 |
| 9 | Semantic Pruning | Phase 3 포함 | **로드맵에서 제거**, frequency throttling으로 대체 |
| 10 | Discovery | multicast/gossip/static | Phase 1에서 **static만 지원** |

---

## 1. 경쟁사 소스코드 구조 분석

Rox의 아키텍처를 설계하기 전에, 경쟁사 5개의 실제 소스코드 구조를 분석한다.
1:1 복제가 아니라 각 프로젝트의 **검증된 설계 패턴**을 추출하여 Rox에 통합한다.

### 1.1 zenoh — 프로토콜 레이어 분리

```
eclipse-zenoh/zenoh/
├── zenoh/                    ← 사용자 API (Session, Publisher, Subscriber)
├── zenoh-ext/                ← 고급 기능 (AdvancedPublisher, 직렬화)
├── zenohd/                   ← 라우터 데몬 (standalone binary)
├── commons/
│   ├── zenoh-protocol/       ← 와이어 프로토콜 정의 (메시지 포맷)
│   ├── zenoh-codec/          ← 직렬화/역직렬화
│   ├── zenoh-buffers/        ← zero-copy 버퍼 추상화
│   ├── zenoh-config/         ← 설정 시스템
│   ├── zenoh-crypto/         ← TLS/인증
│   └── zenoh-shm/            ← 공유 메모리 (feature flag)
├── io/
│   ├── zenoh-transport/      ← 전송 계층 (unicast/multicast)
│   └── zenoh-links/          ← 링크 추상화 (TCP/UDP/TLS/QUIC/WS/Serial)
└── plugins/
    ├── zenoh-plugin-rest/    ← REST API 플러그인
    └── zenoh-plugin-storage-manager/  ← 스토리지 백엔드
```

**Rox에 차용할 패턴:**
- `zenoh-protocol` / `zenoh-transport` / `zenoh-links`의 3계층 분리
- `zenoh-buffers`의 zero-copy 버퍼 추상화
- `zenoh-shm`의 feature flag 기반 SHM 선택적 활성화
- 플러그인 아키텍처 (동적 로딩)

### 1.2 copper-rs — 결정론적 런타임

```
copper-project/copper-rs/
├── cu29/                     ← 메인 SDK 크레이트 (prelude, 매크로 재수출)
├── cu29-runtime/             ← 런타임 코어
│   ├── config.rs             ← RON 기반 Task Graph 설정
│   ├── copperlist.rs         ← 핵심 데이터 구조 (pre-allocated 메시지 큐)
│   ├── curuntime.rs          ← 런타임 실행 엔진
│   └── tasks.rs              ← CuSrcTask / CuTask / CuSinkTask trait 정의
├── cu29-log/                 ← zero-copy 구조화 로깅
├── cu29-log-reader/          ← 로그 리플레이 도구
├── cu29-derive/              ← proc-macro (copper_runtime 매크로)
├── cu29-mem/                 ← Heterogeneous Memory Pool (GPU 포함)
├── components/               ← 하드웨어 드라이버 (센서, 모터, GPIO)
│   ├── cu-hesai/             ← LiDAR 드라이버
│   ├── cu-rp-gpio/           ← Raspberry Pi GPIO
│   ├── cu-zenoh/             ← Zenoh 브릿지
│   └── cu-gstreamer/         ← GStreamer 파이프라인
└── examples/
    └── cu_rp_balancebot/     ← BalanceBot 시뮬레이션 (Bevy + Avian3d)
```

**Rox에 차용할 패턴:**
- `CopperList`의 pre-allocated, 순차 메모리 접근 구조
- `CuSrcTask` / `CuTask` / `CuSinkTask` 3단 trait 분리 (센서→처리→액추에이터)
- RON 기반 Task Graph → 컴파일 타임 스케줄러 생성
- `cu29-log`의 deterministic replay 구조

### 1.3 dora-rs — AI Dataflow

```
dora-rs/dora/
├── apis/
│   ├── python/node/          ← Python API (PyO3 바인딩)
│   ├── rust/node/            ← Rust Node API
│   └── c/node/               ← C API
├── binaries/
│   ├── cli/                  ← dora CLI (dora new / run / build)
│   ├── daemon/               ← dora-daemon (분산 실행)
│   └── coordinator/          ← 멀티 머신 조율
├── libraries/
│   ├── core/                 ← 핵심 타입 정의 (DataId, NodeId)
│   ├── shared-memory-server/ ← SHM 서버 (프로세스 간 zero-copy)
│   └── message/              ← Apache Arrow 기반 메시지
├── node-hub/                 ← 사전 패키징 노드 저장소
│   ├── dora-rerun/           ← Rerun 시각화
│   ├── dora-mediapipe/       ← MediaPipe 포즈 추정
│   ├── dora-mistral-rs/      ← LLM 추론 (Rust)
│   └── dora-vggt/            ← 멀티카메라 뎁스
└── examples/
    └── python-dataflow/      ← YAML 파이프라인 예제
```

**Rox에 차용할 패턴:**
- Apache Arrow 기반 zero-copy 메시지 포맷
- `shared-memory-server`의 메시지 추적/폐기 방식
- `node-hub` 컨셉 (사전 패키징 컴포넌트 저장소)
- YAML 기반 데이터플로우 선언

### 1.4 iceoryx2 — Lock-free IPC

```
eclipse-iceoryx/iceoryx2/
├── iceoryx2/                 ← 메인 크레이트 (Pub/Sub + Req/Res API)
│   └── src/
│       ├── service/          ← Service Discovery + QoS
│       ├── port/             ← Publisher, Subscriber, Client, Server
│       └── node/             ← Node (리소스 소유자)
├── iceoryx2-bb-lock-free/    ← Lock-free 자료구조
│   └── src/
│       ├── mpmc/             ← Multi-Producer Multi-Consumer Queue
│       └── spsc/             ← Single-Producer Single-Consumer
├── iceoryx2-bb-memory/       ← 메모리 관리 (bump allocator 등)
├── iceoryx2-cal/             ← 플랫폼 추상화 레이어
│   └── src/
│       ├── shared_memory/    ← OS별 SHM 구현
│       ├── event/            ← OS별 이벤트 메커니즘
│       └── zero_copy_connection/ ← zero-copy 커넥션
├── iceoryx2-tunnels/         ← 네트워크 터널 (Zenoh 등)
│   └── zenoh/                ← Zenoh 기반 원격 통신
└── iceoryx2-ffi/             ← C / C++ FFI 바인딩
```

**Rox에 차용할 패턴:**
- ⚡ **[v2 변경] iceoryx2를 로컬 IPC 백엔드로 직접 의존하여 래핑 (자체 구현 대신)**
- `iceoryx2-cal`의 플랫폼 추상화 레이어 (OS별 SHM)
- `Service` 개념 (디스커버리 + QoS 정책)
- Daemonless 아키텍처 (중앙 프로세스 불필요)

### 1.5 HORUS — DX 중심 설계

```
softmata/horus/
├── horus/                    ← 통합 크레이트 (node!, message! 매크로)
├── horus_core/
│   ├── communication/        ← Hub (SHM + Network 자동 전환)
│   ├── scheduling/           ← 스케줄러 (우선순위 기반)
│   ├── core/                 ← Node trait, NodeInfo
│   └── memory/               ← 공유 메모리 관리
├── horus_manager/            ← CLI 도구 (horus new / run / monitor)
├── horus_daemon/             ← 원격 배포 데몬
├── horus_macros/             ← node! proc-macro
├── horus_py/                 ← Python 바인딩
├── horus_c/                  ← C 바인딩
├── horus_ai/                 ← AI/Perception 모듈
└── horus_library/
    ├── messages/             ← 표준 메시지 타입 (CmdVel, LaserScan 등)
    └── apps/                 ← 예제 애플리케이션
```

**Rox에 차용할 패턴:**
- `Hub`의 SHM↔Network 자동 전환 구조
- `horus_manager` CLI의 DX (프로젝트 생성→실행→모니터링→배포)
- `node!` 매크로의 선언적 노드 정의
- `horus_ai`를 별도 크레이트로 분리하는 구조

---

## 2. Rox 아키텍처 설계

### 2.1 설계 원칙

경쟁사에서 추출한 패턴을 기반으로 3가지 설계 원칙을 정한다.

1. **통신은 zenoh급, 실행은 copper급, 지능은 독자**
   - 통신 성능: zenoh의 multi-transport + iceoryx2의 lock-free SHM
   - 실행 모델: copper의 deterministic task graph
   - 지능 레이어: 어떤 경쟁사도 갖추지 못한 인프라 수준 AI Agent

2. **Agent는 제어 루프 밖에서만 동작**
   - 데이터 경로(hot path)에 AI 추론이 개입하지 않음
   - Agent는 통신 메타데이터(QoS, 라우팅, 장애 징후)만 관리
   - 로봇의 실시간성을 해치지 않으면서 통신 품질을 높이는 구조

3. **점진적 채택 가능**
   - ⚡ [v2] `rox` 단일 크레이트 + feature flag로 선택적 기능 활성화
   - ROS 2 브릿지로 기존 생태계에서 점진적 마이그레이션

4. ⚡ **[v2 신규] 결정론성의 2계층 분리**
   - **Hard Deterministic Layer** (rox-core): TaskGraph 노드 실행은 bit-exact 재현 보장
   - **Soft Deterministic Layer** (rox-agent): Agent 판단은 "최종 결정값"만 로그에 기록, 리플레이 시 기록값 주입

5. ⚡ **[v2 신규] 안전은 QM 등급부터, SIL 인증은 별도 로드맵**
   - 초기 버전은 "안전 보조(safety advisory)" 도구로 포지셔닝
   - rox-guard는 별도 프로세스로 격리하여 Freedom from Interference 확보

### 2.2 전체 구조

```
rox (binary: rox-cli)
  └─ Engine
       ├─ rox-protocol           ← 와이어 프로토콜 정의
       ├─ rox-codec              ← 직렬화 (Arrow + 커스텀)
       ├─ rox-buffer             ← zero-copy 버퍼 추상화
       ├─ rox-transport          ← 전송 계층 추상화
       │    ├─ shm::ShmTransport        ← ⚡[v2] iceoryx2 래핑 SHM
       │    ├─ tcp::TcpTransport        ← TCP/TLS
       │    ├─ udp::UdpTransport        ← UDP (⚡[v2] QUIC는 Phase 5로 이동)
       │    └─ serial::SerialTransport  ← 임베디드용
       ├─ rox-core               ← 핵심 런타임
       │    ├─ session::Session         ← zenoh 참조 세션 관리
       │    ├─ node::Node               ← copper 참조 태스크 노드
       │    ├─ topic::Topic             ← Pub/Sub 토픽
       │    ├─ service::Service         ← Req/Rep 서비스
       │    ├─ graph::TaskGraph         ← copper 참조 DAG 스케줄러
       │    ├─ discovery::Discovery     ← 자동 피어 탐색
       │    └─ qos::QoSPolicy          ← QoS 정책 (정적 + 동적)
       ├─ rox-log                ← copper 참조 deterministic 로깅
       ├─ rox-replay             ← copper 참조 비트 단위 리플레이
       ├─ rox-bridge             ← ROS 2 / DDS / Zenoh 브릿지
       ├─ 🧠 rox-agent          ← [핵심 차별화] 지능형 통신 관제
       │    ├─ qos_agent.rs            ← Predictive QoS Agent
       │    ├─ healing_agent.rs        ← Self-healing Agent
       │    ├─ throttle_agent.rs       ← ⚡[v2] 주파수 제한 (Pruning 대체)
       │    ├─ anomaly_agent.rs        ← 이상 탐지 엔진
       │    ├─ ml_runtime.rs           ← Rust-native 경량 추론
       │    └─ rule_engine.rs          ← 조건 → 액션 매핑
       ├─ 🛡️ rox-guard          ← [핵심 차별화] 안전 검증 레이어
       │    ├─ validator.rs            ← zero-copy 명령 검증
       │    ├─ schema.rs               ← Type-safe 명령 스키마
       │    ├─ boundary.rs             ← 물리적 안전 경계 감시
       │    └─ audit.rs                ← 불변 감사 로그
       ├─ rox-cli                ← CLI 도구 (rox new / run / monitor)
       ├─ rox-api                ← REST / gRPC 관리 API
       └─ rox-dashboard          ← 모니터링 웹 UI
```

### 2.3 Rust 크레이트 의존성 매핑

| 경쟁사 참조                | Rox 크레이트            | Rust 의존성                         | 용도                              |
|---------------------------|------------------------|-------------------------------------|-----------------------------------|
| zenoh-transport           | `rox-transport`        | `tokio`, `socket2`                  | [v2] 멀티 트랜스포트 (QUIC는 Phase5) |
| zenoh-buffers/shm         | `rox-buffer`           | `shared_memory`, `memmap2`          | zero-copy 버퍼 + SHM              |
| zenoh-protocol            | `rox-protocol`         | `nom` (파싱), `bytes`               | 와이어 프로토콜 정의               |
| dora: Apache Arrow        | `rox-codec`            | `bincode` 기본, `arrow`(feature)     | [v2] bincode 기본, Arrow 선택적    |
| copper: CopperList        | `rox-core::graph`      | `petgraph`, proc-macro              | Task Graph + 스케줄러 생성         |
| copper: cu29-log          | `rox-log`              | `bincode`, `memmap2`                | Deterministic 로깅                |
| copper: cu29-log-reader   | `rox-replay`           | `rox-log` 의존                      | 비트 단위 리플레이                 |
| ⚡ iceoryx2 (직접 의존)    | `rox-transport::shm`   | **`iceoryx2`** 크레이트 래핑         | [v2] Lock-free SHM IPC            |
| (iceoryx2에 포함)          | `rox-transport` 내부   | iceoryx2-cal 통해 자동              | [v2] OS별 SHM은 iceoryx2가 처리    |
| horus_macros              | `rox-derive`           | `syn`, `quote`, `proc-macro2`       | `#[rox_node]` 매크로               |
| —                         | `rox-agent`            | `ort` (ONNX), `tract`, `ndarray`    | [신규] 경량 ML 추론                |
| —                         | `rox-guard`            | `rox-protocol` 의존                 | [신규] 안전 검증                   |

### 2.4 디렉토리 구조

```
rox/
├── Cargo.toml                    ← workspace 루트
├── rox/                          ← 🔧 [v3] 사용자 대면 통합 크레이트
│   ├── Cargo.toml                ←   features = ["agent", "guard", "bridge-ros2", "shm", "arrow"]
│   └── src/
│       └── lib.rs                ←   pub use rox_core::*; (+ feature-gated re-exports)
├── crates/
│   ├── rox-protocol/             ← 와이어 프로토콜 정의
│   │   └── src/
│   │       ├── message.rs            ← RoxMessage 정의
│   │       ├── header.rs             ← 메시지 헤더 (QoS, 우선순위)
│   │       ├── keyexpr.rs            ← 키 표현식 (zenoh 참조)
│   │       └── wire.rs               ← 와이어 포맷 인코딩
│   ├── rox-codec/                ← 직렬화/역직렬화
│   │   └── src/
│   │       ├── arrow.rs              ← Apache Arrow 직렬화
│   │       ├── bincode.rs            ← 바이너리 직렬화
│   │       └── robotics_types.rs     ← 로보틱스 특화 타입
│   ├── rox-buffer/               ← zero-copy 버퍼
│   │   └── src/
│   │       ├── zbuf.rs               ← ZBuf (zenoh 참조)
│   │       ├── pool.rs               ← Memory Pool (copper 참조)
│   │       └── shm.rs                ← Shared Memory 관리
│   ├── rox-transport/            ← 전송 계층
│   │   └── src/
│   │       ├── lib.rs                ← Transport trait 정의
│   │       ├── shm.rs                ← Lock-free SHM (iceoryx2 참조)
│   │       ├── tcp.rs                ← TCP/TLS 전송
│   │       ├── udp.rs                ← UDP/QUIC 전송
│   │       ├── serial.rs             ← Serial (임베디드)
│   │       └── selector.rs           ← 🧠 Agent 연동 트랜스포트 자동 선택
│   ├── rox-core/                 ← 핵심 런타임
│   │   └── src/
│   │       ├── engine.rs             ← Rox 엔진 (부트스트랩)
│   │       ├── session.rs            ← 세션 관리
│   │       ├── node.rs               ← Node 정의 + lifecycle
│   │       ├── topic.rs              ← Topic (Pub/Sub)
│   │       ├── service.rs            ← Service (Req/Rep)
│   │       ├── graph.rs              ← Task Graph (RON/YAML)
│   │       ├── scheduler.rs          ← 결정론적 스케줄러
│   │       ├── discovery.rs          ← 피어 탐색 (멀티캐스트/유니캐스트)
│   │       ├── qos.rs                ← QoS 정책 관리
│   │       └── config.rs             ← 설정 (serde_yaml)
│   ├── rox-log/                  ← Deterministic 로깅
│   ├── rox-replay/               ← 비트 단위 리플레이
│   ├── rox-bridge/               ← ⚡[v2] zenoh-plugin-ros2dds 래핑
│   │   └── src/
│   │       ├── ros2.rs               ← [v2] Rox ↔ Zenoh ↔ ROS 2 경로
│   │       └── zenoh.rs              ← Zenoh 호환 브릿지
│   ├── rox-agent/                ← 🧠 지능형 통신 관제
│   │   └── src/
│   │       ├── runtime.rs            ← Agent 런타임 (별도 스레드)
│   │       ├── qos_agent.rs          ← Predictive QoS
│   │       ├── healing_agent.rs      ← Self-healing
│   │       ├── throttle_agent.rs     ← 🔧 [v3] 주파수 제한 (Pruning 대체)
│   │       ├── anomaly_agent.rs      ← 이상 탐지
│   │       ├── ml_runtime.rs         ← Rust-native ML 추론
│   │       └── rule_engine.rs        ← 조건 → 액션
│   ├── rox-guard/                ← 🛡️ 안전 검증
│   │   └── src/
│   │       ├── validator.rs          ← 명령 검증기
│   │       ├── schema.rs             ← Type-safe 스키마
│   │       ├── boundary.rs           ← 물리적 안전 경계
│   │       └── audit.rs              ← 감사 로그
│   ├── rox-derive/               ← proc-macro
│   ├── rox-cli/                  ← CLI 도구
│   ├── rox-api/                  ← REST/gRPC API
│   └── rox-dashboard/            ← 웹 모니터링 UI
├── config/
│   └── rox.yml                   ← 기본 설정 파일
├── models/                       ← Agent용 경량 ML 모델
└── examples/
    ├── basic-pubsub/
    ├── multi-robot/
    └── agent-qos-demo/
```

---

## 3. 핵심 컴포넌트 상세 설계

### 3.1 RoxMessage — 프로토콜 독립 메시지

zenoh의 `Sample` + copper의 `CuStampedDataSet` + iceoryx2의 `Sample`을 통합 참조.

```rust
/// Rox의 기본 메시지 단위
pub struct RoxMessage {
    pub header: MessageHeader,
    pub payload: Bytes,            // 🔧 [v3] Bytes 단일 타입으로 통일
}

pub struct MessageHeader {
    pub key: KeyExpr,              // zenoh 참조: 계층적 키 ("robot/lidar/scan")
    pub timestamp: RoxTimestamp,   // copper 참조: 결정론적 타임스탬프
    pub qos: QoSMetadata,         // 우선순위, 신뢰성, 만료 시간
    pub source_id: NodeId,         // 발행자 식별
    pub sequence: u64,             // 순서 번호 (리플레이용)
}

pub struct RoxTimestamp {
    pub hw_time: u64,             // 하드웨어 클럭 (나노초)
    pub logical_time: u64,        // 논리 클럭 (HLC, zenoh 참조)
    pub tov: TimeOfValidity,      // copper 참조: 데이터 유효 시간 범위
}

// 🔧 [v3] Phase 4 이후 feature flag로 확장 가능한 페이로드 타입
// #[cfg(feature = "shm")]
// pub struct ShmPayload { handle: ShmHandle, offset: usize, len: usize }
// #[cfg(feature = "arrow")]
// pub struct ArrowPayload(arrow::array::ArrayRef)
//
// 확장 시 RoxMessage.payload를 enum으로 전환:
// pub enum RoxPayload { Inline(Bytes), Shm(ShmPayload), Arrow(ArrowPayload) }

pub struct QoSMetadata {
    pub priority: Priority,        // 0(최고) ~ 7(최저), 8단계
    pub reliability: Reliability,  // BestEffort / Reliable
    pub durability: Durability,    // Volatile / TransientLocal
    pub deadline_us: Option<u64>,  // 마이크로초 단위 데드라인
    pub lifespan_us: Option<u64>,  // 데이터 만료 시간
}

/// zenoh 참조: 와일드카드 지원 키 표현식
/// 🔧 [v3] 멀티 로봇 네임스페이스 강제 규칙:
///   형식: "{robot_id}/{subsystem}/{topic}"
///   예시: "robot-01/lidar/scan", "robot-02/motor/cmd"
///   글로벌: "_global/{topic}" (로봇 ID 불필요한 공유 토픽)
pub struct KeyExpr(String);

impl KeyExpr {
    /// 🔧 [v3] 네임스페이스 규칙을 강제하는 생성자
    pub fn new(robot_id: &str, subsystem: &str, topic: &str) -> Self {
        Self(format!("{}/{}/{}", robot_id, subsystem, topic))
    }

    /// 글로벌 토픽 (로봇 ID 불필요)
    pub fn global(topic: &str) -> Self {
        Self(format!("_global/{}", topic))
    }

    /// 로봇 ID 추출 (멀티 로봇 환경에서 충돌 방지)
    pub fn robot_id(&self) -> Option<&str> {
        let first = self.0.split('/').next()?;
        if first == "_global" { None } else { Some(first) }
    }

    /// 와일드카드 매칭: "robot-01/*/scan" 패턴
    pub fn matches(&self, pattern: &str) -> bool { /* zenoh KeyExpr 호환 매칭 */ todo!() }
}
```

**경쟁사 대비 개선점:**
- zenoh: `QoSMetadata`를 메시지 헤더에 내장 (zenoh는 transport 수준에서만 관리)
- 🔧 [v3] 초기 버전은 `Bytes` 단일 페이로드로 복잡도 최소화. SHM/Arrow는 Phase 4에서 feature flag로 확장
- dora-rs: `RoxTimestamp`에 하드웨어 클럭 + 논리 클럭 + TOV 통합 (dora는 타임스탬프 관리 약함)

### 3.2 Transport — 멀티 트랜스포트 추상화

zenoh의 `zenoh-links` + iceoryx2의 `zero_copy_connection` + HORUS의 `Hub` 자동 전환.

```rust
/// 모든 트랜스포트가 구현하는 trait
/// zenoh의 LinkUnicast/LinkMulticast trait 참조
#[async_trait]
pub trait Transport: Send + Sync + 'static {
    /// 메시지 전송 (zero-copy 가능)
    async fn send(&self, msg: &RoxMessage) -> Result<()>;

    /// 메시지 수신
    async fn recv(&self) -> Result<RoxMessage>;

    /// 트랜스포트 종류 식별
    fn kind(&self) -> TransportKind;

    /// 현재 지연 시간 추정 (Agent가 참조)
    fn estimated_latency_us(&self) -> u64;

    /// 현재 대역폭 사용률 (Agent가 참조)
    fn bandwidth_usage(&self) -> f32;
}

pub enum TransportKind {
    SharedMemory,  // 같은 머신: lock-free SHM (iceoryx2 참조)
    Tcp,           // 다른 머신: TCP/TLS
    Udp,           // 저지연 필요: UDP
    Quic,          // 보안 + 멀티플렉싱: QUIC
    Serial,        // 임베디드: UART/SPI
}

/// 🧠 Agent 연동 트랜스포트 선택기
/// HORUS의 Hub 자동 전환을 확장하여, Agent가 실시간으로 트랜스포트 변경
pub struct TransportSelector {
    transports: Vec<Box<dyn Transport>>,
    agent_hint: Option<AgentTransportHint>,
}

impl TransportSelector {
    /// 기본: 같은 머신이면 SHM, 다른 머신이면 TCP
    /// Agent 활성화 시: 네트워크 상태에 따라 동적 전환
    pub async fn select(&self, target: &NodeId) -> &dyn Transport {
        if let Some(hint) = &self.agent_hint {
            return self.transports.iter()
                .find(|t| t.kind() == hint.recommended)
                .unwrap_or(&self.transports[0]);
        }
        if self.is_local(target) {
            self.get_transport(TransportKind::SharedMemory)
        } else {
            self.get_transport(TransportKind::Tcp)
        }
    }

    /// 🔧 [v3 신규] Transport 전환 프로토콜 — 메시지 유실 방지 4단계
    ///
    /// Agent가 SuggestTransport를 보내면 이 메서드가 호출된다.
    /// 전환 중 in-flight 메시지를 보존하여 데이터 손실을 방지한다.
    pub async fn switch_transport(
        &mut self,
        target: &NodeId,
        from: TransportKind,
        to: TransportKind,
    ) -> Result<TransportSwitchReport> {
        let mut report = TransportSwitchReport::default();

        // Phase 1: DRAIN — 현재 트랜스포트의 전송 큐를 비움
        let old = self.get_transport_mut(from);
        let drained = old.drain_pending().await;
        report.drained_count = drained.len();

        // Phase 2: BUFFER — drain된 메시지를 임시 버퍼에 저장
        let mut transition_buffer: VecDeque<RoxMessage> = drained.into();

        // Phase 3: SWITCH — 새 트랜스포트 활성화
        let new_transport = self.get_transport(to);
        if !new_transport.is_ready().await {
            // 새 트랜스포트 준비 실패 → 원래 트랜스포트 유지, Agent에 실패 보고
            report.success = false;
            report.reason = Some("New transport not ready".into());
            return Ok(report);
        }
        self.active_transport.insert(target.clone(), to);

        // Phase 4: REPLAY — 버퍼의 메시지를 새 트랜스포트로 재전송
        while let Some(msg) = transition_buffer.pop_front() {
            new_transport.send(&msg).await?;
            report.replayed_count += 1;
        }

        report.success = true;
        report.switch_duration_us = report.start.elapsed().as_micros() as u64;
        Ok(report)
    }
}

pub struct TransportSwitchReport {
    pub success: bool,
    pub drained_count: usize,
    pub replayed_count: usize,
    pub switch_duration_us: u64,
    pub reason: Option<String>,
    start: Instant,
}
```

**SHM Transport — ⚡ [v2 변경] iceoryx2 래핑:**
```rust
/// [v2] iceoryx2를 직접 의존하여 래핑 (자체 lock-free SHM 구현 대신)
/// iceoryx2 팀은 8년간 lock-free SHM을 최적화하여 ~100ns 지연 달성.
/// Phase 4에서 래핑 오버헤드가 >2μs일 경우에만 자체 구현 검토.
pub struct Iceoryx2ShmTransport {
    node: iceoryx2::node::Node<iceoryx2::service::ipc::Service>,
    publishers: HashMap<KeyExpr, Iceoryx2Publisher>,
    subscribers: HashMap<KeyExpr, Iceoryx2Subscriber>,
}

impl Transport for Iceoryx2ShmTransport {
    async fn send(&self, msg: &RoxMessage) -> Result<()> {
        // RoxMessage → iceoryx2 Sample (loan → write → send, zero-copy)
    }
    async fn recv(&self) -> Result<RoxMessage> {
        // iceoryx2 Sample → RoxMessage 변환
    }
    fn kind(&self) -> TransportKind { TransportKind::SharedMemory }
    fn estimated_latency_us(&self) -> u64 { 1 } // ~1μs (래핑 오버헤드 포함)
    fn bandwidth_usage(&self) -> f32 { /* iceoryx2 메트릭 조회 */ 0.0 }
}
```

### 3.3 Node — 태스크 노드

copper의 `CuTask` trait + HORUS의 `Node` trait + dora의 데이터플로우 노드.

```rust
/// Rox 노드의 생명주기 trait
/// copper의 CuTask + CuSrcTask + CuSinkTask를 통합
#[async_trait]
pub trait RoxNode: Send + Sync + 'static {
    /// 노드 이름
    fn name(&self) -> &str;

    /// 초기화 (설정 로드, 리소스 할당)
    async fn init(&mut self, ctx: &mut NodeContext) -> Result<()>;

    /// 주기적 실행 (copper의 process 메서드 참조)
    /// copper: 입력 → 처리 → 출력의 결정론적 파이프라인
    async fn tick(&mut self, ctx: &mut NodeContext) -> Result<()>;

    /// 종료 (리소스 해제)
    async fn shutdown(&mut self, ctx: &mut NodeContext) -> Result<()>;
}

/// copper의 CuSrcTask 참조: 센서/드라이버 노드
#[async_trait]
pub trait RoxSourceNode: RoxNode {
    type Output: RoxSerialize;
    async fn produce(&mut self, ctx: &mut NodeContext) -> Result<Self::Output>;
}

/// copper의 CuSinkTask 참조: 액추에이터 노드
/// 🛡️ rox-guard 연동: 명령이 Guard를 통과해야 액추에이터에 전달
#[async_trait]
pub trait RoxSinkNode: RoxNode {
    type Input: RoxSerialize;
    async fn consume(&mut self, input: Self::Input, ctx: &mut NodeContext) -> Result<()>;
}

/// 노드가 사용하는 컨텍스트
/// dora의 Node API + copper의 CuRuntime 참조
pub struct NodeContext {
    pub publishers: HashMap<String, Publisher>,
    pub subscribers: HashMap<String, Subscriber>,
    pub services: HashMap<String, ServiceClient>,
    pub clock: RoxClock,           // 결정론적 클럭 (리플레이 시 고정)
    pub logger: RoxLogger,         // 구조화 로거
    pub memory_pool: MemoryPool,   // SHM 메모리 풀 접근
}

/// ⚡ [v2 신규] RoxClock — 리플레이 모드 명시적 설계
pub enum RoxClock {
    /// 실제 실행 모드: 하드웨어 클럭 사용
    RealTime,
    /// 리플레이 모드: 로그에서 타임스탬프 주입 (ML 재실행 안 함)
    Replay { log_reader: LogReader, cycle: u64 },
}
```

**Proc-macro로 선언적 노드 정의 (HORUS의 node! 참조):**
```rust
/// HORUS의 node! 매크로를 참조하되,
/// Guard 연동과 Agent 메트릭 내보내기를 추가
#[rox_node]
struct LidarProcessor {
    #[rox_sub("lidar/raw")]
    input: PointCloud,

    #[rox_pub("lidar/filtered")]
    output: PointCloud,

    #[rox_param(default = 0.1)]
    voxel_size: f32,
}

impl RoxNode for LidarProcessor {
    async fn tick(&mut self, ctx: &mut NodeContext) -> Result<()> {
        if let Some(cloud) = ctx.subscribers["lidar/raw"].try_recv()? {
            let filtered = voxel_filter(&cloud, self.voxel_size);
            ctx.publishers["lidar/filtered"].publish(filtered).await?;
        }
        Ok(())
    }
    // init, shutdown은 기본 구현 사용
}
```

### 3.4 TaskGraph — 결정론적 스케줄러

copper의 RON 기반 Task Graph를 참조하되, Agent 연동을 추가.

#### 🔧 [v3 신규] PolicyUpdate 적용 시점 — Cycle Boundary Protocol

Agent의 `VersionedPolicyUpdate.apply_at_cycle`이 스케줄러와 정확히 동기화되는 방법:

```rust
impl Scheduler {
    pub async fn run_cycle(&mut self, cycle: u64) {
        // ──── Cycle Boundary (정책 적용 지점) ────
        // Step 1: Agent policy queue를 drain
        while let Ok(update) = self.policy_rx.try_recv() {
            if update.apply_at_cycle <= cycle {
                // 이 사이클부터 적용
                self.apply_policy(&update);
                self.log.record_policy_applied(cycle, &update);
            } else {
                // 아직 적용 시점이 아님 → 다시 큐에 넣음
                self.policy_pending.push(update);
            }
        }

        // Step 2: pending 중 이번 사이클에 해당하는 것 적용
        self.policy_pending.retain(|u| {
            if u.apply_at_cycle <= cycle {
                self.apply_policy(u);
                self.log.record_policy_applied(cycle, u);
                false // retain에서 제거
            } else {
                true  // 유지
            }
        });

        // ──── Hard Deterministic Execution ────
        // Step 3: TaskGraph의 노드를 순차 실행 (copper CopperList 참조)
        for node in &self.execution_order {
            node.tick(&mut self.context).await;
        }

        // Step 4: 이번 사이클 결과를 로그에 기록
        self.log.record_cycle_complete(cycle);
    }
}
```

**리플레이 시:** `rox-replay`가 로그에서 `record_policy_applied(cycle, update)`를 읽어
동일 사이클에 동일 정책을 주입. ML 추론을 재실행하지 않고 기록된 결정값 사용.

```rust
/// copper의 Task Graph 설정을 참조
/// RON + YAML 듀얼 지원
#[derive(Deserialize)]
pub struct TaskGraphConfig {
    pub nodes: Vec<NodeConfig>,
    pub connections: Vec<ConnectionConfig>,
    pub agent: Option<AgentGraphConfig>,  // 🧠 Agent 설정
    pub guard: Option<GuardGraphConfig>,  // 🛡️ Guard 설정
}

#[derive(Deserialize)]
pub struct NodeConfig {
    pub id: String,
    pub node_type: String,             // Rust 타입 경로
    pub rate_hz: Option<f64>,          // 실행 주기 (Hz)
    pub priority: u8,                  // 스케줄링 우선순위
    pub background: bool,              // copper 참조: 비동기 배경 태스크
}

#[derive(Deserialize)]
pub struct ConnectionConfig {
    pub from: String,                  // "node_id/output_name"
    pub to: String,                    // "node_id/input_name"
    pub qos: Option<QoSConfig>,        // 커넥션별 QoS 오버라이드
}

/// 🧠 Task Graph에 Agent가 개입하는 지점
#[derive(Deserialize)]
pub struct AgentGraphConfig {
    pub enabled: bool,
    pub qos_prediction: bool,          // QoS 예측 활성화
    pub anomaly_detection: bool,       // 이상 탐지 활성화
    pub healing: bool,                 // 자가 치유 활성화
    pub throttling: Option<ThrottlingConfig>, // 🔧 [v3] 주파수 제한 (Pruning 대체)
}
```

**설정 파일 예시 (rox.yml):**

```yaml
# Rox 설정 파일
transport:
  shm:
    enabled: true
    pool_size_mb: 256
  tcp:
    bind: "0.0.0.0:7447"
    tls: false
  quic:
    bind: "0.0.0.0:7448"

discovery:
  mode: static              # ⚡[v2] Phase 1: static만. multicast는 Phase 5
  peers:
    - "192.168.1.101:7447"
    - "192.168.1.102:7447"

nodes:
  - id: lidar_driver
    type: "rox_drivers::VelodyneLidar"
    rate_hz: 10.0
    priority: 0             # 최고 우선순위
    config:
      ip: "192.168.1.201"
      port: 2368

  - id: lidar_filter
    type: "rox_perception::VoxelFilter"
    rate_hz: 10.0
    priority: 1
    config:
      voxel_size: 0.1

  - id: path_planner
    type: "rox_planning::AStarPlanner"
    rate_hz: 5.0
    priority: 2

  - id: motor_controller
    type: "rox_drivers::CanMotor"
    rate_hz: 100.0
    priority: 0             # 모터 제어도 최고 우선순위

connections:
  - from: "lidar_driver/scan"
    to: "lidar_filter/input"
    qos:
      reliability: reliable
      priority: 1

  - from: "lidar_filter/output"
    to: "path_planner/pointcloud"

  - from: "path_planner/velocity_cmd"
    to: "motor_controller/command"
    qos:
      reliability: reliable
      deadline_us: 10000    # 10ms 데드라인

# 🧠 Agent 설정 — 통신 인프라만 관제, 제어 루프 미개입
agent:
  enabled: true
  # ⚡[v2] 콜드 스타트: Observer 모드로 시작
  start_mode:
    type: observer
    # 🔧 [v3] 복합 전환 조건 (시간 + 데이터 품질)
    min_observation_hours: 24
    min_data_points: 100000
    min_topic_coverage: 0.8     # 80% 토픽에서 메트릭 수집
    max_anomaly_ratio: 0.05     # 이상 탐지 비율 5% 이하
  qos_prediction:
    enabled: true
    level: 0                    # ⚡[v2] 0=규칙, 1=자기학습, 2=사전학습
    interval_ms: 100
  anomaly_detection:
    enabled: true
    jitter_threshold_us: 500
    packet_loss_threshold: 0.01
  healing:
    enabled: true
    failover_timeout_ms: 50
  # ⚡[v2] Semantic Pruning 제거 → frequency throttling
  throttling:
    enabled: false

# 🛡️ Guard 설정 — 안전 경계 감시
guard:
  enabled: true
  # ⚡[v2] 안전 등급 명시
  safety_level: qm             # qm(초기) | sil1 | sil2 (별도 인증 필요)
  # ⚡[v2] 별도 프로세스로 실행
  process_isolation: true
  # 🔧 [v3] Guard Supervisor (자동 재시작)
  supervisor:
    max_restart_attempts: 3
    restart_cooldown_sec: 5
    interim_policy: block_all_commands   # 재시작 중 적용
  # ⚡[v2] Watchdog 설정
  watchdog:
    heartbeat_interval_ms: 100
    timeout_ms: 500
  # ⚡[v2] Fail-safe 기본 동작
  failsafe:
    on_guard_failure: block_all_commands
    on_validation_timeout: reject
  boundaries:
    - node: "motor_controller"
      max_velocity: 2.0       # m/s
      max_acceleration: 5.0   # m/s²
      geofence:               # 물리적 작동 영역
        min_x: -10.0
        max_x: 10.0
        min_y: -10.0
        max_y: 10.0
  audit_log:
    enabled: true
    path: "./logs/audit/"
    rotation_mb: 100
```

### 3.5 Config — 핫 리로드

```rust
/// zenoh의 Config + copper의 RON Config를 통합
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

/// zenoh의 confWatcher 참조: notify 크레이트 기반
pub struct ConfigWatcher {
    path: PathBuf,
    watcher: notify::RecommendedWatcher,
    tx: mpsc::Sender<RoxConfig>,
}

impl ConfigWatcher {
    /// 설정 파일 변경 감지 → 파싱 → 런타임에 전파
    /// 변경 불가 항목 (transport.shm.pool_size 등)은 재시작 필요 경고
    pub async fn watch(&mut self) -> Result<()> {
        // notify 이벤트 → serde_yaml 파싱 → diff → 적용
    }
}
```

---

## 4. 🧠 AI Agent 레이어 설계

경쟁사에 없는, Rox의 핵심 차별점.
**원칙: Agent는 데이터 경로(hot path) 밖에서 동작하며, 통신 메타데이터만 관리한다.**

⚡ **[v2 변경] Agent는 별도 프로세스에서 실행. ML 모델은 3단계 진화. 콜드 스타트 모드 추가.**

### Agent ML 진화 3단계

| Level | 방식 | 시점 | 설명 |
|-------|------|------|------|
| **Level 0** | 규칙 기반 | Phase 2 (출시 시) | ML 없음. 통계 임계치만 사용 (jitter > 500μs면 경고) |
| **Level 1** | 자기 학습 | Phase 3 | 배포 후 자체 이력으로 온라인 학습 (AutoEncoder/Isolation Forest) |
| **Level 2** | 사전 학습 | Phase 6+ | 충분한 데이터 축적 후 ONNX 모델 배포 |

### Agent 콜드 스타트 모드

| 모드 | 동작 | 전환 조건 |
|------|------|----------|
| **Observer** | 데이터 수집만, 정책 변경 없음 | 설정된 시간(기본 24h) 경과 |
| **Suggestion** | 정책 변경 제안, 관리자 승인 필요 | 관리자가 Autonomous 전환 승인 |
| **Autonomous** | 자동 정책 변경 + 성능 악화 시 자동 롤백 | — |

### 4.1 Agent 아키텍처 — 제어 루프 밖의 관제탑

```
                   ┌─────────────────────────────────┐
                   │          rox-core 런타임          │
                   │                                   │
  [Sensor Node]───▶│  Topic ──▶ Transport ──▶ Topic   │──▶ [Actuator Node]
                   │    │                       │      │
                   │    │     hot path (μs)      │      │
                   └────┼───────────────────────┼──────┘
                        │                       │
                   메타데이터 수집          정책 적용
                        │                       │
                   ┌────▼───────────────────────▼──────┐
                   │         🧠 rox-agent 런타임        │
                   │         (별도 스레드, 낮은 우선순위)   │
                   │                                     │
                   │  ┌─────────────┐ ┌──────────────┐  │
                   │  │ QoS Agent   │ │ Healing Agent│  │
                   │  │ (트래픽 예측)│ │ (장애 복구)   │  │
                   │  └──────┬──────┘ └──────┬───────┘  │
                   │         │               │           │
                   │  ┌──────▼──────┐ ┌──────▼───────┐  │
                   │  │ Anomaly Det │ │Throttle Agent│  │
                   │  │ (이상 탐지)  │ │ (주파수 제한) │  │  🔧 [v3] Pruning→Throttle
                   │  └──────┬──────┘ └──────┬───────┘  │
                   │         │               │           │
                   │         ▼               ▼           │
                   │    ┌──────────────────────────┐    │
                   │    │     Rule Engine           │    │
                   │    │  조건 → 정책 변경/경고     │    │
                   │    └──────────┬───────────────┘    │
                   └───────────────┼─────────────────────┘
                                   │
                         ┌─────────┼─────────┐
                         ▼         ▼         ▼
                   QoS 정책변경  경고 발송  우회경로 생성
                   (rox-core에   (Webhook)  (Transport
                    피드백)                  Selector에)
```

### 4.2 AgentRuntime — Agent 스케줄러

```rust
/// Agent 런타임: rox-core와 별도 스레드에서 실행
/// 낮은 우선순위로 실행되어 실시간 태스크에 영향 없음
pub struct AgentRuntime {
    config: AgentGraphConfig,
    metrics_rx: ShmReceiver<TransportMetrics>,           // [v2] SHM 기반 프로세스 간 통신
    policy_tx: ShmSender<VersionedPolicyUpdate>,         // [v2] 버저닝된 정책 피드백

    /// [v2 신규] 현재 Agent 모드 (Observer → Suggestion → Autonomous)
    mode: AgentStartMode,
    /// [v2 신규] ML 진화 레벨 (Level 0=규칙 → 1=자기학습 → 2=사전학습)
    ml_level: MlLevel,
    /// [v2 신규] 정책 이력 (자동 롤백용)
    policy_history: VecDeque<PolicySnapshot>,

    qos_agent: Option<QoSPredictionAgent>,
    healing_agent: Option<SelfHealingAgent>,
    anomaly_agent: Option<AnomalyDetectionAgent>,
    throttle_agent: Option<ThrottleAgent>,               // [v2] Pruning 대체

    rule_engine: RuleEngine,
}

/// ⚡ [v2 신규] 콜드 스타트 모드
pub enum AgentStartMode {
    Observer { duration_hours: u32 },
    Suggestion,
    Autonomous,
}

/// ⚡ [v2 신규] ML 진화 레벨
pub enum MlLevel {
    RuleBased,                                            // Level 0
    SelfLearning { model: OnlineModel, data_points: u64 },// Level 1
    PreTrained { model: tract::SimplePlan<...> },         // Level 2
}

/// rox-core의 Transport가 Agent에 보내는 메트릭
/// 이것은 hot path에서 lock-free로 수집됨
pub struct TransportMetrics {
    pub timestamp: u64,
    pub topic: KeyExpr,
    pub latency_us: u64,           // 전송 지연
    pub jitter_us: u64,            // jitter (지연 변동)
    pub throughput_bps: u64,       // 처리량
    pub packet_loss_ratio: f32,    // 패킷 손실률
    pub queue_depth: u32,          // 큐 깊이
    pub transport_kind: TransportKind,
}

/// ⚡ [v2 변경] 버저닝된 정책 업데이트 — 결정론적 리플레이 보장
pub struct VersionedPolicyUpdate {
    pub version: u64,
    pub apply_at_cycle: u64,       // 이 스케줄링 사이클부터 적용 (Hard Layer 경계)
    pub confidence: f32,           // Agent 확신도 (0.0~1.0)
    pub rollback_snapshot: Option<PolicySnapshot>, // 이전 정책 (자동 롤백용)
    pub update: PolicyUpdateType,
}

pub enum PolicyUpdateType {
    /// QoS 우선순위 변경
    UpdateQoS { topic: KeyExpr, new_priority: Priority },

    /// 트랜스포트 전환 추천
    SuggestTransport { target: NodeId, recommended: TransportKind },

    /// 토픽 데이터 간격 조정 (throttling)
    ThrottleTopic { topic: KeyExpr, new_interval_ms: u64 },

    /// 경고 이벤트 발생
    Alert { level: AlertLevel, message: String },

    /// 우회 경로 생성 (healing)
    CreateFailover { failed_node: NodeId, backup_route: Vec<NodeId> },
}

impl AgentRuntime {
    pub async fn run(&mut self) {
        loop {
            // 1. rox-core에서 메트릭 수집 (non-blocking)
            while let Ok(metrics) = self.metrics_rx.try_recv() {
                // 2. 각 Agent에 메트릭 전달
                if let Some(qos) = &mut self.qos_agent {
                    qos.observe(&metrics);
                }
                if let Some(anomaly) = &mut self.anomaly_agent {
                    anomaly.observe(&metrics);
                }
            }

            // 3. 주기적 Agent 실행
            let mut events = vec![];

            if let Some(qos) = &mut self.qos_agent {
                events.extend(qos.evaluate().await);
            }
            if let Some(healing) = &mut self.healing_agent {
                events.extend(healing.evaluate().await);
            }
            if let Some(anomaly) = &mut self.anomaly_agent {
                events.extend(anomaly.evaluate().await);
            }

            // 4. 룰 엔진 평가 → [v2] 모드에 따라 다른 동작
            for event in events {
                if let Some(update) = self.rule_engine.evaluate(&event) {
                    match &self.mode {
                        AgentStartMode::Observer { .. } => {
                            // 관찰만: 로그에 기록, 정책 변경 없음
                            log::info!("Observer mode: would apply {:?}", update);
                        }
                        AgentStartMode::Suggestion => {
                            // 제안만: 대시보드에 표시, 관리자 승인 대기
                        }
                        AgentStartMode::Autonomous => {
                            // 자동 적용 + [v2] 버저닝 + 롤백 준비
                            let versioned = VersionedPolicyUpdate {
                                version: self.next_version(),
                                apply_at_cycle: self.current_cycle + 1,
                                confidence: 0.0, // Agent가 계산
                                rollback_snapshot: Some(self.current_snapshot()),
                                update,
                            };
                            let _ = self.policy_tx.send(versioned).await;
                        }
                    }
                }
            }

            // 5. Agent 주기 (기본 100ms — 실시간 태스크에 영향 없음)
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// 🔧 [v3 신규] 정책 적용 후 자동 롤백 판단
    ///
    /// 정량 기준:
    /// - 적용 후 5초간 평균 latency가 적용 전 5초 대비 20% 이상 증가 → 롤백
    /// - 적용 후 5초간 패킷 손실률이 적용 전 대비 2배 이상 증가 → 롤백
    /// - 적용 후 5초간 jitter 분산이 적용 전 대비 50% 이상 증가 → 롤백
    async fn check_and_rollback(&mut self, applied: &VersionedPolicyUpdate) {
        tokio::time::sleep(Duration::from_secs(5)).await;

        let before = self.metrics_snapshot_before_policy.as_ref().unwrap();
        let after = self.collect_metrics_snapshot();

        let latency_increase = (after.avg_latency - before.avg_latency) as f64 / before.avg_latency as f64;
        let loss_increase = after.packet_loss / before.packet_loss.max(0.001);
        let jitter_increase = (after.jitter_variance - before.jitter_variance) / before.jitter_variance.max(0.001);

        let should_rollback = latency_increase > 0.20
            || loss_increase > 2.0
            || jitter_increase > 0.50;

        if should_rollback {
            log::warn!(
                "Policy v{} caused degradation (latency +{:.0}%, loss x{:.1}, jitter +{:.0}%). Rolling back.",
                applied.version, latency_increase * 100.0, loss_increase, jitter_increase * 100.0
            );

            if let Some(snapshot) = &applied.rollback_snapshot {
                let rollback = VersionedPolicyUpdate {
                    version: self.next_version(),
                    apply_at_cycle: self.current_cycle + 1,
                    confidence: 1.0, // 롤백은 확실한 판단
                    rollback_snapshot: None,
                    update: snapshot.to_policy_update(),
                };
                let _ = self.policy_tx.send(rollback).await;
            }
        }
    }
}

/// 🔧 [v3 신규] Observer → Suggestion → Autonomous 전환 조건 복합화
///
/// v2에서는 시간(24h)만 기준이었으나, v3에서는 데이터 품질도 검증한다.
pub struct ModeTransitionCriteria {
    /// 최소 관찰 시간 (기본 24시간)
    pub min_observation_hours: u32,

    /// 최소 수집 데이터 포인트 수 (기본 100,000)
    pub min_data_points: u64,

    /// 최소 토픽 커버리지 (전체 토픽 중 메트릭이 수집된 비율, 기본 80%)
    pub min_topic_coverage: f32,

    /// 최소 안정성 (관찰 기간 중 이상 탐지 비율이 이 값 이하여야 함, 기본 5%)
    pub max_anomaly_ratio: f32,
}

impl ModeTransitionCriteria {
    /// Observer → Suggestion 전환 가능 여부 판단
    pub fn can_transition(&self, stats: &ObservationStats) -> bool {
        stats.observation_hours >= self.min_observation_hours
            && stats.total_data_points >= self.min_data_points
            && stats.topic_coverage >= self.min_topic_coverage
            && stats.anomaly_ratio <= self.max_anomaly_ratio
    }
}
```

### 4.3 QoS Prediction Agent — 핵심 차별화

```rust
/// 트래픽 패턴을 학습하여 병목을 예측하고 QoS를 선제 조정
/// zenoh에 없는 Rox만의 기능
pub struct QoSPredictionAgent {
    /// [v2] ML 모델은 Level에 따라 다르게 동작 (Level 0에서는 미사용)
    /// Level 0: 규칙 기반 (통계 임계치)
    /// Level 1: 자기학습 (온라인 AutoEncoder)
    /// Level 2: 사전학습 (ONNX 모델)

    history: VecDeque<TransportMetrics>,
    window_size: usize,
    predictions: HashMap<KeyExpr, QoSPrediction>,

    /// [v2 신규] 통계 기반 임계치 (Level 0용)
    latency_threshold_us: u64,
    jitter_threshold_us: u64,
}

pub struct QoSPrediction {
    pub congestion_probability: f32,   // 0.0 ~ 1.0
    pub recommended_priority: Priority,
    pub predicted_latency_us: u64,
}

impl QoSPredictionAgent {
    pub fn observe(&mut self, metrics: &TransportMetrics) {
        self.history.push_back(metrics.clone());
        if self.history.len() > self.window_size {
            self.history.pop_front();
        }
    }

    /// [v2] ML Level에 따라 다른 평가 방법
    pub async fn evaluate(&mut self, ml_level: &MlLevel) -> Vec<AgentEvent> {
        match ml_level {
            MlLevel::RuleBased => self.evaluate_rule_based(),
            MlLevel::SelfLearning { model, .. } => self.evaluate_self_learning(model),
            MlLevel::PreTrained { model } => self.evaluate_pretrained(model),
        }
    }

    /// Level 0: 단순 통계 임계치 (ML 없음)
    fn evaluate_rule_based(&self) -> Vec<AgentEvent> {
        let mut events = vec![];
        // 최근 메트릭의 평균 jitter/latency가 임계치 초과 시 경고
        for (topic, window) in self.group_by_topic() {
            let avg_latency = window.iter().map(|m| m.latency_us).sum::<u64>() / window.len() as u64;
            if avg_latency > self.latency_threshold_us * 80 / 100 {
                events.push(AgentEvent::CongestionWarning {
                    topic: topic.clone(),
                    probability: avg_latency as f32 / self.latency_threshold_us as f32,
                    recommended_priority: Priority::High,
                });
            }
        }
        events
    }
}
```

### 4.4 Self-healing Agent

```rust
/// 통신 노드 장애 징후를 감지하여 자동 우회 경로 생성
/// copper-rs에 없는 Rox만의 기능
pub struct SelfHealingAgent {
    /// 노드별 헬스 상태
    node_health: HashMap<NodeId, NodeHealthState>,

    /// 네트워크 토폴로지 (우회 경로 계산용)
    topology: petgraph::Graph<NodeId, ConnectionInfo>,

    /// 장애 판정 타임아웃
    failover_timeout: Duration,
}

pub struct NodeHealthState {
    pub last_seen: Instant,
    pub consecutive_failures: u32,
    pub avg_latency_us: f64,
    pub status: HealthStatus,
}

pub enum HealthStatus {
    Healthy,
    Degraded,      // 성능 저하 감지
    Suspected,     // 장애 의심
    Failed,        // 장애 확정 → 우회 경로 활성화
}

impl SelfHealingAgent {
    pub async fn evaluate(&mut self) -> Vec<AgentEvent> {
        let mut events = vec![];

        for (node_id, health) in &mut self.node_health {
            match health.status {
                HealthStatus::Suspected => {
                    if health.last_seen.elapsed() > self.failover_timeout {
                        health.status = HealthStatus::Failed;

                        // petgraph로 대체 경로 계산
                        if let Some(backup) = self.find_backup_route(node_id) {
                            events.push(AgentEvent::FailoverActivated {
                                failed_node: node_id.clone(),
                                backup_route: backup,
                            });
                        }
                    }
                }
                HealthStatus::Healthy => {
                    // 최근 연속 실패가 임계치 초과 → Degraded로 전환
                    if health.consecutive_failures > 3 {
                        health.status = HealthStatus::Degraded;
                        events.push(AgentEvent::NodeDegraded {
                            node_id: node_id.clone(),
                        });
                    }
                }
                _ => {}
            }
        }

        events
    }
}
```

---

## 5. 🛡️ Guard 레이어 설계

⚡ **[v2 전면 개편] 별도 프로세스 격리 + Watchdog + Fail-safe + 안전 등급 명시**

**안전 등급: QM (Quality Management)**
> 초기 버전은 IEC 61508 SIL 인증 대상이 아닌 "안전 보조 도구"로 포지셔닝.
> SIL 1/2 인증은 v2.0 이후 별도 로드맵으로 추진.
> 마케팅에서 "인증된 안전 기능"이라고 주장하지 않는다.

### 5.1 Guard 프로세스 아키텍처 — ⚡ [v2] 별도 프로세스

```rust
/// [v2] Guard는 별도 프로세스에서 실행 (Freedom from Interference)
/// rox-core, rox-agent와 메모리 공간 완전 격리
pub struct GuardProcess {
    validator: CommandValidator,
    watchdog: GuardWatchdog,
    failsafe: FailsafePolicy,
    audit_logger: AuditLogger,
    /// rox-core와 SHM으로 통신
    request_rx: ShmReceiver<ValidationRequest>,
    response_tx: ShmSender<ValidationResponse>,
}

/// [v2 신규] Watchdog — Guard 프로세스 생존 감시
pub struct GuardWatchdog {
    heartbeat_interval: Duration,  // 100ms
    timeout: Duration,             // 500ms
    last_heartbeat: Instant,
}

/// [v2 신규] Fail-safe 정책 — Guard 실패 시 기본 동작
pub enum FailsafePolicy {
    BlockAllCommands,              // 모든 actuator 명령 차단 (가장 안전)
    RepeatLastSafe { last_safe: Option<RoxMessage> }, // 최종 안전 명령 반복
    EmergencyStop,                 // 긴급 정지 신호
}

/// AI/외부 시스템의 명령을 메모리 복사 없이 실시간 검증
pub struct CommandValidator {
    schemas: HashMap<String, CommandSchema>,
    boundaries: Vec<SafetyBoundary>,
}

pub struct CommandSchema {
    pub velocity_range: Range<f64>,
    pub acceleration_range: Range<f64>,
    pub torque_range: Range<f64>,
    pub valid_zones: Vec<GeoFence>,
    /// [v2 신규] WCET 사양 — 측정 환경: ARM Cortex-A72, PREEMPT_RT
    pub wcet_us: u64,              // 예: 10μs (99.99th percentile)
}

pub enum ValidationResult {
    /// 명령 통과
    Passed,
    /// 명령 수정 후 통과 (클램핑)
    Clamped { original: f64, clamped: f64, field: String },
    /// 명령 거부
    Rejected { reason: String },
    /// 긴급 정지 발동
    EmergencyStop { reason: String },
}

impl CommandValidator {
    /// [v2] WCET 보장: 지연 최소화 (분기 예측 최적화)
    #[inline]
    pub fn validate(&self, topic: &KeyExpr, msg: &RoxMessage) -> ValidationResult {
        // 1. 스키마 기반 범위 검증
        // 2. 물리적 안전 경계 검증
        // 3. 결과를 감사 로그에 기록 (async, non-blocking)
        // 4. 거부 시 rox-agent에 이벤트 전달
    }
}

impl GuardProcess {
    pub async fn run(&mut self) {
        loop {
            self.watchdog.send_heartbeat();
            match tokio::time::timeout(self.watchdog.heartbeat_interval, self.request_rx.recv()).await {
                Ok(Ok(req)) => {
                    let result = self.validator.validate(&req.topic, &req.message);
                    self.audit_logger.log(&req, &result);
                    let _ = self.response_tx.send(ValidationResponse { id: req.id, result }).await;
                }
                Ok(Err(_)) => self.failsafe.activate(),  // 채널 에러 → fail-safe
                Err(_) => {} // 타임아웃: 정상 (요청 없음)
            }
        }
    }
}

/// 🔧 [v3 신규] Guard Supervisor — rox-core 내에서 Guard 프로세스를 감시/재시작
/// Guard 프로세스 자체의 단일 장애점을 해소한다.
pub struct GuardSupervisor {
    guard_process: Option<std::process::Child>,
    config: GuardConfig,
    /// Guard 재시작 중에는 BlockAllCommands를 강제 적용
    interim_policy: FailsafePolicy,
    max_restart_attempts: u32,
    restart_count: u32,
    last_restart: Option<Instant>,
}

impl GuardSupervisor {
    /// rox-core 시작 시 Guard 프로세스를 spawn
    pub fn spawn_guard(&mut self) -> Result<()> {
        self.guard_process = Some(
            std::process::Command::new("rox-guard")
                .arg("--config").arg(&self.config.path)
                .spawn()?
        );
        self.restart_count = 0;
        Ok(())
    }

    /// Guard heartbeat가 timeout 내에 도착하지 않으면 호출
    pub async fn on_guard_timeout(&mut self) {
        // 1. 즉시 BlockAllCommands 발동 (재시작 중 모든 명령 차단)
        self.interim_policy = FailsafePolicy::BlockAllCommands;
        log::error!("Guard process unresponsive — activating BlockAllCommands");

        // 2. Guard 프로세스 강제 종료
        if let Some(proc) = &mut self.guard_process {
            let _ = proc.kill();
        }

        // 3. 재시작 시도 (최대 3회)
        if self.restart_count < self.max_restart_attempts {
            self.restart_count += 1;
            log::warn!("Restarting Guard process (attempt {}/{})", self.restart_count, self.max_restart_attempts);

            match self.spawn_guard() {
                Ok(_) => {
                    // Guard가 정상 시작하면 interim_policy 해제
                    // (Guard의 첫 heartbeat 수신 시 해제)
                }
                Err(e) => {
                    log::error!("Guard restart failed: {}. Maintaining BlockAllCommands.", e);
                }
            }
        } else {
            // 3회 재시작 실패 → EmergencyStop 발동
            log::error!("Guard restart exhausted. Triggering EmergencyStop.");
            self.interim_policy = FailsafePolicy::EmergencyStop;
        }
    }
}
```

### 5.2 Audit Logger — 불변 감사 로그

```rust
/// 모든 Agent 판단 + Guard 검증 결과를 기록
/// copper의 deterministic log를 확장하여 안전 인증용 감사 추적
pub struct AuditLogger {
    writer: BufWriter<File>,
    hasher: blake3::Hasher,        // 각 엔트리의 해시 체인
    sequence: AtomicU64,
}

pub struct AuditEntry {
    pub sequence: u64,
    pub timestamp: RoxTimestamp,
    pub entry_type: AuditEntryType,
    pub prev_hash: [u8; 32],       // 이전 엔트리 해시 (체인 무결성)
}

pub enum AuditEntryType {
    CommandValidated { topic: String, result: ValidationResult },
    AgentPolicyChange { update: PolicyUpdate },
    BoundaryViolation { node: String, violation: String },
    EmergencyStop { reason: String },
    NodeHealthChange { node: String, from: HealthStatus, to: HealthStatus },
}
```

---

## 6. 데이터 흐름 — 전체 통합

```
[Sensor Node] ──── rox-core 런타임 ────────────────────────────────────
     │                                                                  │
     ▼                                                                  │
  Publisher ──▶ RoxMessage ──▶ TransportSelector ──▶ Transport          │
     │              │               │                    │              │
     │         (hot path)      🧠 Agent가              SHM/TCP/        │
     │          sub-μs          추천한                  UDP/QUIC        │
     │                        트랜스포트                   │              │
     │                                                    │              │
     │              ┌────────── Subscriber ◀──────────────┘              │
     │              │                                                    │
     │              ▼                                                    │
     │         RoxMessage                                               │
     │              │                                                    │
     │         🛡️ Guard (CommandValidator)                             │
     │              │                                                    │
     │         ┌────┴──── Passed ────▶ [Actuator Node]                 │
     │         │                                                        │
     │         └── Rejected ────▶ AuditLog + Agent 경고                │
     │                                                                  │
─────┼──────────────────────────────────────────────────────────────────┘
     │
     │    메트릭 수집 (lock-free, non-blocking)
     │
     ▼
🧠 rox-agent 런타임 (별도 스레드, 낮은 우선순위) ─────────────────────
     │                                                                │
     ├── QoS Prediction Agent                                         │
     │     └── 트래픽 패턴 학습 → QoS 정책 피드백 ──▶ rox-core      │
     ├── Self-healing Agent                                           │
     │     └── 노드 헬스 감시 → 우회 경로 생성 ──▶ Transport Selector│
     ├── Anomaly Detection Agent                                      │
     │     └── jitter/패킷로스 탐지 → 경고 ──▶ Dashboard / Webhook   │
     └── 🔧 [v3] Throttle Agent                                      │
           └── 주파수 제한으로 대역폭 절감                               │
                                                                      │
──────────────────────────────────────────────────────────────────────┘

 📊 rox-dashboard (실시간 모니터링)
     │
     ├── 노드 상태 / 토폴로지 시각화
     ├── Agent 판단 이력
     ├── Guard 감사 로그
     └── 성능 메트릭 (Prometheus / Grafana)
```

---

## 7. 리팩토링 전략 — ⚡ [v2 전면 재조정] Phase별 구현

### Phase 1A: 최소 핵심 (2개월)

**목표:** 로컬 Pub/Sub 동작하는 최소 코어

작업 항목:
- `rox-protocol`: RoxMessage, KeyExpr (멀티 로봇 네임스페이스 포함)
- `rox-buffer`: Bytes 기반 zero-copy 버퍼
- `rox-transport::shm`: **[v2] iceoryx2 래핑** SHM 백엔드
- `rox-transport::tcp`: TCP 백엔드
- `rox-core`: Session, Publisher, Subscriber
- `rox-codec`: bincode 기반 직렬화
- `rox` 통합 크레이트: feature flag 구조
- `discovery`: **[v2] static 모드만**

검증 방법:
- 벤치마크: iceoryx2 래핑 SHM Pub/Sub 지연 시간 (목표: <5μs 포함 래핑 오버헤드)
- zenoh와 동일 조건에서 throughput 비교
- "5분 안에 첫 Pub/Sub 실행" 가이드 작동 확인

### Phase 1B: 결정론성 기초 (2개월)

**목표:** copper 수준의 결정론적 실행

작업 항목:
- `rox-core::graph`: YAML Task Graph 파서
- `rox-core::scheduler`: 순차 실행 스케줄러 (copper CopperList 참조)
- `rox-log`: 바이너리 로깅 (copper cu29-log 참조)
- `rox-replay`: **[v2] RoxClock::Replay 모드** 포함 비트 단위 리플레이
- `rox-derive`: `#[rox_node]` 매크로
- 에러 타입: **[v2] `thiserror` + `miette`** 기반 사용자 친화적 에러

검증 방법:
- 동일 입력 → 동일 출력 재현성 테스트
- 리플레이 모드에서 RoxClock이 로그 타임스탬프를 정확히 주입하는지 확인

### Phase 2: Agent Level 0 + Guard + Monitor (3개월)

**목표:** ⚡ [v2] 규칙 기반 Agent + 격리된 Guard + TUI 모니터링

작업 항목:
- `rox-agent::runtime`: **[v2] 별도 프로세스**, SHM 기반 메트릭 수신
- `rox-agent::anomaly_agent`: **[v2] Level 0 규칙 기반** 이상 탐지 (ML 없음)
- `rox-agent::qos_agent`: **[v2] Level 0** 통계적 QoS 예측
- `rox-agent::mode`: **[v2] Observer → Suggestion → Autonomous** 전환
- `rox-guard`: **[v2] 별도 프로세스**, watchdog, fail-safe, WCET 측정
- `rox-monitor`: **[v2 신규] ratatui 기반 TUI** 모니터링 도구
- **[v2] VersionedPolicyUpdate**: version + apply_at_cycle + rollback

검증 방법:
- 인위적 jitter 주입 → Level 0 Anomaly Agent 경고 발생 확인
- Guard 프로세스 강제 종료 → fail-safe 동작 확인
- Observer 모드에서 정책 변경이 발생하지 않는지 확인

### Phase 3: Agent Level 1 + 자기 학습 (2개월)

**목표:** ⚡ [v2] 온라인 학습 기반 지능형 통신 관제

작업 항목:
- `rox-agent::ml_runtime`: tract 기반 경량 추론
- `rox-agent::qos_agent`: Level 1 온라인 학습 (AutoEncoder)
- `rox-agent::healing_agent`: 우회 경로 생성 (petgraph)
- `rox-agent::throttle_agent`: **[v2] frequency throttling** (Pruning 대체)
- 자동 롤백 메커니즘

검증 방법:
- 24시간 관찰 → 자동 Autonomous 전환 → 정책 변경 품질 측정
- 노드 강제 종료 → Healing Agent 우회 경로 생성 시간 (목표: <50ms)
- 잘못된 정책 적용 → 자동 롤백 동작 확인

### Phase 4: 자체 SHM 검토 + Arrow (2개월)

**목표:** ⚡ [v2 신규] iceoryx2 래핑에서 자체 구현으로 전환 여부 판단

작업 항목:
- iceoryx2 래핑 오버헤드 벤치마크 분석
- 오버헤드 > 2μs 시에만 자체 lock-free SHM 착수
- `rox-codec::arrow`: Arrow 직렬화 (feature flag)
- `rox-buffer::shm`: SHM 페이로드 (feature flag)

판단 기준:
- 래핑 오버헤드 < 2μs → 자체 구현 불필요
- 래핑 오버헤드 > 2μs → 자체 구현 착수 (추가 4개월)

### Phase 5: 네트워크 + 브릿지 (3개월)

**목표:** 멀티 머신 + ROS 2 호환

작업 항목:
- `rox-transport::udp`: UDP 전송
- QUIC 전송 (실시간성 벤치마크 후 결정)
- `rox-core::discovery`: **[v2] multicast 모드** 추가
- `rox-bridge`: **[v2] zenoh-plugin-ros2dds 래핑** (Rox ↔ Zenoh ↔ ROS 2)
- 네트워크 파티션: 파티션 감지 → 독립 운영 → 재결합 시 정책 병합

### Phase 6: DX + 대시보드 (2개월)

**목표:** HORUS급 개발자 경험

작업 항목:
- `rox-cli`: `rox new / build / run / monitor / deploy`
- `rox-api`: axum 기반 REST API + SSE 실시간 이벤트
- `rox-dashboard`: React 웹 UI (노드 토폴로지, Agent 이력, 성능)
- Agent Level 2: 커뮤니티 데이터 공유 플랫폼 기반 사전학습 모델

---

## 8. 경쟁사 대비 예상 성능 차이

| 영역                      | zenoh               | iceoryx2          | copper-rs            | Rox (목표)                          |
|---------------------------|---------------------|-------------------|----------------------|-------------------------------------|
| 로컬 IPC 지연              | ~10μs (SHM)        | <1μs              | <1μs (CopperList)   | ~1-5μs (iceoryx2 래핑 오버헤드 포함) |
| 네트워크 지연              | ~50μs (TCP)        | N/A (로컬 전용)    | Zenoh 경유           | ~50μs + Agent 최적화로 안정성 향상   |
| 동시 노드 수              | 수천 개             | 수백 개            | 수십 개              | 수천 개 (zenoh급 확장)              |
| 결정론적 리플레이          | ✗                   | ✗                 | ✓ (비트 단위)        | ✓ + Agent 판단 로그 포함            |
| 장애 복구 시간             | 수동                | 수동              | 수동                 | <50ms (Self-healing Agent)          |
| QoS 조정                  | 수동 설정            | 수동 설정          | ✗                   | Level 0:규칙 → Level 2:ML 자동     |

**주의:** 위 수치는 목표치이며, 실제 벤치마크 결과가 나온 뒤에만 공식 주장할 것.

---

## 9. 특허 출원 대상 기술 포인트

1. **Agent-in-the-Loop 미들웨어**: 로봇 통신 미들웨어 내부에 AI Agent를
   제어 루프 밖에서 동작시켜, 통신 QoS를 실시간 예측/조정하는 아키텍처

2. **Self-healing Transport**: 통신 노드 장애 징후를 ML로 탐지하여,
   실시간 우회 경로를 자동 생성하는 방법

3. **Zero-copy Command Guard**: AI/외부 시스템의 로봇 명령을
   메모리 복사 없이 물리적 안전 경계 내에서 실시간 검증하는 구조

4. **Deterministic Replay with Agent Log**: 결정론적 리플레이에
   Agent의 판단 이력을 포함시켜, "왜 그 시점에 QoS가 변경되었는지"
   재현 가능한 감사 추적 방법

→ 가출원 시 위 4개를 각각 독립 청구항으로 구성 가능

---

## 10. 의사결정 기록

| 결정 사항                        | 선택                    | 이유                                                        |
|----------------------------------|------------------------|-------------------------------------------------------------|
| 비동기 런타임                     | tokio                  | Rust 생태계 사실상 표준, zenoh/dora 모두 사용                  |
| ⚡ SHM 구현                      | **iceoryx2 래핑**       | [v2] iceoryx2는 8년 검증. 자체 구현은 Phase 4에서 재검토       |
| ⚡ 메시지 직렬화                  | **bincode 기본**        | [v2] Arrow는 feature flag로 Phase 4에서 추가                  |
| Task Graph 포맷                  | YAML (RON 선택적)       | HORUS/dora의 YAML 접근성 + copper의 RON 정밀성 양립           |
| ⚡ Agent ML 엔진                 | **Level 0: 규칙기반**   | [v2] tract는 Level 1+에서만. 규칙기반부터 출시                 |
| 로깅 포맷                        | 자체 바이너리 (copper 참조)| copper의 zero-copy 구조화 로깅이 성능 최고                    |
| 우회 경로 계산                    | petgraph               | Rust 표준 그래프 라이브러리, 경로 알고리즘 내장                 |
| 감사 로그 해시                    | blake3                 | SHA-256 대비 15배 빠름, Rust-native                           |
| CLI 프레임워크                    | clap                   | Rust CLI 사실상 표준                                         |
| HTTP 프레임워크                   | axum                   | tokio 네이티브, tower 미들웨어 호환 (zenoh-plugin-rest도 사용) |
| 키 표현식                         | zenoh KeyExpr 호환      | ROS 2 브릿지 호환성 + zenoh 마이그레이션 경로 제공              |
| ⚡ Discovery (Phase 1)           | **static 모드만**       | [v2] multicast/gossip는 Phase 5에서 추가                     |
| ⚡ ROS 2 Bridge                  | **zenoh-ros2dds 래핑**  | [v2] 자체 구현 대신 Rox ↔ Zenoh ↔ ROS 2 경로                |
| ⚡ 결정론성 범위                  | **Hard/Soft 2계층**     | [v2] Agent 판단은 Soft Layer, 기록값 주입으로 리플레이         |
| ⚡ Guard 프로세스                | **별도 프로세스 격리**   | [v2] Freedom from Interference. 메모리 격리 보장              |
| ⚡ Guard 안전 등급               | **QM (초기)**           | [v2] SIL 인증은 v2.0 이후 별도 로드맵                         |
| ⚡ 사용자 크레이트                | **`rox` 단일 진입점**   | [v2] zenoh 전략 참조. feature flag로 선택적 활성화             |
| ⚡ TUI 모니터링                   | **ratatui**             | [v2 신규] Phase 2에서 Dashboard보다 먼저 제공                 |
| 🔧 RoxPayload (Phase 1)          | **Bytes 단일**          | [v3] 3종 enum 복잡도 제거. SHM/Arrow는 Phase 4 feature flag  |
| 🔧 Guard 이중화                   | **Supervisor 자동재시작** | [v3] 재시작 중 BlockAllCommands. 3회 실패 시 EmergencyStop    |
| 🔧 Transport 전환                 | **4단계 프로토콜**       | [v3] drain→buffer→switch→replay로 메시지 유실 방지            |
| 🔧 Agent 전환 기준                | **복합 조건**            | [v3] 시간 + 데이터량 + 커버리지 + 안정성 4중 검증              |

---

## 부록 A: 용어 정리

- **GuardSupervisor**: 🔧 [v3] Guard 프로세스를 감시/재시작하는 rox-core 내부 컴포넌트
- **Cycle Boundary**: 🔧 [v3] 스케줄러가 PolicyUpdate를 적용하는 정확한 시점
- **TransportSwitchReport**: 🔧 [v3] 트랜스포트 전환 결과를 기록하는 구조체
- **ModeTransitionCriteria**: 🔧 [v3] Observer→Suggestion→Autonomous 전환의 복합 판단 조건
- **Node**: 센서/처리/액추에이터를 추상화한 실행 단위 (copper의 Task에 대응)
- **Topic**: Pub/Sub 기반 데이터 스트림 (zenoh의 Key Expression에 대응)
- **Service**: Request/Response 기반 동기 호출 (zenoh의 Queryable에 대응)
- **RoxMessage**: 프로토콜 독립적 메시지 단위 (헤더 + 페이로드)
- **TaskGraph**: 노드 간 연결을 정의하는 DAG (copper의 RON Config에 대응)
- **Transport**: 메시지를 실제로 전달하는 계층 (SHM/TCP/UDP/QUIC/Serial)
- **AgentTap**: rox-core에서 메트릭을 수집하는 지점 (hot path 외부)
- **PolicyUpdate**: Agent가 rox-core에 피드백하는 정책 변경 메시지
- **Guard**: AI/외부 명령을 물리적 안전 경계 내에서 검증하는 레이어
- **AuditEntry**: Guard/Agent의 판단을 불변 해시 체인으로 기록하는 감사 항목

## 부록 B: 경쟁사 GitHub 레포지토리 참조

| 프로젝트    | 레포지토리                                        | 참조한 핵심 파일/모듈                              |
|------------|--------------------------------------------------|--------------------------------------------------|
| zenoh      | `eclipse-zenoh/zenoh`                            | zenoh-transport, zenoh-buffers, zenoh-shm         |
| copper-rs  | `copper-project/copper-rs`                       | cu29-runtime (copperlist, tasks, curuntime)        |
| dora-rs    | `dora-rs/dora`                                   | shared-memory-server, node-hub, message           |
| iceoryx2   | `eclipse-iceoryx/iceoryx2`                       | iceoryx2-bb-lock-free, iceoryx2-cal, iceoryx2-tunnels |
| HORUS      | `softmata/horus`, `horus-robotics/horus`         | horus_core/communication, horus_macros            |
