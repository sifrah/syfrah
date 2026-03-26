//! Lightweight HTTP REST API for topology queries.
//!
//! Disabled by default. Enable via `[api]` section in `~/.syfrah/config.toml`:
//!
//! ```toml
//! [api]
//! enabled = true
//! listen = "127.0.0.1:9100"
//! ```
//!
//! Endpoints:
//! - `GET /v1/topology/regions`         — list all regions
//! - `GET /v1/topology/regions/{r}`     — region detail + zones
//! - `GET /v1/topology/zones/{z}/peers` — peers in zone
//! - `GET /v1/topology/health`          — zone health status
//! - `GET /v1/peers`                    — all peers
//! - `GET /v1/health`                   — simple health check

use std::net::SocketAddr;

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use syfrah_core::mesh::{PeerStatus, Region, Zone};
use tokio::net::TcpListener;

use crate::topology::TopologyView;

/// Default listen address (localhost-only for security).
const DEFAULT_LISTEN: &str = "127.0.0.1:9100";

/// Configuration for the HTTP API server.
#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// Whether the API server is enabled.
    pub enabled: bool,
    /// Socket address to bind to.
    pub listen: SocketAddr,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen: DEFAULT_LISTEN.parse().expect("valid default listen addr"),
        }
    }
}

/// Start the HTTP API server. Returns immediately if the API is disabled.
///
/// This function runs until the provided `shutdown` future resolves (typically
/// a cancellation token or ctrl-c signal).
pub async fn serve(config: ApiConfig, shutdown: tokio::sync::watch::Receiver<bool>) {
    if !config.enabled {
        tracing::debug!("HTTP API disabled, skipping");
        return;
    }

    let app = router();

    let listener = match TcpListener::bind(config.listen).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("HTTP API failed to bind to {}: {e}", config.listen);
            return;
        }
    };

    tracing::info!("HTTP API listening on {}", config.listen);

    let mut rx = shutdown;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            // Wait until the shutdown flag is set to true.
            while !*rx.borrow_and_update() {
                if rx.changed().await.is_err() {
                    break;
                }
            }
        })
        .await
        .unwrap_or_else(|e| tracing::error!("HTTP API server error: {e}"));
}

/// Build the axum [`Router`] with all topology endpoints.
pub fn router() -> Router {
    Router::new()
        .route("/v1/topology/regions", get(list_regions))
        .route("/v1/topology/regions/{region}", get(region_detail))
        .route("/v1/topology/zones/{zone}/peers", get(zone_peers))
        .route("/v1/topology/health", get(topology_health))
        .route("/v1/peers", get(list_peers))
        .route("/v1/health", get(health))
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct RegionListResponse {
    regions: Vec<String>,
}

#[derive(Serialize)]
struct RegionDetailResponse {
    region: String,
    zones: Vec<String>,
    peer_count: usize,
    active_count: usize,
}

#[derive(Serialize)]
struct ZonePeersResponse {
    zone: String,
    peers: Vec<PeerSummary>,
}

#[derive(Serialize)]
struct PeerSummary {
    name: String,
    endpoint: String,
    mesh_ipv6: String,
    status: String,
    region: Option<String>,
    zone: Option<String>,
}

#[derive(Serialize)]
struct ZoneHealth {
    zone: String,
    region: String,
    total: usize,
    active: usize,
    unreachable: usize,
}

#[derive(Serialize)]
struct TopologyHealthResponse {
    zones: Vec<ZoneHealth>,
}

#[derive(Serialize)]
struct PeerListResponse {
    peers: Vec<PeerSummary>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    peer_count: usize,
    active_count: usize,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn load_view() -> Result<TopologyView, (StatusCode, Json<serde_json::Value>)> {
    TopologyView::snapshot().map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": format!("store unavailable: {e}") })),
        )
    })
}

fn peer_to_summary(p: &syfrah_core::mesh::PeerRecord) -> PeerSummary {
    PeerSummary {
        name: p.name.clone(),
        endpoint: p.endpoint.to_string(),
        mesh_ipv6: p.mesh_ipv6.to_string(),
        status: format!("{:?}", p.status),
        region: p.region.clone(),
        zone: p.zone.clone(),
    }
}

