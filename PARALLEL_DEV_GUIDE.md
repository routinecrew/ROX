# ROX — 병렬 개발 가이드

> 5개 에이전트가 동시에 개발하기 위한 운영 매뉴얼

---

## 1. 전체 구조 요약

```
                    ┌─────────────────────────────────────┐
                    │        contracts/shared_types.rs     │
                    │  (모든 에이전트가 공유하는 타입/trait)  │
                    └──────────────┬──────────────────────┘
                                   │
       ┌───────────┬───────────────┼───────────────┬───────────┐
       │           │               │               │           │
 ┌─────▼─────┐ ┌───▼────┐   ┌─────▼─────┐   ┌─────▼────┐ ┌────▼─────┐
 │  Agent A   │ │Agent B │   │  Agent C  │   │ Agent D  │ │ Agent E  │
 │ protocol   │ │ core   │   │ transport │   │ agent    │ │ api/cli  │
 │ codec      │ │ derive │   │ log       │   │ guard    │ │ bridge   │
 │ buffer     │ │        │   │ replay    │   │          │ │          │
 │ (기반 계층) │ │(런타임) │   │ (통신)    │   │(지능/안전)│ │(인터페이스)│
 └─────┬─────┘ └───┬────┘   └───────────┘   └──────────┘ └──────────┘
       │           │
       │     ┌─────┘ (core는 protocol/codec/buffer에 의존)
       └─────┘
```

---

## 2. 에이전트 역할 배정

| 에이전트 | 크레이트 | 핵심 역할 | 스킬 파일 |
|----------|----------|----------|-----------|
| **Agent A** | `rox-protocol`, `rox-codec`, `rox-buffer` | 프로토콜, 직렬화, zero-copy 버퍼 | `skills/agent-a-protocol.md` |
| **Agent B** | `rox-core`, `rox-derive` | 코어 런타임, TaskGraph, Config | `skills/agent-b-core.md` |
| **Agent C** | `rox-transport`, `rox-log`, `rox-replay` | 멀티 트랜스포트, 결정론적 로깅 | `skills/agent-c-transport.md` |
| **Agent D** | `rox-agent`, `rox-guard` | AI 에이전트, 안전 검증 | `skills/agent-d-intelligence.md` |
| **Agent E** | `rox-api`, `rox-cli`, `rox-bridge` | REST API, CLI, ROS 2 브릿지 | `skills/agent-e-interface.md` |

---

## 3. 의존성 그래프와 병렬화 전략

```
Week 1-2:  [A: protocol/codec/buffer]  [D: agent/guard 단독]  [E: API mock]
                      │
Week 3-4:  [A: 완성] ──▶ [B: core 시작]  [C: transport 시작]
                              │                │
Week 5-6:     [B: core 완성]  [C: transport 완성]  [D: 통합]  [E: 통합]
                              │                │               │
Week 7-8:  ◀──────── 전체 통합 테스트 + 버그 수정 ────────────▶
```

### 핵심 원칙: Mock으로 독립 개발

Agent A(protocol)가 완성되기 전에도 B, C, D, E는 **mock**을 써서 동시 개발한다.

```rust
// 모든 에이전트가 사용하는 공통 mock (contracts/mock.rs)

pub struct MockTopicRegistry {
    topics: RwLock<HashMap<String, MockTopic>>,
}

#[async_trait]
impl TopicRegistry for MockTopicRegistry {
    async fn publish(&self, key: &KeyExpr, qos: QoSMetadata) -> Result<MessageSender> {
        let (sender, _) = new_message_bus();
        Ok(sender)
    }
    async fn subscribe(&self, key: &KeyExpr) -> Result<MessageReceiver> {
        // ...
    }
    async fn unpublish(&self, key: &KeyExpr) -> Result<()> { Ok(()) }
    async fn list_topics(&self) -> Vec<KeyExpr> { vec![] }
}
```

**이것이 `TopicRegistry` trait으로 인터페이스를 분리한 이유다.**
mock만 교체하면 실제 core 없이 각 크레이트를 독립 실행할 수 있다.

---

## 4. Claude Code 에이전트 실행 방법

### 4.1 사전 준비

```bash
cd ROX

# 워크스페이스 빌드 확인
cargo build --workspace
```

### 4.2 에이전트 실행

```bash
# 실행 스크립트에 권한 부여
chmod +x run-agents.sh

# 1단계: Agent A 먼저 (기반 계층)
./run-agents.sh a

# 2단계: Agent B, D 동시 (core + intelligence)
./run-agents.sh b    # 터미널 2
./run-agents.sh d    # 터미널 3

# 3단계: Agent C, E 동시 (transport + interface)
./run-agents.sh c    # 터미널 4
./run-agents.sh e    # 터미널 5
```

### 4.3 에이전트별 브랜치

| 에이전트 | 브랜치 |
|----------|--------|
| Agent A | `agent-a/protocol` |
| Agent B | `agent-b/core` |
| Agent C | `agent-c/transport` |
| Agent D | `agent-d/intelligence` |
| Agent E | `agent-e/interface` |

---

## 5. contracts 변경 절차

1. 변경 필요성 설명과 함께 PR 생성
2. 영향받는 에이전트 목록 명시
3. 모든 크레이트 `cargo test --workspace` 통과 확인 후 merge
4. 각 크레이트의 `src/contracts.rs`에 변경 반영

---

## 6. 통합 테스트 절차

```bash
# 전체 워크스페이스 빌드
cargo build --workspace

# 전체 테스트
cargo test --workspace

# 개별 크레이트 테스트
cargo test -p rox-protocol
cargo test -p rox-codec
cargo test -p rox-buffer
cargo test -p rox-core
cargo test -p rox-transport
cargo test -p rox-log
cargo test -p rox-replay
cargo test -p rox-agent
cargo test -p rox-guard
cargo test -p rox-api
cargo test -p rox-cli
cargo test -p rox-bridge

# 벤치마크
cargo bench -p rox-protocol
cargo bench -p rox-transport
```
