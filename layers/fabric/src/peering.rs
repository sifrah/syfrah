use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, RwLock, Semaphore};
use tracing::{debug, info, warn};

use syfrah_core::mesh::{
    decrypt_record, encrypt_record, JoinRequest, JoinResponse, PeerRecord, PeeringMessage,
};

use crate::events::{self, EventType};

const JOIN_TIMEOUT: Duration = Duration::from_secs(300);
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_MSG_SIZE: u32 = 65536;

/// Default maximum concurrent peering connections.
const DEFAULT_MAX_CONNECTIONS: usize = 100;
/// Default maximum pending join requests.
const DEFAULT_MAX_PENDING_JOINS: usize = 100;

#[derive(Debug, Error)]
pub enum PeeringError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("encryption error: {0}")]
    Encryption(#[from] syfrah_core::mesh::MeshError),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("request not found: {0}")]
    NotFound(String),
    #[error("timeout")]
    Timeout,
    #[error("rejected: {0}")]
    Rejected(String),
    #[error("peer limit exceeded: {current} peers (max {max})")]
    PeerLimitExceeded { current: usize, max: usize },
}

/// Metadata about a pending join request (serializable for CLI display).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JoinRequestInfo {
    pub request_id: String,
    pub node_name: String,
    pub wg_public_key: String,
    pub endpoint: SocketAddr,
    pub wg_listen_port: u16,
    pub received_at: u64,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub zone: Option<String>,
}

struct PendingJoin {
    info: JoinRequestInfo,
    response_tx: oneshot::Sender<JoinResponse>,
}

/// Config for auto-accepting join requests with a PIN.
pub struct AutoAcceptConfig {
    pub pin: String,
    pub mesh_name: String,
    pub mesh_secret_str: String,
    pub mesh_prefix: std::net::Ipv6Addr,
    pub my_record: PeerRecord,
    pub wg_pubkey: wireguard_control::Key,
    pub encryption_key: [u8; 32],
    pub peering_port: u16,
    pub max_peers: usize,
}

/// Callback type invoked when a peer is accepted (either manually or via PIN).
pub type OnAccepted = Arc<dyn Fn(PeerRecord) + Send + Sync>;

/// Manages peering state: pending join requests, listener lifecycle.
pub struct PeeringState {
    pending: Arc<RwLock<HashMap<String, PendingJoin>>>,
    listener_active: Arc<RwLock<bool>>,
    auto_accept: Arc<RwLock<Option<AutoAcceptConfig>>>,
    /// Metrics: total connections rejected due to limit.
    connections_rejected: Arc<AtomicU64>,
    /// Metrics: currently active connections.
    connections_active: Arc<AtomicU64>,
    /// Max concurrent connections (configurable).
    max_connections: usize,
    /// Max pending join requests (configurable).
    max_pending_joins: usize,
}

impl Default for PeeringState {
    fn default() -> Self {
        Self::new()
    }
}

impl PeeringState {
    pub fn new() -> Self {
        Self::with_limits(DEFAULT_MAX_CONNECTIONS, DEFAULT_MAX_PENDING_JOINS)
    }