async fn list_regions() -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let view = load_view()?;
    let mut regions: Vec<String> = view
        .regions()
        .into_iter()
        .map(|r| r.as_str().to_owned())
        .collect();
    regions.sort();
    Ok(Json(RegionListResponse { regions }))
}

async fn region_detail(
    Path(region): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let r = Region::new(&region).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid region name" })),
        )
    })?;

    let view = load_view()?;
    let peers = view.peers_in_region(&r);

    if peers.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "region not found" })),
        ));
    }

    let mut zones: Vec<String> = view
        .zones_in_region(&r)
        .into_iter()
        .map(|z| z.as_str().to_owned())
        .collect();
    zones.sort();

    Ok(Json(RegionDetailResponse {
        region,
        zones,
        peer_count: peers.len(),
        active_count: view.active_count_in_region(&r),
    }))
}

async fn zone_peers(
    Path(zone): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let z = Zone::new(&zone).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid zone name" })),
        )
    })?;

    let view = load_view()?;
    let peers: Vec<PeerSummary> = view.peers_in_zone(&z).iter().map(peer_to_summary).collect();

    Ok(Json(ZonePeersResponse { zone, peers }))
}

async fn topology_health() -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let view = load_view()?;

    let mut zones = Vec::new();
    for region in view.regions() {
        for zone in view.zones_in_region(region) {
            let peers = view.peers_in_zone(zone);
            let active = peers
                .iter()
                .filter(|p| p.status == PeerStatus::Active)
                .count();
            let unreachable = peers
                .iter()
                .filter(|p| p.status == PeerStatus::Unreachable)
                .count();

            zones.push(ZoneHealth {
                zone: zone.as_str().to_owned(),
                region: region.as_str().to_owned(),
                total: peers.len(),
                active,
                unreachable,
            });
        }
    }

    zones.sort_by(|a, b| (&a.region, &a.zone).cmp(&(&b.region, &b.zone)));

    Ok(Json(TopologyHealthResponse { zones }))
}

async fn list_peers() -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let view = load_view()?;

    let mut peers = Vec::new();
    for region in view.regions() {
        for p in view.peers_in_region(region) {
            peers.push(peer_to_summary(p));
        }
    }

    peers.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Json(PeerListResponse { peers }))
}

async fn health() -> impl IntoResponse {
    let (peer_count, active_count) = match crate::store::load() {
        Ok(state) => {
            let total = state.peers.len();
            let active = state
                .peers
                .iter()
                .filter(|p| p.status == PeerStatus::Active)
                .count();
            (total, active)
        }
        Err(_) => (0, 0),
    };

    Json(HealthResponse {
        status: "ok",
        peer_count,
        active_count,
    })
}

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

/// Deserializable `[api]` section of `config.toml`.
#[derive(Debug, serde::Deserialize, Default)]
struct ApiSection {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    listen: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
struct ConfigFileApi {
    #[serde(default)]
    api: ApiSection,
}

/// Load [`ApiConfig`] from `~/.syfrah/config.toml`.
///
/// Returns the default (disabled) config if the file does not exist or
/// has no `[api]` section.
pub fn load_api_config() -> ApiConfig {
    let path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".syfrah")
        .join("config.toml");

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return ApiConfig::default(),
    };

    let file: ConfigFileApi = match toml::from_str(&content) {
        Ok(f) => f,
        Err(_) => return ApiConfig::default(),
    };

    let defaults = ApiConfig::default();
    let listen = file
        .api
        .listen
        .and_then(|s| s.parse().ok())
        .unwrap_or(defaults.listen);

    ApiConfig {
        enabled: file.api.enabled.unwrap_or(false),
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

    /// Helper: send a GET request to the router and return (status, body_string).
    async fn get_response(uri: &str) -> (StatusCode, String) {
        let app = router();
        let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).to_string())
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let (status, body) = get_response("/v1/health").await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["status"], "ok");
    }

    #[tokio::test]
    async fn invalid_region_returns_bad_request() {
        // Region with uppercase is invalid
        let (status, _) = get_response("/v1/topology/regions/INVALID").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn invalid_zone_returns_bad_request() {
        let (status, _) = get_response("/v1/topology/zones/INVALID/peers").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn default_api_config_is_disabled() {
        let cfg = ApiConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.listen.to_string(), "127.0.0.1:9100");
    }
}
