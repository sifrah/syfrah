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
//! - `GET  /v1/fabric/peers`                  — ListPeers
//! - `GET  /v1/fabric/topology`               — GetTopology
//! - `GET  /v1/fabric/events`                 — GetEvents
//! - `GET  /v1/fabric/audit`                  — GetAudit
//! - `GET  /v1/fabric/metrics`                — GetMetrics
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

use crate::auth_middleware::{self, SharedValidator, StubApiKeyValidator};
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
// Response types for new read-only endpoints
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct PeerInfo {
    name: String,
    public_key: String,
    endpoint: String,
    ipv6_address: String,
    status: String,
    last_handshake: u64,
    region: Option<String>,
    zone: Option<String>,
}

#[derive(Debug, Serialize)]
struct ListPeersResponse {
    peers: Vec<PeerInfo>,
}

#[derive(Debug, Serialize)]
struct TopologyEdge {
    from_peer: String,
    to_peer: String,
    latency_us: u64,
}

#[derive(Debug, Serialize)]
struct GetTopologyResponse {
    peers: Vec<PeerInfo>,
    edges: Vec<TopologyEdge>,
}

#[derive(Debug, Serialize)]
struct FabricEventInfo {
    id: String,
    kind: String,
    message: String,
    timestamp: u64,
}

#[derive(Debug, Serialize)]
struct GetEventsResponse {
    events: Vec<FabricEventInfo>,
}

#[derive(Debug, Serialize)]
struct AuditEntryInfo {
    id: String,
    action: String,
    actor: String,
    details: String,
    timestamp: u64,
}

#[derive(Debug, Serialize)]
struct GetAuditResponse {
    entries: Vec<AuditEntryInfo>,
}

#[derive(Debug, Serialize)]
struct GetMetricsResponse {
    peer_count: u32,
    bytes_sent: u64,
    bytes_received: u64,
    handshakes_completed: u32,
    handshakes_failed: u32,
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
///
/// When `validator` is provided the auth middleware layer is applied to all
/// routes, requiring a valid `Authorization: Bearer syf_key_...` header.
pub fn router(handler: SharedHandler) -> Router {
    router_with_auth(
        handler,
        Some(Arc::new(StubApiKeyValidator) as SharedValidator),
    )
}

/// Build the router with an explicit auth validator (or `None` to skip auth).
pub fn router_with_auth(handler: SharedHandler, validator: Option<SharedValidator>) -> Router {
    let base = Router::new()
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
        .route("/v1/fabric/peers", get(list_peers))
        .route("/v1/fabric/topology", get(get_topology))
        .route("/v1/fabric/events", get(get_events))
        .route("/v1/fabric/audit", get(get_audit))
        .route("/v1/fabric/metrics", get(get_metrics));

    match validator {
        Some(v) => auth_middleware::with_auth_layer(base, v).with_state(handler),
        None => base.with_state(handler),
    }
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
// Read-only handlers (store/events/audit/metrics)
// ---------------------------------------------------------------------------

fn peer_record_to_info(p: &syfrah_core::mesh::PeerRecord) -> PeerInfo {
    PeerInfo {
        name: p.name.clone(),
        public_key: p.wg_public_key.clone(),
        endpoint: p.endpoint.to_string(),
        ipv6_address: p.mesh_ipv6.to_string(),
        status: format!("{:?}", p.status),
        last_handshake: p.last_seen,
        region: p.region.clone(),
        zone: p.zone.clone(),
    }
}

async fn list_peers() -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let peers = crate::store::get_peers().map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("store unavailable: {e}"),
            }),
        )
    })?;
    let mut infos: Vec<PeerInfo> = peers.iter().map(peer_record_to_info).collect();
    infos.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(ListPeersResponse { peers: infos }))
}