    /// Create a PeeringState with custom connection and pending-join limits.
    pub fn with_limits(max_connections: usize, max_pending_joins: usize) -> Self {
        Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
            listener_active: Arc::new(RwLock::new(false)),
            auto_accept: Arc::new(RwLock::new(None)),
            connections_rejected: Arc::new(AtomicU64::new(0)),
            connections_active: Arc::new(AtomicU64::new(0)),
            max_connections,
            max_pending_joins,
        }
    }

    /// Return the number of connections rejected due to the concurrency limit.
    pub fn connections_rejected(&self) -> u64 {
        self.connections_rejected.load(Ordering::Relaxed)
    }

    /// Return the number of currently active connections.
    pub fn connections_active(&self) -> u64 {
        self.connections_active.load(Ordering::Relaxed)
    }

    pub async fn is_active(&self) -> bool {
        *self.listener_active.read().await
    }

    pub async fn set_active(&self, active: bool) {
        *self.listener_active.write().await = active;
    }

    pub async fn set_auto_accept(&self, config: Option<AutoAcceptConfig>) {
        *self.auto_accept.write().await = config;
    }

    pub async fn list_pending(&self) -> Vec<JoinRequestInfo> {
        let pending = self.pending.read().await;
        pending.values().map(|p| p.info.clone()).collect()
    }

    pub async fn accept(
        &self,
        request_id: &str,
        response: JoinResponse,
    ) -> Result<JoinRequestInfo, PeeringError> {
        let mut pending = self.pending.write().await;
        let entry = pending
            .remove(request_id)
            .ok_or_else(|| PeeringError::NotFound(request_id.to_string()))?;
        let info = entry.info.clone();
        let _ = entry.response_tx.send(response);
        Ok(info)
    }

    pub async fn reject(
        &self,
        request_id: &str,
        reason: Option<String>,
    ) -> Result<(), PeeringError> {
        let mut pending = self.pending.write().await;
        let entry = pending
            .remove(request_id)
            .ok_or_else(|| PeeringError::NotFound(request_id.to_string()))?;
        let _ = entry.response_tx.send(JoinResponse {
            accepted: false,
            mesh_name: None,
            mesh_secret: None,
            mesh_prefix: None,
            peers: vec![],
            reason,
            approved_by: None,
        });
        Ok(())
    }

    /// Run the peering TCP listener. Runs forever.
    #[allow(unreachable_code)]
    pub async fn run_listener(
        &self,
        port: u16,
        encryption_key: Option<[u8; 32]>,
        on_announce: Arc<dyn Fn(PeerRecord) + Send + Sync>,
        on_accepted: OnAccepted,
    ) -> Result<(), PeeringError> {
        self.set_active(true).await;
        let addr: SocketAddr = format!("0.0.0.0:{port}").parse().unwrap();
        let listener = TcpListener::bind(addr).await?;
        let semaphore = Arc::new(Semaphore::new(self.max_connections));
        info!(
            port = port,
            max_connections = self.max_connections,
            max_pending_joins = self.max_pending_joins,
            "peering listener started"
        );

        loop {
            let (stream, peer_addr) = match listener.accept().await {
                Ok((s, a)) => (s, a),
                Err(e) => {
                    warn!("TCP accept error: {e}");
                    continue;
                }
            };

            // Enforce concurrent connection limit via semaphore.
            let permit = match semaphore.clone().try_acquire_owned() {
                Ok(p) => p,
                Err(_) => {
                    self.connections_rejected.fetch_add(1, Ordering::Relaxed);
                    warn!(
                        peer = %peer_addr,
                        active = self.connections_active.load(Ordering::Relaxed),
                        limit = self.max_connections,
                        "connection limit reached, rejecting"
                    );
                    // Drop the stream immediately (TCP RST).
                    drop(stream);
                    continue;
                }
            };

            self.connections_active.fetch_add(1, Ordering::Relaxed);
            debug!("peering connection from {peer_addr}");

            let pending = self.pending.clone();
            let enc_key = encryption_key;
            let on_announce = on_announce.clone();
            let auto_accept = self.auto_accept.clone();
            let on_accepted = on_accepted.clone();
            let active_counter = self.connections_active.clone();
            let max_pending = self.max_pending_joins;

            tokio::spawn(async move {
                let _permit = permit; // held until this task ends
                if let Err(e) = handle_incoming(
                    stream,
                    peer_addr,
                    pending,
                    enc_key,
                    on_announce,
                    auto_accept,
                    on_accepted,
                    max_pending,
                )
                .await
                {
                    debug!("peering connection from {peer_addr} failed: {e}");
                }
                active_counter.fetch_sub(1, Ordering::Relaxed);
            });
        }

        Ok(())
    }
}

