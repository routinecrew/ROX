//! API route handlers.

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::Json;
use serde::Serialize;
use std::convert::Infallible;
use std::sync::Arc;

use crate::server::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

#[derive(Serialize)]
pub struct TopicInfo {
    pub key: String,
}

#[derive(Serialize)]
pub struct NodeInfo {
    pub id: String,
    pub node_type: String,
    pub rate_hz: Option<f64>,
    pub priority: u8,
}

/// GET /v1/health
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// GET /v1/topics
pub async fn list_topics(State(state): State<Arc<AppState>>) -> Json<Vec<TopicInfo>> {
    let topics = state.topic_keys.lock().await;
    Json(
        topics
            .iter()
            .map(|k| TopicInfo { key: k.clone() })
            .collect(),
    )
}

/// GET /v1/nodes
pub async fn list_nodes(State(state): State<Arc<AppState>>) -> Json<Vec<NodeInfo>> {
    Json(
        state
            .nodes
            .iter()
            .map(|n| NodeInfo {
                id: n.id.clone(),
                node_type: n.node_type.clone(),
                rate_hz: n.rate_hz,
                priority: n.priority,
            })
            .collect(),
    )
}

/// GET /v1/events/stream — SSE event stream
pub async fn event_stream(
    State(state): State<Arc<AppState>>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.event_tx.subscribe();

    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(agent_event) => {
                    let json = serde_json::to_string(&agent_event).unwrap_or_default();
                    yield Ok::<_, Infallible>(Event::default().data(json).event("agent_event"));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    yield Ok(Event::default().comment("missed events"));
                }
                Err(_) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
