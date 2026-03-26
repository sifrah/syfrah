//! External gRPC-compatible API for fabric operations.
//!
//! Since proto code generation tooling (buf/prost) is not yet available, this
//! module implements a REST/JSON gateway that mirrors the FabricService RPCs
//! defined in the planned `fabric.proto` (see #356).  Each endpoint accepts
//! and returns JSON with the same field names the proto messages will use,
//! making the migration to real gRPC transparent to clients.
//!
//! Endpoints:
//! - `POST /v1/fabric/peering/start`          — StartPeering
//! - `POST /v1/fabric/peering/stop`           — StopPeering
//! - `GET  /v1/fabric/peering/requests`       — ListPeeringRequests
//! - `POST /v1/fabric/peering/accept`         — AcceptPeering
//! - `POST /v1/fabric/peering/reject`         — RejectPeering
//! - `POST /v1/fabric/peers/remove`           — RemovePeer
//! - `POST /v1/fabric/peers/update-endpoint`  — UpdatePeerEndpoint
//! - `POST /v1/fabric/reload`                 — Reload
//! - `POST /v1/fabric/rotate-secret`          — RotateSecret
//! - `GET  /v1/fabric/status`                 — GetStatus (health check)
//!
//! Configuration (in `~/.syfrah/config.toml`):
//!
//! ```toml
//! [grpc]
//! enabled = true
//! listen = "0.0.0.0:8443"
//! ```

use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::control::{FabricHandler, FabricRequest, FabricResponse};

/// Default listen address for the gRPC API.
const DEFAULT_LISTEN: &str = "0.0.0.0:8443";

/// Configuration for the gRPC-compatible API server.
#[derive(Debug, Clone)]
pub struct GrpcApiConfig {
    /// Whether the gRPC API server is enabled.
    pub enabled: bool,
    /// Socket address to bind to.
    pub listen: SocketAddr,
}

impl Default for GrpcApiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: DEFAULT_LISTEN
                .parse()
                .expect("valid default grpc listen addr"),
        }
    }
}

// ---------------------------------------------------------------------------
// Request types (matching planned proto messages)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct StartPeeringRequest {
    port: u16,
    pin: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AcceptPeeringRequest {
    request_id: String,
}