/// Handle an incoming TCP connection.
#[allow(clippy::too_many_arguments)]
async fn handle_incoming(
    mut stream: TcpStream,
    peer_addr: SocketAddr,
    pending: Arc<RwLock<HashMap<String, PendingJoin>>>,
    encryption_key: Option<[u8; 32]>,
    on_announce: Arc<dyn Fn(PeerRecord) + Send + Sync>,
    auto_accept: Arc<RwLock<Option<AutoAcceptConfig>>>,
    on_accepted: OnAccepted,
    max_pending_joins: usize,
) -> Result<(), PeeringError> {
    // Apply read timeout to protect against slowloris attacks.
    let msg = tokio::time::timeout(EXCHANGE_TIMEOUT, read_message(&mut stream))
        .await
        .map_err(|_| PeeringError::Timeout)??;

    match msg {
        PeeringMessage::JoinRequest(mut req) => {
            // Auto-detect endpoint: if 0.0.0.0, use TCP peer IP
            if req.endpoint.ip().is_unspecified() {
                req.endpoint = SocketAddr::new(peer_addr.ip(), req.wg_listen_port);
                info!(
                    "auto-detected endpoint for {}: {}",
                    req.node_name, req.endpoint
                );
            }

            info!(
                node = %req.node_name,
                endpoint = %req.endpoint,
                request_id = %req.request_id,
                "join request received"
            );
            events::emit(
                EventType::JoinRequestReceived,
                Some(&req.node_name),
                Some(&req.endpoint.to_string()),
                Some(&format!("request_id={}", req.request_id)),
                None,
            );

            // Warn if node name already in active peers (the node likely left and is rejoining)
            {
                let peers = crate::store::get_peers().unwrap_or_default();
                if peers.iter().any(|p| {
                    p.name == req.node_name && p.status == syfrah_core::mesh::PeerStatus::Active
                }) {
                    warn!(
                        node = %req.node_name,
                        "node name already in mesh — accepting will replace the old peer entry"
                    );
                }
            }

            // Check PIN auto-accept
            if let Some(ref req_pin) = req.pin {
                let auto = auto_accept.read().await;
                if let Some(ref config) = *auto {
                    if config.pin == *req_pin {
                        // Check peer limit before accepting
                        let current_peers = crate::store::peer_count().unwrap_or(0);
                        if current_peers >= config.max_peers {
                            warn!(
                                node = %req.node_name,
                                current = current_peers,
                                max = config.max_peers,
                                "peer limit reached, rejecting join request"
                            );
                            events::emit(
                                EventType::PeerLimitReached,
                                Some(&req.node_name),
                                Some(&req.endpoint.to_string()),
                                Some(&format!("max_peers={}", config.max_peers)),
                                None,
                            );
                            let response = JoinResponse {
                                accepted: false,
                                mesh_name: None,
                                mesh_secret: None,
                                mesh_prefix: None,
                                peers: vec![],
                                reason: Some(format!(
                                    "peer limit reached ({}/{})",
                                    current_peers, config.max_peers
                                )),
                                approved_by: None,
                            };
                            write_message(&mut stream, &PeeringMessage::JoinResponse(response))
                                .await?;
                            return Ok(());
                        }

                        info!(node = %req.node_name, request_id = %req.request_id, "PIN matched, auto-accepting");
                        events::emit(
                            EventType::JoinAutoAccepted,
                            Some(&req.node_name),
                            Some(&req.endpoint.to_string()),
                            Some("pin-matched"),
                            None,
                        );
                        let (response, new_record) = build_auto_accept_response(&req, config)?;
                        write_message(&mut stream, &PeeringMessage::JoinResponse(response)).await?;
                        on_accepted(new_record);
                        return Ok(());
                    }
                }
            }

            // Manual approval: store and wait
            let (tx, rx) = oneshot::channel();
            let pending_node_name = req.node_name.clone();
            let pending_endpoint = req.endpoint.to_string();
            let info = JoinRequestInfo {
                request_id: req.request_id.clone(),
                node_name: req.node_name,
                wg_public_key: req.wg_public_key,
                endpoint: req.endpoint,
                wg_listen_port: req.wg_listen_port,
                received_at: now(),
                region: req.region,
                zone: req.zone,
            };

            {
                let mut map = pending.write().await;
                // Dedup: if there's already a pending request from the same node name,
                // remove it (the joiner retried with a new key).
                let stale_id = map
                    .values()
                    .find(|p| p.info.node_name == info.node_name)
                    .map(|p| p.info.request_id.clone());
                if let Some(old_id) = stale_id {
                    info!(node = %info.node_name, old_request_id = %old_id, "replacing stale join request");
                    map.remove(&old_id);
                }
                // Enforce pending queue limit.
                if map.len() >= max_pending_joins {
                    warn!(
                        limit = max_pending_joins,
                        "pending join queue full, rejecting request"
                    );
                    return Err(PeeringError::Protocol("too many pending joins".into()));
                }
                map.insert(
                    req.request_id.clone(),
                    PendingJoin {
                        info,
                        response_tx: tx,
                    },
                );
            }

            let response = match tokio::time::timeout(JOIN_TIMEOUT, rx).await {
                Ok(Ok(resp)) => resp,
                Ok(Err(_)) => {
                    let mut map = pending.write().await;
                    map.remove(&req.request_id);
                    return Err(PeeringError::Protocol("daemon shutdown".into()));
                }
                Err(_) => {
                    let mut map = pending.write().await;
                    map.remove(&req.request_id);
                    events::emit(
                        EventType::JoinTimeout,
                        Some(&pending_node_name),
                        Some(&pending_endpoint),
                        Some(&format!("request_id={}", req.request_id)),
                        None,
                    );
                    JoinResponse {
                        accepted: false,
                        mesh_name: None,
                        mesh_secret: None,
                        mesh_prefix: None,
                        peers: vec![],
                        reason: Some("request timed out".into()),
                        approved_by: None,
                    }
                }
            };

            write_message(&mut stream, &PeeringMessage::JoinResponse(response)).await?;
        }

        PeeringMessage::PeerAnnounce(ciphertext) => {
            let enc_key =
                encryption_key.ok_or_else(|| PeeringError::Protocol("no encryption key".into()))?;
            let record = decrypt_record(&ciphertext, &enc_key)?;
            info!(
                peer = %record.name,
                ipv6 = %record.mesh_ipv6,
                from = %peer_addr,
                "peer announce received"
            );
            on_announce(record);
        }

        PeeringMessage::JoinResponse(_) => {
            return Err(PeeringError::Protocol("unexpected JoinResponse".into()));
        }
    }

    Ok(())
}