async fn get_topology() -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let peers = crate::store::get_peers().map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("store unavailable: {e}"),
            }),
        )
    })?;
    let mut infos: Vec<PeerInfo> = peers.iter().map(peer_record_to_info).collect();
    infos.sort_by(|a, b| a.name.cmp(&b.name));

    // Build full-mesh edges between all active peers.
    let active_names: Vec<&str> = peers
        .iter()
        .filter(|p| p.status == syfrah_core::mesh::PeerStatus::Active)
        .map(|p| p.name.as_str())
        .collect();
    let mut edges = Vec::new();
    for i in 0..active_names.len() {
        for j in (i + 1)..active_names.len() {
            edges.push(TopologyEdge {
                from_peer: active_names[i].to_string(),
                to_peer: active_names[j].to_string(),
                latency_us: 0,
            });
        }
    }

    Ok(Json(GetTopologyResponse {
        peers: infos,
        edges,
    }))
}

async fn get_events() -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let events = crate::events::list_events().map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("events unavailable: {e}"),
            }),
        )
    })?;
    let items: Vec<FabricEventInfo> = events
        .iter()
        .enumerate()
        .map(|(i, e)| FabricEventInfo {
            id: format!("{}", i + 1),
            kind: e.event_type.to_string(),
            message: e
                .details
                .clone()
                .unwrap_or_else(|| e.event_type.to_string()),
            timestamp: e.timestamp,
        })
        .collect();
    Ok(Json(GetEventsResponse { events: items }))
}

async fn get_audit() -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let entries = crate::audit::read_entries().map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("audit log unavailable: {e}"),
            }),
        )
    })?;
    let items: Vec<AuditEntryInfo> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| AuditEntryInfo {
            id: format!("{}", i + 1),
            action: e.event_type.clone(),
            actor: e
                .caller_uid
                .map(|uid| format!("uid:{uid}"))
                .unwrap_or_else(|| "system".to_string()),
            details: e.details.clone().unwrap_or_default(),
            timestamp: e.timestamp,
        })
        .collect();
    Ok(Json(GetAuditResponse { entries: items }))
}