#[derive(Debug, Deserialize)]
struct RejectPeeringRequest {
    request_id: String,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RemovePeerRequest {
    name_or_key: String,
}

#[derive(Debug, Deserialize)]
struct UpdatePeerEndpointRequest {
    name_or_key: String,
    endpoint: String,
}

// ---------------------------------------------------------------------------
// Response types (matching planned proto messages)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct StatusResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Serialize)]
struct PeeringRequestInfo {
    request_id: String,
    node_name: String,
    wg_public_key: String,
    endpoint: String,
    wg_listen_port: u16,
    received_at: u64,
    region: Option<String>,
    zone: Option<String>,
}

#[derive(Debug, Serialize)]
struct ListPeeringRequestsResponse {
    requests: Vec<PeeringRequestInfo>,
}

#[derive(Debug, Serialize)]
struct AcceptPeeringResponse {
    peer_name: String,
}

#[derive(Debug, Serialize)]
struct RemovePeerResponse {
    peer_name: String,
    announced_to: usize,
}

#[derive(Debug, Serialize)]
struct UpdatePeerEndpointResponse {
    peer_name: String,
    old_endpoint: String,
    new_endpoint: String,
}

#[derive(Debug, Serialize)]
struct ReloadResponse {
    changes: Vec<String>,
    skipped: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RotateSecretResponse {
    new_secret: String,
    new_ipv6: String,
    peers_notified: usize,
    peers_failed: usize,
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// Shared handler passed to all route handlers via axum state.
type SharedHandler = Arc<dyn FabricHandler>;

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the axum [`Router`] for the fabric gRPC-compatible API.
pub fn router(handler: SharedHandler) -> Router {
    Router::new()
        .route("/v1/fabric/peering/start", post(start_peering))
        .route("/v1/fabric/peering/stop", post(stop_peering))
        .route("/v1/fabric/peering/requests", get(list_peering_requests))
        .route("/v1/fabric/peering/accept", post(accept_peering))
        .route("/v1/fabric/peering/reject", post(reject_peering))
        .route("/v1/fabric/peers/remove", post(remove_peer))
        .route(
            "/v1/fabric/peers/update-endpoint",
            post(update_peer_endpoint),
        )
        .route("/v1/fabric/reload", post(reload))
        .route("/v1/fabric/rotate-secret", post(rotate_secret))
        .route("/v1/fabric/status", get(status))
        .with_state(handler)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Convert a [`FabricResponse`] to an axum-compatible response.
fn fabric_response_to_axum(
    resp: FabricResponse,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    match resp {
        FabricResponse::Ok => Ok(Json(serde_json::json!({"ok": true})).into_response()),
        FabricResponse::Error { message } => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse { error: message }),
        )),
        FabricResponse::PeeringList { requests } => {
            let items: Vec<PeeringRequestInfo> = requests
                .into_iter()
                .map(|r| PeeringRequestInfo {
                    request_id: r.request_id,
                    node_name: r.node_name,
                    wg_public_key: r.wg_public_key,
                    endpoint: r.endpoint.to_string(),
                    wg_listen_port: r.wg_listen_port,
                    received_at: r.received_at,
                    region: r.region,
                    zone: r.zone,
                })
                .collect();
            Ok(Json(ListPeeringRequestsResponse { requests: items }).into_response())
        }
        FabricResponse::PeeringAccepted { peer_name } => {
            Ok(Json(AcceptPeeringResponse { peer_name }).into_response())
        }
        FabricResponse::PeerRemoved {
            peer_name,
            announced_to,
        } => Ok(Json(RemovePeerResponse {
            peer_name,
            announced_to,
        })
        .into_response()),
        FabricResponse::PeerEndpointUpdated {
            peer_name,
            old_endpoint,
            new_endpoint,
        } => Ok(Json(UpdatePeerEndpointResponse {
            peer_name,
            old_endpoint,
            new_endpoint,
        })
        .into_response()),
        FabricResponse::ConfigReloaded { changes, skipped } => {
            Ok(Json(ReloadResponse { changes, skipped }).into_response())
        }
        FabricResponse::SecretRotated {
            new_secret,
            new_ipv6,
            peers_notified,
            peers_failed,
        } => Ok(Json(RotateSecretResponse {
            new_secret,
            new_ipv6,
            peers_notified,
            peers_failed,
        })
        .into_response()),
    }
}

async fn start_peering(
    axum::extract::State(handler): axum::extract::State<SharedHandler>,
    Json(req): Json<StartPeeringRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let fabric_req = FabricRequest::PeeringStart {
        port: req.port,
        pin: req.pin,
    };
    let resp = handler.handle(fabric_req, None).await;
    fabric_response_to_axum(resp)
}

async fn stop_peering(
    axum::extract::State(handler): axum::extract::State<SharedHandler>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let resp = handler.handle(FabricRequest::PeeringStop, None).await;
    fabric_response_to_axum(resp)
}

async fn list_peering_requests(
    axum::extract::State(handler): axum::extract::State<SharedHandler>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let resp = handler.handle(FabricRequest::PeeringList, None).await;
    fabric_response_to_axum(resp)
}

async fn accept_peering(
    axum::extract::State(handler): axum::extract::State<SharedHandler>,
    Json(req): Json<AcceptPeeringRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let fabric_req = FabricRequest::PeeringAccept {
        request_id: req.request_id,
    };
    let resp = handler.handle(fabric_req, None).await;
    fabric_response_to_axum(resp)
}

async fn reject_peering(
    axum::extract::State(handler): axum::extract::State<SharedHandler>,
    Json(req): Json<RejectPeeringRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let fabric_req = FabricRequest::PeeringReject {
        request_id: req.request_id,
        reason: req.reason,
    };
    let resp = handler.handle(fabric_req, None).await;
    fabric_response_to_axum(resp)
}

async fn remove_peer(
    axum::extract::State(handler): axum::extract::State<SharedHandler>,
    Json(req): Json<RemovePeerRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let fabric_req = FabricRequest::RemovePeer {
        name_or_key: req.name_or_key,
    };
    let resp = handler.handle(fabric_req, None).await;
    fabric_response_to_axum(resp)
}

async fn update_peer_endpoint(
    axum::extract::State(handler): axum::extract::State<SharedHandler>,
    Json(req): Json<UpdatePeerEndpointRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let endpoint: std::net::SocketAddr = req.endpoint.parse().map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("invalid endpoint address: {e}"),
            }),
        )
    })?;
    let fabric_req = FabricRequest::UpdatePeerEndpoint {
        name_or_key: req.name_or_key,
        endpoint,
    };
    let resp = handler.handle(fabric_req, None).await;
    fabric_response_to_axum(resp)
}

