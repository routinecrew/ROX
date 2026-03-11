// ============================================================
// 공용 Mock — 모든 에이전트가 독립 개발 시 사용
// ============================================================
// Agent B(core)가 완성되기 전에 A, C, D, E가 사용하는 가짜 구현.
// 실제 통합 시에는 이 파일을 제거하고 rox-core의 TopicManager를 사용한다.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::RwLock;

use crate::contracts::*;

// ============================================================
// MockTopicRegistry
// ============================================================

pub struct MockTopicRegistry {
    topics: RwLock<HashMap<String, MockTopic>>,
}

struct MockTopic {
    sender: MessageSender,
    qos: QoSMetadata,
}

impl MockTopicRegistry {
    pub fn new() -> Self {
        Self {
            topics: RwLock::new(HashMap::new()),
        }
    }

    /// 테스트용: 주기적으로 가짜 메시지를 발행하는 토픽 추가
    pub async fn add_test_topic(&self, key: &str) {
        let (sender, _) = new_message_bus();
        let tx = sender.clone();
        let key_expr = KeyExpr(key.to_string());

        tokio::spawn(async move {
            let mut seq = 0u64;
            loop {
                let msg = RoxMessage {
                    header: MessageHeader {
                        key: key_expr.clone(),
                        timestamp: RoxTimestamp {
                            hw_time: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_nanos() as u64,
                            logical_time: seq,
                        },
                        qos: QoSMetadata::default(),
                        source_id: NodeId("mock-node".to_string()),
                        sequence: seq,
                    },
                    payload: Bytes::from(vec![0u8; 256]),
                };
                if tx.send(Arc::new(msg)).is_err() {
                    break;
                }
                seq += 1;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });

        self.topics.write().await.insert(
            key.to_string(),
            MockTopic {
                sender,
                qos: QoSMetadata::default(),
            },
        );
    }
}

impl Default for MockTopicRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TopicRegistry for MockTopicRegistry {
    async fn publish(&self, key: &KeyExpr, qos: QoSMetadata) -> Result<MessageSender> {
        let (sender, _) = new_message_bus();
        self.topics.write().await.insert(
            key.as_str().to_string(),
            MockTopic {
                sender: sender.clone(),
                qos,
            },
        );
        Ok(sender)
    }

    async fn subscribe(&self, key: &KeyExpr) -> Result<MessageReceiver> {
        let topics = self.topics.read().await;
        let topic = topics
            .get(key.as_str())
            .ok_or_else(|| anyhow::anyhow!("topic not found: {}", key))?;
        Ok(topic.sender.subscribe())
    }

    async fn unpublish(&self, key: &KeyExpr) -> Result<()> {
        self.topics.write().await.remove(key.as_str());
        Ok(())
    }

    async fn list_topics(&self) -> Vec<KeyExpr> {
        self.topics
            .read()
            .await
            .keys()
            .map(|k| KeyExpr(k.clone()))
            .collect()
    }
}

// ============================================================
// MockAgentEventBus — Agent E(API)용 가짜 이벤트 발행기
// ============================================================

pub fn mock_agent_event_bus() -> tokio::sync::broadcast::Sender<AgentEvent> {
    let (tx, _) = tokio::sync::broadcast::channel::<AgentEvent>(256);
    let sender = tx.clone();

    tokio::spawn(async move {
        let mut seq = 0u64;
        loop {
            let event = if seq % 3 == 0 {
                AgentEvent::CongestionWarning {
                    topic: KeyExpr::new("robot-01", "lidar", "scan"),
                    probability: 0.75,
                    recommended_priority: Priority::High,
                }
            } else if seq % 3 == 1 {
                AgentEvent::AnomalyDetected {
                    topic: KeyExpr::new("robot-01", "motor", "cmd"),
                    anomaly_type: "latency_spike".to_string(),
                    severity: 0.6,
                }
            } else {
                AgentEvent::ThrottleApplied {
                    topic: KeyExpr::new("robot-02", "camera", "raw"),
                    new_interval_ms: 200,
                }
            };

            if sender.send(event).is_err() {
                break;
            }
            seq += 1;
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });

    tx
}

// ============================================================
// 사용 예시
// ============================================================
//
// #[tokio::test]
// async fn test_with_mock() {
//     let registry = Arc::new(MockTopicRegistry::new());
//     registry.add_test_topic("robot-01/lidar/scan").await;
//
//     let mut rx = registry.subscribe(&KeyExpr::new("robot-01", "lidar", "scan")).await.unwrap();
//     let msg = rx.recv().await.unwrap();
//     assert_eq!(msg.header.key, KeyExpr::new("robot-01", "lidar", "scan"));
// }
