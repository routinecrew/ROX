//! API server setup.

use anyhow::{Context, Result};
use axum::routing::get;
use axum::Router;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::routes;
use rox_protocol::{AgentEvent, NodeConfig};

/// Shared application state for API handlers.
pub struct AppState {
    pub topic_keys: Mutex<Vec<String>>,
    pub nodes: Vec<NodeConfig>,
    pub event_tx: broadcast::Sender<AgentEvent>,
}

/// REST API + SSE server for Rox monitoring.
pub struct ApiServer {
    bind_addr: String,
    state: Arc<AppState>,
}

impl ApiServer {
    /// Create a new API server.
    pub fn new(
        bind_addr: impl Into<String>,
        nodes: Vec<NodeConfig>,
        event_tx: broadcast::Sender<AgentEvent>,
    ) -> Self {
        Self {
            bind_addr: bind_addr.into(),
            state: Arc::new(AppState {
                topic_keys: Mutex::new(Vec::new()),
                nodes,
                event_tx,
            }),
        }
    }

    /// Update the list of active topics.
    pub async fn update_topics(&self, topics: Vec<String>) {
        *self.state.topic_keys.lock().await = topics;
    }

    /// Get the event sender for broadcasting agent events.
    pub fn event_sender(&self) -> broadcast::Sender<AgentEvent> {
        self.state.event_tx.clone()
    }

    /// Build the Axum router.
    pub fn router(&self) -> Router {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        Router::new()
            .route("/v1/health", get(routes::health))
            .route("/v1/topics", get(routes::list_topics))
            .route("/v1/nodes", get(routes::list_nodes))
            .route("/v1/events/stream", get(routes::event_stream))
            .layer(cors)
            .layer(TraceLayer::new_for_http())
            .with_state(Arc::clone(&self.state))
    }

    /// Start serving. Runs until shutdown signal.
    pub async fn serve(self) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(&self.bind_addr)
            .await
            .with_context(|| format!("failed to bind API on {}", self.bind_addr))?;

        let local_addr = listener.local_addr()?;
        info!(addr = %local_addr, "API server listening");

        axum::serve(listener, self.router())
            .await
            .context("API server error")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_app() -> (Router, Arc<AppState>) {
        let (tx, _) = broadcast::channel(256);
        let nodes = vec![NodeConfig {
            id: "lidar".to_string(),
            node_type: "drivers::Lidar".to_string(),
            rate_hz: Some(10.0),
            priority: 0,
            background: false,
        }];
        let state = Arc::new(AppState {
            topic_keys: Mutex::new(vec!["robot/lidar/scan".to_string()]),
            nodes,
            event_tx: tx,
        });
        let server = ApiServer {
            bind_addr: "127.0.0.1:0".to_string(),
            state: Arc::clone(&state),
        };
        (server.router(), state)
    }

    #[tokio::test]
    async fn test_server_creation() {
        let (tx, _) = broadcast::channel(256);
        let server = ApiServer::new("127.0.0.1:0", vec![], tx);
        let _router = server.router();
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let (app, _) = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn test_topics_endpoint() {
        let (app, _) = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.as_array().unwrap().len() == 1);
        assert_eq!(json[0]["key"], "robot/lidar/scan");
    }

    #[tokio::test]
    async fn test_nodes_endpoint() {
        let (app, _) = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/nodes")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json[0]["id"], "lidar");
        assert_eq!(json[0]["node_type"], "drivers::Lidar");
    }

    #[tokio::test]
    async fn test_update_topics() {
        let (tx, _) = broadcast::channel(256);
        let server = ApiServer::new("127.0.0.1:0", vec![], tx);

        server
            .update_topics(vec!["a".to_string(), "b".to_string()])
            .await;

        let app = server.router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.as_array().unwrap().len(), 2);
    }
}