async fn reload(
    axum::extract::State(handler): axum::extract::State<SharedHandler>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let resp = handler.handle(FabricRequest::Reload, None).await;
    fabric_response_to_axum(resp)
}

async fn rotate_secret(
    axum::extract::State(handler): axum::extract::State<SharedHandler>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let resp = handler.handle(FabricRequest::RotateSecret, None).await;
    fabric_response_to_axum(resp)
}

async fn status() -> impl IntoResponse {
    Json(StatusResponse { status: "ok" })
}

// ---------------------------------------------------------------------------
// Server lifecycle
// ---------------------------------------------------------------------------

/// Start the gRPC-compatible API server.
///
/// Returns immediately if the API is disabled. Runs until the provided
/// `shutdown` receiver signals `true`.
pub async fn serve(
    config: GrpcApiConfig,
    handler: SharedHandler,
    shutdown: tokio::sync::watch::Receiver<bool>,
) {
    if !config.enabled {
        tracing::debug!("gRPC API disabled, skipping");
        std::future::pending::<()>().await;
        return;
    }

    let app = router(handler);

    let listener = match TcpListener::bind(config.listen).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("gRPC API failed to bind to {}: {e}", config.listen);
            return;
        }
    };

    tracing::info!("gRPC API listening on {}", config.listen);

    let mut rx = shutdown;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            while !*rx.borrow_and_update() {
                if rx.changed().await.is_err() {
                    break;
                }
            }
        })
        .await
        .unwrap_or_else(|e| tracing::error!("gRPC API server error: {e}"));
}

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

