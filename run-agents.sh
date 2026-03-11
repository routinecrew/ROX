#!/bin/bash
# =============================================================
# ROX 에이전트 실행 스크립트
# =============================================================
# 사용법:
#   ./run-agents.sh a    → Agent A (protocol/codec/buffer) 실행
#   ./run-agents.sh b    → Agent B (core/derive) 실행
#   ./run-agents.sh c    → Agent C (transport/log/replay) 실행
#   ./run-agents.sh d    → Agent D (agent/guard) 실행
#   ./run-agents.sh e    → Agent E (api/cli/bridge) 실행
#
# 실행 순서:
#   1단계: 터미널 1에서 → ./run-agents.sh a    (완료 대기)
#   2단계: 터미널 2에서 → ./run-agents.sh b
#          터미널 3에서 → ./run-agents.sh d
#   3단계: 터미널 4에서 → ./run-agents.sh c
#          터미널 5에서 → ./run-agents.sh e
# =============================================================

set -e
cd "$(dirname "$0")"

AGENT="$1"

CLAUDE_CMD="claude --dangerously-skip-permissions -p"

if [ -z "$AGENT" ]; then
  echo "사용법: ./run-agents.sh [a|b|c|d|e]"
  echo ""
  echo "  a  →  Agent A: rox-protocol + rox-codec + rox-buffer (프로토콜 & 직렬화)"
  echo "  b  →  Agent B: rox-core + rox-derive (코어 런타임 엔진)"
  echo "  c  →  Agent C: rox-transport + rox-log + rox-replay (통신 & 로깅)"
  echo "  d  →  Agent D: rox-agent + rox-guard (AI 에이전트 & 안전)"
  echo "  e  →  Agent E: rox-api + rox-cli + rox-bridge (인터페이스 & 통합)"
  echo ""
  echo "권장 순서: a → (b, d 동시) → (c, e 동시)"
  exit 1
fi

case "$AGENT" in
  a)
    echo "🚀 Agent A (rox-protocol, rox-codec, rox-buffer) 시작..."
    $CLAUDE_CMD "
당신은 Agent A입니다. Rox 프로젝트의 프로토콜/직렬화/버퍼 기반 계층을 만듭니다.

먼저 다음 파일들을 읽으세요:
- skills/agent-a-protocol.md (당신의 스킬)
- contracts/shared_types.rs (공유 타입 계약)
- Rox_System_Design.md (시스템 설계서) — 섹션 3.1, 2.4만
- CLAUDE.md (프로젝트 규칙)

작업 디렉토리:
- crates/rox-protocol/
- crates/rox-codec/
- crates/rox-buffer/

구현 순서:
1. contracts/shared_types.rs의 타입들을 각 크레이트의 src/contracts.rs로 확인
2. rox-protocol: KeyExpr(와일드카드 매칭), RoxMessage, MessageHeader, wire 인코딩/디코딩
3. rox-codec: RoxSerialize trait, bincode 직렬화, robotics_types (CmdVel, PointCloud 등)
4. rox-buffer: ZBuf(zero-copy 버퍼), MemoryPool, SHM stub

각 단계마다 cargo test가 통과하게 해주세요.
unwrap() 금지, println! 금지, tracing 사용.
"
    ;;

  b)
    echo "🚀 Agent B (rox-core, rox-derive) 시작..."
    $CLAUDE_CMD "
당신은 Agent B입니다. Rox 프로젝트의 코어 런타임 엔진을 만듭니다.

먼저 다음 파일들을 읽으세요:
- skills/agent-b-core.md (당신의 스킬)
- contracts/shared_types.rs (공유 타입 계약)
- contracts/mock.rs (Mock 구현)
- Rox_System_Design.md — 섹션 3.3(Node), 3.4(TaskGraph), 3.5(Config)
- CLAUDE.md (프로젝트 규칙)

작업 디렉토리:
- crates/rox-core/
- crates/rox-derive/

구현 순서:
1. 공유 타입/mock을 src/contracts.rs, src/mock.rs로 확인
2. Config (config.rs) — serde_yaml로 RoxConfig 로딩 + notify 핫 리로드
3. TopicManager (topic.rs) — TopicRegistry trait 구현
4. Node (node.rs) — NodeRunner 생명주기 관리
5. TaskGraph (graph.rs) — petgraph DAG + 위상 정렬
6. Scheduler (scheduler.rs) — 결정론적 사이클 실행 + PolicyUpdate 적용
7. Session (session.rs) — 엔진 진입점
8. rox-derive: #[rox_node] proc-macro

cargo test -p rox-core가 통과해야 합니다.
unwrap() 금지, println! 금지, tracing 사용.
"
    ;;

  c)
    echo "🚀 Agent C (rox-transport, rox-log, rox-replay) 시작..."
    $CLAUDE_CMD "
당신은 Agent C입니다. Rox 프로젝트의 통신 인프라와 로깅 시스템을 만듭니다.