/// Build a JoinResponse and PeerRecord for auto-accept.
fn build_auto_accept_response(
    req: &JoinRequest,
    config: &AutoAcceptConfig,
) -> Result<(JoinResponse, PeerRecord), PeeringError> {
    use syfrah_core::addressing;

    let new_wg_pub = wireguard_control::Key::from_base64(&req.wg_public_key)
        .map_err(|_| PeeringError::Protocol("invalid WG public key".into()))?;
    let new_mesh_ipv6 = addressing::derive_node_address(&config.mesh_prefix, new_wg_pub.as_bytes());

    // Load current peers from store + our own record
    let mut all_peers = crate::store::load().map(|s| s.peers).unwrap_or_default();
    all_peers.push(config.my_record.clone());

    // Use the joiner's region/zone from the request. If zone was not
    // provided, auto-generate one using the leader's peer list so the
    // joiner gets a unique zone.
    let region = req.region.clone().unwrap_or_else(|| "region-1".to_string());
    let zone = req
        .zone
        .clone()
        .unwrap_or_else(|| crate::store::generate_zone(&region, &all_peers));

    let new_record = PeerRecord {
        name: req.node_name.clone(),
        wg_public_key: req.wg_public_key.clone(),
        endpoint: req.endpoint,
        mesh_ipv6: new_mesh_ipv6,
        last_seen: now(),
        status: syfrah_core::mesh::PeerStatus::Active,
        region: Some(region),
        zone: Some(zone),
    };

    let response = JoinResponse {
        accepted: true,
        mesh_name: Some(config.mesh_name.clone()),
        mesh_secret: Some(config.mesh_secret_str.clone()),
        mesh_prefix: Some(config.mesh_prefix),
        peers: all_peers,
        reason: None,
        approved_by: Some("pin".into()),
    };

    Ok((response, new_record))
}

// --- Client-side functions ---