/// Deserializable `[grpc]` section of `config.toml`.
#[derive(Debug, Deserialize, Default)]
struct GrpcSection {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    listen: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ConfigFileGrpc {
    #[serde(default)]
    grpc: GrpcSection,
}

/// Load [`GrpcApiConfig`] from `~/.syfrah/config.toml`.
///
/// Returns the default (disabled) config if the file does not exist or
/// has no `[grpc]` section.
pub fn load_grpc_config() -> GrpcApiConfig {
    let path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".syfrah")
        .join("config.toml");

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return GrpcApiConfig::default(),
    };

    let file: ConfigFileGrpc = match toml::from_str(&content) {
        Ok(f) => f,
        Err(_) => return GrpcApiConfig::default(),
    };

    let defaults = GrpcApiConfig::default();
    let listen = file
        .grpc
        .listen
        .and_then(|s| s.parse().ok())
        .unwrap_or(defaults.listen);

    GrpcApiConfig {
        enabled: file.grpc.enabled.unwrap_or(false),
        listen,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// Stub handler that returns predictable responses for testing.
    struct StubFabricHandler;

    #[async_trait::async_trait]
    impl FabricHandler for StubFabricHandler {
        async fn handle(&self, req: FabricRequest, _caller_uid: Option<u32>) -> FabricResponse {
            match req {
                FabricRequest::PeeringStart { .. } => FabricResponse::Ok,
                FabricRequest::PeeringStop => FabricResponse::Ok,
                FabricRequest::PeeringList => FabricResponse::PeeringList { requests: vec![] },
                FabricRequest::PeeringAccept { request_id } => FabricResponse::PeeringAccepted {
                    peer_name: format!("peer-{request_id}"),
                },
                FabricRequest::PeeringReject { .. } => FabricResponse::Ok,
                FabricRequest::RemovePeer { name_or_key } => FabricResponse::PeerRemoved {
                    peer_name: name_or_key,
                    announced_to: 3,
                },
                FabricRequest::Reload => FabricResponse::ConfigReloaded {
                    changes: vec!["keepalive".into()],
                    skipped: vec![],
                },
                FabricRequest::UpdatePeerEndpoint {
                    name_or_key,
                    endpoint,
                } => FabricResponse::PeerEndpointUpdated {
                    peer_name: name_or_key,
                    old_endpoint: "1.2.3.4:51820".into(),
                    new_endpoint: endpoint.to_string(),
                },
                FabricRequest::RotateSecret => FabricResponse::SecretRotated {
                    new_secret: "new-secret".into(),
                    new_ipv6: "fd00::1".into(),
                    peers_notified: 5,
                    peers_failed: 0,
                },
            }
        }
    }

    fn test_handler() -> SharedHandler {
        Arc::new(StubFabricHandler)
    }

    async fn send_request(
        app: Router,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, String) {
        let body = match body {
            Some(v) => Body::from(serde_json::to_vec(&v).unwrap()),
            None => Body::empty(),
        };
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(body)
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&bytes).to_string())
    }

    #[tokio::test]
    async fn status_returns_ok() {
        let app = router(test_handler());
        let (status, body) = send_request(app, "GET", "/v1/fabric/status", None).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["status"], "ok");
    }

    #[tokio::test]
    async fn start_peering_returns_ok() {
        let app = router(test_handler());
        let (status, body) = send_request(
            app,
            "POST",
            "/v1/fabric/peering/start",
            Some(serde_json::json!({"port": 7946, "pin": "1234"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["ok"], true);
    }

    #[tokio::test]
    async fn stop_peering_returns_ok() {
        let app = router(test_handler());
        let (status, _) = send_request(app, "POST", "/v1/fabric/peering/stop", None).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_peering_requests_returns_empty() {
        let app = router(test_handler());
        let (status, body) = send_request(app, "GET", "/v1/fabric/peering/requests", None).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["requests"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn accept_peering_returns_peer_name() {
        let app = router(test_handler());
        let (status, body) = send_request(
            app,
            "POST",
            "/v1/fabric/peering/accept",
            Some(serde_json::json!({"request_id": "abc"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["peer_name"], "peer-abc");
    }

    #[tokio::test]
    async fn reject_peering_returns_ok() {
        let app = router(test_handler());
        let (status, _) = send_request(
            app,
            "POST",
            "/v1/fabric/peering/reject",
            Some(serde_json::json!({"request_id": "abc"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn remove_peer_returns_details() {
        let app = router(test_handler());
        let (status, body) = send_request(
            app,
            "POST",
            "/v1/fabric/peers/remove",
            Some(serde_json::json!({"name_or_key": "node-a"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["peer_name"], "node-a");
        assert_eq!(v["announced_to"], 3);
    }

    #[tokio::test]
    async fn update_peer_endpoint_returns_details() {
        let app = router(test_handler());
        let (status, body) = send_request(
            app,
            "POST",
            "/v1/fabric/peers/update-endpoint",
            Some(serde_json::json!({
                "name_or_key": "node-b",
                "endpoint": "10.0.0.1:51820"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["peer_name"], "node-b");
        assert_eq!(v["new_endpoint"], "10.0.0.1:51820");
    }

    #[tokio::test]
    async fn update_peer_endpoint_invalid_addr() {
        let app = router(test_handler());
        let (status, body) = send_request(
            app,
            "POST",
            "/v1/fabric/peers/update-endpoint",
            Some(serde_json::json!({
                "name_or_key": "node-b",
                "endpoint": "not-an-address"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["error"].as_str().unwrap().contains("invalid endpoint"));
    }

    #[tokio::test]
    async fn reload_returns_changes() {
        let app = router(test_handler());
        let (status, body) = send_request(app, "POST", "/v1/fabric/reload", None).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["changes"][0], "keepalive");
    }

    #[tokio::test]
    async fn rotate_secret_returns_details() {
        let app = router(test_handler());
        let (status, body) = send_request(app, "POST", "/v1/fabric/rotate-secret", None).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["peers_notified"], 5);
        assert_eq!(v["peers_failed"], 0);
    }

    #[test]
    fn default_grpc_config_is_disabled() {
        let cfg = GrpcApiConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.listen.to_string(), "0.0.0.0:8443");
    }
}
