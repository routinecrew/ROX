# ROX — Claude Code 지침

## 프로젝트 개요
Rox — Intelligent Nerve System for Robotics.
zenoh + iceoryx2 + copper-rs의 강점을 통합하고, AI Agent 레이어를 추가한 로보틱스 통신 미들웨어.

## 반드시 읽어야 할 파일 (우선순위 순)
1. `contracts/shared_types.rs` — 공유 타입/trait. **절대 임의 변경 금지.**
2. `contracts/mock.rs` — 독립 개발용 mock 구현
3. `Rox_System_Design.md` — 전체 아키텍처 설계서
4. `PARALLEL_DEV_GUIDE.md` — 병렬 개발 운영 가이드
5. `skills/agent-*.md` — 본인 담당 에이전트의 상세 스킬

## 에이전트 배정
| 에이전트 | 크레이트 | 역할 |
|----------|----------|------|
| Agent A | `rox-protocol`, `rox-codec`, `rox-buffer` | 프로토콜, 직렬화, zero-copy 버퍼 |
| Agent B | `rox-core`, `rox-derive` | 코어 런타임 엔진, 매크로 |
| Agent C | `rox-transport`, `rox-log`, `rox-replay` | 멀티 트랜스포트, 로깅, 리플레이 |
| Agent D | `rox-agent`, `rox-guard` | AI 에이전트, 안전 검증 |
| Agent E | `rox-api`, `rox-cli`, `rox-bridge` | REST API, CLI, ROS 2 브릿지 |

## 아키텍처

**14-크레이트 workspace** — 전문 영역별 에이전트 병렬 개발:

```
contracts/shared_types.rs  ← Single source of truth for types & traits
        │
    rox-protocol           ← 와이어 프로토콜 (KeyExpr, RoxMessage, wire encoding)
    rox-codec              ← 직렬화 (bincode 기본, Arrow 선택적)
    rox-buffer             ← zero-copy 버퍼 (ZBuf, MemoryPool, SHM)
    ├── rox-core           ← 핵심 런타임 (Session, Node, Topic, TaskGraph, Scheduler)
    ├── rox-transport      ← 전송 계층 (SHM/TCP/UDP/Serial, TransportSelector)
    ├── rox-log            ← 결정론적 로깅
    ├── rox-replay         ← 비트 단위 리플레이
    ├── rox-agent          ← AI 통신 관제 (QoS예측, 자가치유, 이상탐지, 주파수제한)
    ├── rox-guard          ← 안전 검증 (명령검증, GeoFence, 감사로그)
    ├── rox-bridge         ← ROS 2 / Zenoh 브릿지
    ├── rox-api            ← REST API + SSE (port 9090)
    ├── rox-cli            ← CLI 도구 (rox new/run/monitor)
    └── rox-derive         ← proc-macro (#[rox_node])
```

**Key abstractions:**
- `TopicRegistry` trait — 추상 토픽 레지스트리; 모든 크레이트가 이에 의존
- `Transport` trait — 전송 계층 공통 인터페이스
- `RoxNode` trait — 노드 생명주기 (init/tick/shutdown)
- `MockTopicRegistry` in `contracts/mock.rs` — 독립 개발용

**데이터 흐름:** Sensor Node → Publisher → RoxMessage → TransportSelector → Transport(SHM/TCP) → Subscriber → Actuator Node. Agent는 hot path 밖에서 메타데이터만 관제.

## 빌드/테스트

```bash
# 전체 빌드
cargo build --workspace

# 전체 테스트
cargo test --workspace

# 개별 크레이트
cargo test -p rox-protocol
cargo test -p rox-codec
cargo test -p rox-buffer
cargo test -p rox-core
cargo test -p rox-transport
cargo test -p rox-log
cargo test -p rox-agent
cargo test -p rox-guard
cargo test -p rox-api
cargo test -p rox-cli
cargo test -p rox-bridge
```

## 코딩 규칙
- **No `unwrap()`** — `anyhow::Result`로 에러 전파
- **No `println!`** — `tracing` 크레이트 사용 (`info!`, `debug!`, `warn!`, `error!`)
- **`unsafe` requires safety justification** in comments
- **Doc comments required** on all public APIs
- 각 크레이트는 `contracts.rs`와 `mock.rs` 로컬 복사본을 가짐; canonical source는 `contracts/shared_types.rs`

## contracts 변경 절차
1. 변경 필요성 설명과 함께 PR 생성
2. 영향받는 에이전트 목록 명시
3. `cargo test --workspace` 통과 확인 후 merge

## 독립 개발 방법
core가 아직 없어도 `contracts/mock.rs`의 `MockTopicRegistry`를 사용하면
각 크레이트를 독립적으로 빌드하고 테스트할 수 있다.
통합 시에만 mock → 실제 TopicManager로 교체.

## Token Optimization
- **서브에이전트(Agent tool) 사용 금지** — 직접 Glob, Grep, Read 등 기본 도구로 해결할 것
- **응답은 최소한으로** — 코드 변경 시 변경 사항만 간결히 설명
- **파일은 필요한 부분만 읽기** — offset/limit 활용
- **병렬 도구 호출 활용** — 독립적인 도구 호출은 반드시 한 번에 병렬 실행