먼저 다음 파일들을 읽으세요:
- skills/agent-c-transport.md (당신의 스킬)
- contracts/shared_types.rs (공유 타입 계약)
- contracts/mock.rs (Mock 구현)
- Rox_System_Design.md — 섹션 3.2(Transport)
- CLAUDE.md (프로젝트 규칙)

작업 디렉토리:
- crates/rox-transport/
- crates/rox-log/
- crates/rox-replay/

rox-core 의존성 없이 독립 빌드하세요.
contracts/shared_types.rs의 타입과 contracts/mock.rs를 크레이트 내부에 사용하세요.

구현 순서:
1. rox-transport: Transport trait 정의
2. TcpTransport (tcp.rs) — tokio TCP, length-prefixed framing
3. UdpTransport (udp.rs) — 저지연 UDP
4. TransportSelector (selector.rs) — Agent 연동 동적 전환, 4단계 프로토콜
5. SHM stub (shm.rs) — mock SHM (Phase 4에서 iceoryx2 연동)
6. rox-log: RoxLogger + LogEntry + mmap writer
7. rox-replay: LogReader + ReplayEngine

cargo test가 통과해야 합니다.
unwrap() 금지, println! 금지, tracing 사용.
"
    ;;

  d)
    echo "🚀 Agent D (rox-agent, rox-guard) 시작..."
    $CLAUDE_CMD "
당신은 Agent D입니다. Rox 프로젝트의 AI 에이전트 런타임과 안전 검증 레이어를 만듭니다.

먼저 다음 파일들을 읽으세요:
- skills/agent-d-intelligence.md (당신의 스킬)
- contracts/shared_types.rs (공유 타입 계약)
- contracts/mock.rs (Mock 구현)
- Rox_System_Design.md — 섹션 4(Agent), 5(Guard)
- CLAUDE.md (프로젝트 규칙)

작업 디렉토리:
- crates/rox-agent/
- crates/rox-guard/

rox-core 의존성 없이 독립 빌드하세요.
중요: ONNX Runtime(ort)은 feature flag(onnx)로 선택적. 먼저 Level 0(규칙 기반)을 완전 구현.

구현 순서:
1. rox-agent:
   - AgentRuntime (runtime.rs) — 메인 루프 + 모드 관리
   - QoSPredictionAgent (qos_agent.rs) — Level 0 규칙 기반
   - SelfHealingAgent (healing_agent.rs) — petgraph 대체 경로
   - AnomalyDetectionAgent (anomaly_agent.rs) — Z-score 기반
   - ThrottleAgent (throttle_agent.rs) — 주파수 제한
   - RuleEngine (rule_engine.rs) — nom 조건 파서
   - ModeTransition (mode_transition.rs) — Observer→Suggestion→Autonomous
2. rox-guard:
   - CommandValidator (validator.rs) — 범위/GeoFence 검증
   - AuditLogger (audit.rs) — blake3 해시 체인
   - GuardWatchdog (watchdog.rs)
   - FailsafePolicy (failsafe.rs)

cargo test가 통과해야 합니다.
unwrap() 금지, println! 금지, tracing 사용.
"
    ;;

  e)
    echo "🚀 Agent E (rox-api, rox-cli, rox-bridge) 시작..."
    $CLAUDE_CMD "
당신은 Agent E입니다. Rox 프로젝트의 사용자 대면 인터페이스를 만듭니다.

먼저 다음 파일들을 읽으세요:
- skills/agent-e-interface.md (당신의 스킬)
- contracts/shared_types.rs (공유 타입 계약)
- contracts/mock.rs (Mock 구현)
- CLAUDE.md (프로젝트 규칙)

작업 디렉토리:
- crates/rox-api/
- crates/rox-cli/
- crates/rox-bridge/

rox-core 의존성을 제거하고 독립 빌드하세요.
contracts/shared_types.rs의 타입과 contracts/mock.rs를 크레이트 내부에 사용하세요.

구현 순서:
1. rox-api:
   - AppState (state.rs) — TopicRegistry + event_bus
   - axum 라우터 (routes.rs)
   - 노드 핸들러 (handler/nodes.rs) — GET /v1/nodes
   - 토픽 핸들러 (handler/topics.rs) — GET /v1/topics
   - 이벤트 핸들러 (handler/events.rs) — GET /v1/events, SSE
   - 설정 핸들러 (handler/config.rs) — GET/PATCH /v1/config
   - 헬스체크 (handler/health.rs)
   - 이벤트 저장소 (store.rs) — 링 버퍼
2. rox-cli:
   - Clap 서브커맨드 구조
   - rox new, rox run, rox monitor, rox nodes, rox topics
3. rox-bridge:
   - ROS 2 토픽 매핑 (ros2.rs)
   - Zenoh 호환 브릿지 (zenoh.rs)

cargo test가 통과해야 합니다.
unwrap() 금지, println! 금지, tracing 사용.
"
    ;;

  *)
    echo "❌ 알 수 없는 에이전트: $AGENT"
    echo "사용법: ./run-agents.sh [a|b|c|d|e]"
    exit 1
    ;;
esac