async fn get_metrics() -> impl IntoResponse {
    let peer_count = crate::store::peer_count().unwrap_or(0) as u32;
    let bytes_sent = crate::store::inc_metric("bytes_sent", 0).unwrap_or(0);
    let bytes_received = crate::store::inc_metric("bytes_received", 0).unwrap_or(0);
    let handshakes_completed = crate::store::inc_metric("peers_discovered", 0).unwrap_or(0) as u32;
    let handshakes_failed = crate::store::inc_metric("announcements_failed", 0).unwrap_or(0) as u32;
    Json(GetMetricsResponse {
        peer_count,
        bytes_sent,
        bytes_received,
        handshakes_completed,
        handshakes_failed,
    })
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

/// Start the gRPC-compatible API server with TLS (gateway mode).
///
/// Binds to the given address and terminates TLS using the provided
/// certificate and key PEM bytes.  Returns immediately if the gateway is
/// disabled.
pub async fn serve_gateway_tls(
    gateway: crate::config::GatewayConfig,
    handler: SharedHandler,
    shutdown: tokio::sync::watch::Receiver<bool>,
) {
    if !gateway.enabled {
        tracing::debug!("gateway disabled, skipping TLS API server");
        std::future::pending::<()>().await;
        return;
    }

    // Resolve TLS material (operator-provided or self-signed).
    let (cert_pem, key_pem) = match crate::config::resolve_gateway_tls(
        gateway.tls_cert_path.as_deref(),
        gateway.tls_key_path.as_deref(),
    ) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::error!("gateway TLS setup failed: {e}");
            return;
        }
    };

    let certs = match rustls_pemfile::certs(&mut &cert_pem[..]).collect::<Result<Vec<_>, _>>() {
        Ok(c) if !c.is_empty() => c,
        Ok(_) => {
            tracing::error!("gateway TLS cert file contains no certificates");
            return;
        }
        Err(e) => {
            tracing::error!("failed to parse gateway TLS cert: {e}");
            return;
        }
    };

    let key = match rustls_pemfile::private_key(&mut &key_pem[..]) {
        Ok(Some(k)) => k,
        Ok(None) => {
            tracing::error!("gateway TLS key file contains no private key");
            return;
        }
        Err(e) => {
            tracing::error!("failed to parse gateway TLS key: {e}");
            return;
        }
    };

    let tls_config = match rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
    {
        Ok(c) => std::sync::Arc::new(c),
        Err(e) => {
            tracing::error!("gateway TLS config error: {e}");
            return;
        }
    };
    let tls_acceptor = tokio_rustls::TlsAcceptor::from(tls_config);

    let listener = match TcpListener::bind(gateway.bind_address).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(
                "gateway API failed to bind to {}: {e}",
                gateway.bind_address
            );
            return;
        }
    };

    tracing::info!("gateway API listening on {} (TLS)", gateway.bind_address);

    let app = router(handler);
    let mut rx = shutdown;

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _addr) = match accepted {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("gateway accept error: {e}");
                        continue;
                    }
                };
                let acceptor = tls_acceptor.clone();
                let app = app.clone();
                tokio::spawn(async move {
                    let tls_stream = match acceptor.accept(stream).await {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::debug!("gateway TLS handshake failed: {e}");
                            return;
                        }
                    };
                    let io = hyper_util::rt::TokioIo::new(tls_stream);
                    let service = hyper_util::service::TowerToHyperService::new(app);
                    if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                        hyper_util::rt::TokioExecutor::new(),
                    )
                    .serve_connection(io, service)
                    .await
                    {
                        tracing::debug!("gateway connection error: {e}");
                    }
                });
            }
            _ = async {
                while !*rx.borrow_and_update() {
                    if rx.changed().await.is_err() {
                        break;
                    }
                }
            } => {
                tracing::info!("gateway API shutting down");
                break;
            }
        }
    }
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

    /// Build router without auth for existing handler tests.
    fn test_router() -> Router {
        router_with_auth(test_handler(), None)
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
        let app = test_router();
        let (status, body) = send_request(app, "GET", "/v1/fabric/status", None).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["status"], "ok");
    }

    #[tokio::test]
    async fn start_peering_returns_ok() {
        let app = test_router();
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
        let app = test_router();
        let (status, _) = send_request(app, "POST", "/v1/fabric/peering/stop", None).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_peering_requests_returns_empty() {
        let app = test_router();
        let (status, body) = send_request(app, "GET", "/v1/fabric/peering/requests", None).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["requests"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn accept_peering_returns_peer_name() {
        let app = test_router();
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
        let app = test_router();
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
        let app = test_router();
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
        let app = test_router();
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
        let app = test_router();
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
        let app = test_router();
        let (status, body) = send_request(app, "POST", "/v1/fabric/reload", None).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["changes"][0], "keepalive");
    }

    #[tokio::test]
    async fn rotate_secret_returns_details() {
        let app = test_router();
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

    // ----- Phase 2.4: additional coverage -----

    #[tokio::test]
    async fn list_peering_requests_returns_json_array() {
        let app = test_router();
        let (status, body) = send_request(app, "GET", "/v1/fabric/peering/requests", None).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v.is_object(), "top-level response should be a JSON object");
        let requests = &v["requests"];
        assert!(requests.is_array(), "requests field should be a JSON array");
    }

    #[tokio::test]
    async fn status_returns_json_object() {
        let app = test_router();
        let (status, body) = send_request(app, "GET", "/v1/fabric/status", None).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v.is_object(), "status response should be a JSON object");
    }

    #[tokio::test]
    async fn proto_field_names_match_status_response() {
        let app = test_router();
        let (_, body) = send_request(app, "GET", "/v1/fabric/status", None).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let obj = v.as_object().unwrap();
        assert!(obj.contains_key("status"), "missing proto field: status");
    }

    #[tokio::test]
    async fn proto_field_names_match_accept_peering_response() {
        let app = test_router();
        let (_, body) = send_request(
            app,
            "POST",
            "/v1/fabric/peering/accept",
            Some(serde_json::json!({"request_id": "x"})),
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let obj = v.as_object().unwrap();
        assert!(
            obj.contains_key("peer_name"),
            "missing proto field: peer_name"
        );
    }

    #[tokio::test]
    async fn proto_field_names_match_remove_peer_response() {
        let app = test_router();
        let (_, body) = send_request(
            app,
            "POST",
            "/v1/fabric/peers/remove",
            Some(serde_json::json!({"name_or_key": "n"})),
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let obj = v.as_object().unwrap();
        assert!(
            obj.contains_key("peer_name"),
            "missing proto field: peer_name"
        );
        assert!(
            obj.contains_key("announced_to"),
            "missing proto field: announced_to"
        );
    }

    #[tokio::test]
    async fn proto_field_names_match_update_endpoint_response() {
        let app = test_router();
        let (_, body) = send_request(
            app,
            "POST",
            "/v1/fabric/peers/update-endpoint",
            Some(serde_json::json!({"name_or_key": "n", "endpoint": "1.2.3.4:51820"})),
        )
        .await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let obj = v.as_object().unwrap();
        assert!(
            obj.contains_key("peer_name"),
            "missing proto field: peer_name"
        );
        assert!(
            obj.contains_key("old_endpoint"),
            "missing proto field: old_endpoint"
        );
        assert!(
            obj.contains_key("new_endpoint"),
            "missing proto field: new_endpoint"
        );
    }

    #[tokio::test]
    async fn proto_field_names_match_reload_response() {
        let app = test_router();
        let (_, body) = send_request(app, "POST", "/v1/fabric/reload", None).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let obj = v.as_object().unwrap();
        assert!(obj.contains_key("changes"), "missing proto field: changes");
        assert!(obj.contains_key("skipped"), "missing proto field: skipped");
    }

    #[tokio::test]
    async fn proto_field_names_match_rotate_secret_response() {
        let app = test_router();
        let (_, body) = send_request(app, "POST", "/v1/fabric/rotate-secret", None).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let obj = v.as_object().unwrap();
        assert!(
            obj.contains_key("new_secret"),
            "missing proto field: new_secret"
        );
        assert!(
            obj.contains_key("new_ipv6"),
            "missing proto field: new_ipv6"
        );
        assert!(
            obj.contains_key("peers_notified"),
            "missing proto field: peers_notified"
        );
        assert!(
            obj.contains_key("peers_failed"),
            "missing proto field: peers_failed"
        );
    }

    #[tokio::test]
    async fn invalid_endpoint_returns_404() {
        let app = test_router();
        let req = Request::builder()
            .method("GET")
            .uri("/v1/fabric/nonexistent")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn post_without_json_content_type_is_rejected() {
        let app = test_router();
        let req = Request::builder()
            .method("POST")
            .uri("/v1/fabric/peering/start")
            .header("content-type", "text/plain")
            .body(Body::from(r#"{"port":7946}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // Axum rejects non-JSON content-type with 415 Unsupported Media Type
        assert_eq!(
            resp.status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "non-JSON content-type should be rejected"
        );
    }

    #[tokio::test]
    async fn health_status_always_returns_200() {
        // Call the status endpoint multiple times to confirm it always returns 200.
        for _ in 0..3 {
            let app = test_router();
            let req = Request::builder()
                .method("GET")
                .uri("/v1/fabric/status")
                .body(Body::empty())
                .unwrap();
            let resp = app.oneshot(req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }
    }

    // ----- Read-only endpoint tests -----

    #[tokio::test]
    async fn list_peers_returns_json_array() {
        let app = test_router();
        let (status, body) = send_request(app, "GET", "/v1/fabric/peers", None).await;
        // Without a store this may return SERVICE_UNAVAILABLE or OK with empty list.
        assert!(
            status == StatusCode::OK || status == StatusCode::SERVICE_UNAVAILABLE,
            "unexpected status: {status}"
        );
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        if status == StatusCode::OK {
            assert!(v["peers"].is_array(), "peers field should be a JSON array");
        }
    }

    #[tokio::test]
    async fn get_topology_returns_peers_and_edges() {
        let app = test_router();
        let (status, body) = send_request(app, "GET", "/v1/fabric/topology", None).await;
        assert!(
            status == StatusCode::OK || status == StatusCode::SERVICE_UNAVAILABLE,
            "unexpected status: {status}"
        );
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        if status == StatusCode::OK {
            assert!(v["peers"].is_array(), "peers field should be a JSON array");
            assert!(v["edges"].is_array(), "edges field should be a JSON array");
        }
    }

    #[tokio::test]
    async fn get_events_returns_json_array() {
        let app = test_router();
        let (status, body) = send_request(app, "GET", "/v1/fabric/events", None).await;
        assert!(
            status == StatusCode::OK || status == StatusCode::SERVICE_UNAVAILABLE,
            "unexpected status: {status}"
        );
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        if status == StatusCode::OK {
            assert!(
                v["events"].is_array(),
                "events field should be a JSON array"
            );
        }
    }

    #[tokio::test]
    async fn get_audit_returns_json_array() {
        let app = test_router();
        let (status, body) = send_request(app, "GET", "/v1/fabric/audit", None).await;
        assert!(
            status == StatusCode::OK || status == StatusCode::SERVICE_UNAVAILABLE,
            "unexpected status: {status}"
        );
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        if status == StatusCode::OK {
            assert!(
                v["entries"].is_array(),
                "entries field should be a JSON array"
            );
        }
    }

    #[tokio::test]
    async fn get_metrics_returns_structured_json() {
        let app = test_router();
        let (status, body) = send_request(app, "GET", "/v1/fabric/metrics", None).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let obj = v.as_object().unwrap();
        assert!(
            obj.contains_key("peer_count"),
            "missing proto field: peer_count"
        );
        assert!(
            obj.contains_key("bytes_sent"),
            "missing proto field: bytes_sent"
        );
        assert!(
            obj.contains_key("bytes_received"),
            "missing proto field: bytes_received"
        );
        assert!(
            obj.contains_key("handshakes_completed"),
            "missing proto field: handshakes_completed"
        );
        assert!(
            obj.contains_key("handshakes_failed"),
            "missing proto field: handshakes_failed"
        );
    }

    // ----- Auth middleware integration tests -----

    /// Build router WITH auth enabled (using the stub validator).
    fn authed_router() -> Router {
        router_with_auth(
            test_handler(),
            Some(Arc::new(auth_middleware::StubApiKeyValidator) as auth_middleware::SharedValidator),
        )
    }

    fn authed_request(
        method: &str,
        uri: &str,
        auth: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> Request<Body> {
        let body = match body {
            Some(v) => Body::from(serde_json::to_vec(&v).unwrap()),
            None => Body::empty(),
        };
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(token) = auth {
            builder = builder.header("authorization", token);
        }
        builder.body(body).unwrap()
    }

    #[tokio::test]
    async fn authed_router_rejects_without_token() {
        let app = authed_router();
        let req = authed_request("GET", "/v1/fabric/status", None, None);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn authed_router_accepts_valid_token() {
        let app = authed_router();
        let req = authed_request(
            "GET",
            "/v1/fabric/status",
            Some("Bearer syf_key_testtoken123"),
            None,
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn authed_router_rejects_bad_prefix() {
        let app = authed_router();
        let req = authed_request(
            "GET",
            "/v1/fabric/status",
            Some("Bearer bad_prefix_token"),
            None,
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn authed_router_allows_post_with_token() {
        let app = authed_router();
        let req = authed_request(
            "POST",
            "/v1/fabric/peering/start",
            Some("Bearer syf_key_testtoken123"),
            Some(serde_json::json!({"port": 7946})),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