/// Send a join request to an existing node and wait for response.
pub async fn send_join_request(
    target: SocketAddr,
    request: JoinRequest,
) -> Result<JoinResponse, PeeringError> {
    let mut stream = tokio::time::timeout(EXCHANGE_TIMEOUT, TcpStream::connect(target))
        .await
        .map_err(|_| PeeringError::Timeout)??;

    write_message(&mut stream, &PeeringMessage::JoinRequest(request)).await?;

    let msg = tokio::time::timeout(JOIN_TIMEOUT, read_message(&mut stream))
        .await
        .map_err(|_| PeeringError::Timeout)??;

    match msg {
        PeeringMessage::JoinResponse(resp) => Ok(resp),
        _ => Err(PeeringError::Protocol("expected JoinResponse".into())),
    }
}

/// Announce a new peer to an existing mesh member.
/// Maximum retry attempts for transient announcement failures.
const ANNOUNCE_MAX_RETRIES: u32 = 3;

pub async fn announce_peer(
    target_endpoint: SocketAddr,
    peering_port: u16,
    record: &PeerRecord,
    encryption_key: &[u8; 32],
) -> Result<(), PeeringError> {
    let target = SocketAddr::new(target_endpoint.ip(), peering_port);
    let ciphertext = encrypt_record(record, encryption_key)?;

    let mut last_err = None;
    for attempt in 0..ANNOUNCE_MAX_RETRIES {
        if attempt > 0 {
            let delay = Duration::from_secs(1 << (attempt - 1)); // 1s, 2s, 4s
            tokio::time::sleep(delay).await;
        }
        match try_announce(&target, &ciphertext).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                // Only retry on transient errors (Io, Timeout)
                match &e {
                    PeeringError::Io(_) | PeeringError::Timeout => {
                        debug!(
                            "announce attempt {}/{} to {target} failed: {e}",
                            attempt + 1,
                            ANNOUNCE_MAX_RETRIES
                        );
                        last_err = Some(e);
                    }
                    _ => return Err(e), // Non-transient: fail immediately
                }
            }
        }
    }
    Err(last_err.unwrap_or(PeeringError::Timeout))
}

async fn try_announce(target: &SocketAddr, ciphertext: &[u8]) -> Result<(), PeeringError> {
    let mut stream = tokio::time::timeout(EXCHANGE_TIMEOUT, TcpStream::connect(target))
        .await
        .map_err(|_| PeeringError::Timeout)??;
    write_message(
        &mut stream,
        &PeeringMessage::PeerAnnounce(ciphertext.to_vec()),
    )
    .await?;
    Ok(())
}

/// Announce a new peer to all known mesh members.
/// Returns (succeeded, failed) counts.
pub async fn announce_peer_to_mesh(
    record: &PeerRecord,
    known_peers: &[PeerRecord],
    encryption_key: &[u8; 32],
    peering_port: u16,
) -> (usize, usize) {
    let mut succeeded = 0;
    let mut failed = 0;
    for peer in known_peers {
        if peer.wg_public_key == record.wg_public_key {
            continue;
        }
        if let Err(e) = announce_peer(peer.endpoint, peering_port, record, encryption_key).await {
            warn!(target_peer = %peer.name, target_endpoint = %peer.endpoint, error = %e, "announcement failed after retries");
            events::emit(
                EventType::PeerAnnounceFailed,
                Some(&peer.name),
                Some(&peer.endpoint.to_string()),
                Some(&format!("error={e}")),
                None,
            );
            failed += 1;
        } else {
            debug!(target_peer = %peer.name, record = %record.name, "announced peer");
            succeeded += 1;
        }
    }
    (succeeded, failed)
}

// --- Wire protocol helpers ---

async fn write_message(stream: &mut TcpStream, msg: &PeeringMessage) -> Result<(), PeeringError> {
    let data = serde_json::to_vec(msg)?;
    let len = data.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&data).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_message(stream: &mut TcpStream) -> Result<PeeringMessage, PeeringError> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_MSG_SIZE {
        return Err(PeeringError::Protocol(format!("message too large: {len}")));
    }
    let mut data = vec![0u8; len as usize];
    stream.read_exact(&mut data).await?;
    let msg: PeeringMessage = serde_json::from_slice(&data)?;
    Ok(msg)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Generate a short random request ID (8 hex chars).
pub fn generate_request_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 4];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Generate a random 4-digit PIN.
pub fn generate_pin() -> String {
    use rand::Rng;
    let n: u16 = rand::thread_rng().gen_range(1000..10000);
    n.to_string()
}
