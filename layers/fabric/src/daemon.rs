use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::{info, warn};
use wireguard_control::{Key, KeyPair};

use syfrah_core::addressing;
use syfrah_core::mesh::{PeerRecord, PeerStatus};
use syfrah_core::secret::MeshSecret;

use crate::control::{self, ControlHandler, ControlRequest, ControlResponse};
use crate::peering::{self, AutoAcceptConfig, PeeringState};
use crate::store::{self, NodeState};
use crate::wg;

const UNREACHABLE_TIMEOUT: Duration = Duration::from_secs(300);
const PERSIST_INTERVAL: Duration = Duration::from_secs(30);
const RECONCILE_INTERVAL: Duration = Duration::from_secs(30);

pub struct DaemonConfig {
    pub mesh_name: String,
    pub node_name: String,
    pub wg_listen_port: u16,
    pub public_endpoint: Option<SocketAddr>,
    pub peering_port: u16,
    pub region: Option<String>,
    pub zone: Option<String>,
}

/// Run the init flow: create a new mesh.
pub async fn run_init(config: DaemonConfig) -> anyhow::Result<()> {
    if store::exists() {
        anyhow::bail!("mesh state already exists. Run 'syfrah leave' first.");
    }

    let mesh_secret = MeshSecret::generate();
    let wg_keypair = wg::generate_keypair();

    let mesh_prefix = derive_prefix_from_secret(&mesh_secret);
    let mesh_ipv6 = addressing::derive_node_address(&mesh_prefix, wg_keypair.public.as_bytes());
    let endpoint = resolve_endpoint(&config);

    wg::setup_interface(&wg_keypair, config.wg_listen_port, mesh_ipv6)?;
    info!("wireguard interface syfrah0 up");

    // Region/zone: use provided or defaults
    let region = config
        .region
        .clone()
        .unwrap_or_else(|| "region-1".to_string());
    let zone = config
        .zone
        .clone()
        .unwrap_or_else(|| store::generate_zone(&region, &[]));

    let state = NodeState {
        mesh_name: config.mesh_name.clone(),
        mesh_secret: mesh_secret.to_string(),
        wg_private_key: wg_keypair.private.to_base64(),
        wg_public_key: wg_keypair.public.to_base64(),
        mesh_ipv6,
        mesh_prefix,
        wg_listen_port: config.wg_listen_port,
        node_name: config.node_name.clone(),
        public_endpoint: config.public_endpoint,
        peering_port: config.peering_port,
        peers: vec![],
        region: Some(region.clone()),
        zone: Some(zone.clone()),
        metrics: Default::default(),
    };
    store::save(&state)?;

    println!("Mesh '{}' created.", config.mesh_name);
    println!("  Secret: {mesh_secret}");
    println!("  Node:   {} ({})", config.node_name, mesh_ipv6);
    println!("  Region: {region}");
    println!("  Zone:   {zone}");
    println!();
    println!("Run 'syfrah peering' to accept new nodes.");
    println!("Running daemon... (Ctrl+C to stop)");

    let my_record = build_record(
        &config.node_name,
        &wg_keypair,
        endpoint,
        mesh_ipv6,
        Some(&region),
        Some(&zone),
    );
    run_daemon(my_record, &wg_keypair, mesh_secret, config.peering_port).await
}

/// Auto-init: create mesh if none exists, used by `syfrah peering` on a fresh node.
pub fn auto_init(
    node_name: &str,
    wg_port: u16,
    peering_port: u16,
) -> anyhow::Result<(MeshSecret, KeyPair)> {
    let mesh_secret = MeshSecret::generate();
    let wg_keypair = wg::generate_keypair();

    let mesh_prefix = derive_prefix_from_secret(&mesh_secret);
    let mesh_ipv6 = addressing::derive_node_address(&mesh_prefix, wg_keypair.public.as_bytes());

    wg::setup_interface(&wg_keypair, wg_port, mesh_ipv6)?;

    let state = NodeState {
        mesh_name: node_name.to_string(),
        mesh_secret: mesh_secret.to_string(),
        wg_private_key: wg_keypair.private.to_base64(),
        wg_public_key: wg_keypair.public.to_base64(),
        mesh_ipv6,
        mesh_prefix,
        wg_listen_port: wg_port,
        node_name: node_name.to_string(),
        public_endpoint: None,
        peering_port,
        peers: vec![],
        region: Some("region-1".to_string()),
        zone: Some("region-1-zone-1".to_string()),
        metrics: Default::default(),
    };
    store::save(&state)?;

    println!("Mesh auto-created.");
    println!("  Secret: {mesh_secret}");
    println!("  Node:   {node_name} ({mesh_ipv6})");

    Ok((mesh_secret, wg_keypair))
}

/// Run the join flow: request to join an existing mesh via TCP peering.
pub async fn run_join(
    target: SocketAddr,
    config: DaemonConfig,
    pin: Option<String>,
) -> anyhow::Result<()> {
    if store::exists() {
        anyhow::bail!("mesh state already exists. Run 'syfrah leave' first.");
    }

    let wg_keypair = wg::generate_keypair();
    let endpoint = resolve_endpoint(&config);

    let request = syfrah_core::mesh::JoinRequest {
        request_id: peering::generate_request_id(),
        node_name: config.node_name.clone(),
        wg_public_key: wg_keypair.public.to_base64(),
        endpoint,
        wg_listen_port: config.wg_listen_port,
        pin,
    };

    println!("Sending join request to {target}...");
    println!("Waiting for approval...");

    let response = peering::send_join_request(target, request).await?;

    if !response.accepted {
        let reason = response.reason.unwrap_or_else(|| "no reason given".into());
        anyhow::bail!("Join request rejected: {reason}");
    }

    let mesh_secret_str = response
        .mesh_secret
        .ok_or_else(|| anyhow::anyhow!("accepted but no mesh secret"))?;
    let mesh_secret: MeshSecret = mesh_secret_str
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid mesh secret: {e}"))?;
    let mesh_name = response.mesh_name.unwrap_or_else(|| "mesh".into());
    let mesh_prefix = response
        .mesh_prefix
        .ok_or_else(|| anyhow::anyhow!("accepted but no mesh prefix"))?;

    let mesh_ipv6 = addressing::derive_node_address(&mesh_prefix, wg_keypair.public.as_bytes());

    // Region/zone: use provided or auto-generate from existing peers
    let region = config
        .region
        .clone()
        .unwrap_or_else(|| "region-1".to_string());
    let zone = config
        .zone
        .clone()
        .unwrap_or_else(|| store::generate_zone(&region, &response.peers));

    wg::setup_interface(&wg_keypair, config.wg_listen_port, mesh_ipv6)?;
    info!("wireguard interface syfrah0 up");

    if !response.peers.is_empty() {
        info!("applying {} peers from join response", response.peers.len());
        if let Err(e) = wg::apply_peers(&wg_keypair.public, &response.peers) {
            warn!("failed to apply peers: {e}");
        }
    }

    let state = NodeState {
        mesh_name: mesh_name.clone(),
        mesh_secret: mesh_secret_str,
        wg_private_key: wg_keypair.private.to_base64(),
        wg_public_key: wg_keypair.public.to_base64(),
        mesh_ipv6,
        mesh_prefix,
        wg_listen_port: config.wg_listen_port,
        node_name: config.node_name.clone(),
        public_endpoint: config.public_endpoint,
        peering_port: config.peering_port,
        peers: response.peers.clone(),
        region: Some(region.clone()),
        zone: Some(zone.clone()),
        metrics: Default::default(),
    };
    store::save(&state)?;

    println!("Joined mesh '{mesh_name}'.");
    println!("  Node:   {} ({})", config.node_name, mesh_ipv6);
    println!("  Region: {region}");
    println!("  Zone:   {zone}");
    println!("Running daemon... (Ctrl+C to stop)");

    let my_record = build_record(
        &config.node_name,
        &wg_keypair,
        endpoint,
        mesh_ipv6,
        Some(&region),
        Some(&zone),
    );
    run_daemon(my_record, &wg_keypair, mesh_secret, config.peering_port).await
}

/// Restart daemon from saved state.
pub async fn run_start() -> anyhow::Result<()> {
    let state = store::load().map_err(|_| {
        anyhow::anyhow!("no mesh state found. Run 'syfrah init' or 'syfrah join' first.")
    })?;

    let mesh_secret: MeshSecret = state
        .mesh_secret
        .parse()
        .map_err(|e| anyhow::anyhow!("corrupt secret in state: {e}"))?;
    let wg_private = Key::from_base64(&state.wg_private_key)
        .map_err(|_| anyhow::anyhow!("corrupt WG private key in state"))?;
    let wg_keypair = KeyPair::from_private(wg_private);

    wg::setup_interface(&wg_keypair, state.wg_listen_port, state.mesh_ipv6)?;

    if !state.peers.is_empty() {
        info!("applying {} known peers from state", state.peers.len());
        if let Err(e) = wg::apply_peers(&wg_keypair.public, &state.peers) {
            warn!("failed to apply saved peers: {e}");
        }
    }

    println!("Restarting daemon for mesh '{}'...", state.mesh_name);
    println!("  Node: {} ({})", state.node_name, state.mesh_ipv6);
    println!("Running daemon... (Ctrl+C to stop)");

    let endpoint_addr = state
        .public_endpoint
        .unwrap_or_else(|| SocketAddr::new("0.0.0.0".parse().unwrap(), state.wg_listen_port));
    let my_record = build_record(
        &state.node_name,
        &wg_keypair,
        endpoint_addr,
        state.mesh_ipv6,
        state.region.as_deref(),
        state.zone.as_deref(),
    );
    run_daemon(my_record, &wg_keypair, mesh_secret, state.peering_port).await
}

/// Leave the mesh.
pub async fn run_leave() -> anyhow::Result<()> {
    if !store::exists() {
        println!("No mesh configured.");
        return Ok(());
    }
    if let Err(e) = wg::teardown_interface() {
        eprintln!("Warning: could not tear down WireGuard interface: {e}");
    }
    let _ = std::fs::remove_file(store::control_socket_path());
    store::clear()?;
    println!("Left the mesh. State cleared.");
    Ok(())
}

/// The main daemon loop.
pub async fn run_daemon(
    my_record: PeerRecord,
    wg_keypair: &KeyPair,
    mesh_secret: MeshSecret,
    peering_port: u16,
) -> anyhow::Result<()> {
    store::write_pid()?;

    let wg_pubkey = wg_keypair.public.clone();
    let peering_state = Arc::new(PeeringState::new());
    let enc_key = mesh_secret.encryption_key();

    let metrics_received = Arc::new(AtomicU64::new(0));
    let metrics_reconciliations = Arc::new(AtomicU64::new(0));
    let metrics_unreachable = Arc::new(AtomicU64::new(0));
    let daemon_started = now();

    // on_accepted callback: when a peer is accepted (manual or PIN), add to WG + store + announce
    let accepted_wg_pubkey = wg_pubkey.clone();
    let accepted_recv = metrics_received.clone();
    let accepted_recon = metrics_reconciliations.clone();
    let accepted_enc_key = enc_key;
    let accepted_peering_port = peering_port;
    let on_accepted: peering::OnAccepted = Arc::new(move |new_record| {
        accepted_recv.fetch_add(1, Ordering::Relaxed);
        let pubkey = accepted_wg_pubkey.clone();
        let recon = accepted_recon.clone();
        let record = new_record.clone();
        let enc = accepted_enc_key;
        let pp = accepted_peering_port;
        tokio::spawn(async move {
            // Add to WG
            if let Err(e) = wg::upsert_peer(&pubkey, &record) {
                warn!("failed to add peer to WG: {e}");
            } else {
                recon.fetch_add(1, Ordering::Relaxed);
                info!("wireguard peer added: {}", record.name);
            }
            // Save to store (atomic — no more load+push+save race)
            if let Err(e) = store::upsert_peer(&record) {
                warn!("failed to persist peer {}: {e}", record.name);
            }
            // Announce to existing peers
            let known = store::get_peers().unwrap_or_default();
            peering::announce_peer_to_mesh(&record, &known, &enc, pp).await;
        });
    });

    // Control handler
    let ctrl_handler = Arc::new(DaemonControlHandler {
        peering_state: peering_state.clone(),
        mesh_secret: mesh_secret.clone(),
        my_record: my_record.clone(),
        wg_pubkey: wg_pubkey.clone(),
        peering_port,
        on_accepted: on_accepted.clone(),
    });

    let control_path = store::control_socket_path();
    let control_handler: Arc<dyn ControlHandler> = ctrl_handler;
    let control_task = tokio::spawn(async move {
        control::start_control_listener(&control_path, control_handler).await;
    });

    // on_announce callback: when a peer announce arrives from existing mesh member
    let announce_wg_pubkey = wg_pubkey.clone();
    let announce_recv = metrics_received.clone();
    let announce_recon = metrics_reconciliations.clone();
    let on_announce: Arc<dyn Fn(PeerRecord) + Send + Sync> = Arc::new(move |record| {
        announce_recv.fetch_add(1, Ordering::Relaxed);
        let pubkey = announce_wg_pubkey.clone();
        let recon = announce_recon.clone();
        let record = record.clone();
        tokio::spawn(async move {
            if let Err(e) = wg::upsert_peer(&pubkey, &record) {
                warn!("failed to upsert peer {}: {e}", record.name);
            } else {
                recon.fetch_add(1, Ordering::Relaxed);
                info!("wireguard peer upserted via announce: {}", record.name);
            }
            // Save to store (atomic)
            if let Err(e) = store::upsert_peer(&record) {
                warn!("failed to persist announced peer {}: {e}", record.name);
            }
        });
    });

    // Peering listener
    let listener_state = peering_state.clone();
    let peering_task = tokio::spawn(async move {
        if let Err(e) = listener_state
            .run_listener(peering_port, Some(enc_key), on_announce, on_accepted)
            .await
        {
            warn!("peering listener error: {e}");
        }
    });

    // Persist metrics (atomic — no load+modify+save)
    let persist_recv = metrics_received.clone();
    let persist_recon = metrics_reconciliations.clone();
    let persist_unreach = metrics_unreachable.clone();
    let persist = async {
        let mut interval = tokio::time::interval(PERSIST_INTERVAL);
        loop {
            interval.tick().await;
            let _ = store::set_metric("peers_discovered", persist_recv.load(Ordering::Relaxed));
            let _ = store::set_metric("wg_reconciliations", persist_recon.load(Ordering::Relaxed));
            let _ = store::set_metric(
                "peers_marked_unreachable",
                persist_unreach.load(Ordering::Relaxed),
            );
            let _ = store::set_metric("daemon_started_at", daemon_started);
        }
    };

    // Health check: unreachable detection + recovery + last_seen update
    let health_counter = metrics_unreachable.clone();
    let health_recon = metrics_reconciliations.clone();
    let health_check = async {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;

            // Get WireGuard handshake data for all peers
            let wg_peers = wg::interface_summary().map(|s| s.peers).unwrap_or_default();

            let mut peers = store::get_peers().unwrap_or_default();
            let current = now();
            let mut changed = false;

            for peer in peers.iter_mut() {
                // Find matching WG peer by public key
                let wg_peer = wg_peers.iter().find(|p| p.public_key == peer.wg_public_key);

                // Update last_seen from WireGuard handshake timestamp
                if let Some(wp) = wg_peer {
                    if let Some(handshake_time) = wp.last_handshake {
                        let handshake_epoch = handshake_time
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        if handshake_epoch > peer.last_seen {
                            peer.last_seen = handshake_epoch;
                            changed = true;
                        }
                    }
                }

                // Recovery: unreachable → active if recent handshake
                if peer.status == PeerStatus::Unreachable
                    && current.saturating_sub(peer.last_seen) < UNREACHABLE_TIMEOUT.as_secs()
                {
                    info!(
                        "peer {} recovered (recent handshake), marking active",
                        peer.name
                    );
                    peer.status = PeerStatus::Active;
                    changed = true;
                }

                // Detection: active → unreachable if no handshake for too long
                if peer.status == PeerStatus::Active
                    && current.saturating_sub(peer.last_seen) > UNREACHABLE_TIMEOUT.as_secs()
                {
                    info!("marking peer {} as unreachable", peer.name);
                    peer.status = PeerStatus::Unreachable;
                    health_counter.fetch_add(1, Ordering::Relaxed);
                    changed = true;
                }
            }

            // Persist changes atomically
            if changed {
                for peer in &peers {
                    let _ = store::upsert_peer(peer);
                }
            }
        }
    };

    // Reconciliation loop: compare stored peers with WireGuard config
    let reconcile_wg_pubkey = wg_pubkey.clone();
    let reconcile_recon = health_recon;
    let reconcile = async {
        let mut interval = tokio::time::interval(RECONCILE_INTERVAL);
        loop {
            interval.tick().await;

            let stored_peers = store::get_peers().unwrap_or_default();
            let wg_summary = match wg::interface_summary() {
                Ok(s) => s,
                Err(_) => continue,
            };

            // For each stored peer, ensure it's in WireGuard
            for peer in &stored_peers {
                if peer.status == PeerStatus::Removed {
                    continue;
                }
                let in_wg = wg_summary
                    .peers
                    .iter()
                    .any(|p| p.public_key == peer.wg_public_key);
                if !in_wg {
                    info!(
                        "reconciling: adding missing peer {} to WireGuard",
                        peer.name
                    );
                    if let Err(e) = wg::upsert_peer(&reconcile_wg_pubkey, peer) {
                        warn!("reconciliation failed for {}: {e}", peer.name);
                    } else {
                        reconcile_recon.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    };

    tokio::select! {
        _ = control_task => {}
        _ = peering_task => {}
        _ = persist => {}
        _ = health_check => {}
        _ = reconcile => {}
        _ = tokio::signal::ctrl_c() => {
            println!("\nShutting down...");
        }
    }

    let _ = std::fs::remove_file(store::control_socket_path());
    wg::teardown_interface()?;
    store::remove_pid();
    info!("daemon stopped");
    Ok(())
}

/// Control handler for the daemon.
struct DaemonControlHandler {
    peering_state: Arc<PeeringState>,
    mesh_secret: MeshSecret,
    my_record: PeerRecord,
    wg_pubkey: Key,
    peering_port: u16,
    on_accepted: peering::OnAccepted,
}

#[async_trait::async_trait]
impl ControlHandler for DaemonControlHandler {
    async fn handle(&self, req: ControlRequest) -> ControlResponse {
        match req {
            ControlRequest::PeeringStart { port: _, pin } => {
                if let Some(pin_val) = pin {
                    let state = match store::load() {
                        Ok(s) => s,
                        Err(e) => {
                            return ControlResponse::Error {
                                message: format!("{e}"),
                            }
                        }
                    };
                    self.peering_state
                        .set_auto_accept(Some(AutoAcceptConfig {
                            pin: pin_val,
                            mesh_name: state.mesh_name,
                            mesh_secret_str: state.mesh_secret,
                            mesh_prefix: state.mesh_prefix,
                            my_record: self.my_record.clone(),
                            wg_pubkey: self.wg_pubkey.clone(),
                            encryption_key: self.mesh_secret.encryption_key(),
                            peering_port: self.peering_port,
                        }))
                        .await;
                }
                self.peering_state.set_active(true).await;
                ControlResponse::Ok
            }
            ControlRequest::PeeringStop => {
                self.peering_state.set_active(false).await;
                self.peering_state.set_auto_accept(None).await;
                ControlResponse::Ok
            }
            ControlRequest::PeeringList => {
                let requests = self.peering_state.list_pending().await;
                ControlResponse::PeeringList { requests }
            }
            ControlRequest::PeeringAccept { request_id } => {
                let state = match store::load() {
                    Ok(s) => s,
                    Err(e) => {
                        return ControlResponse::Error {
                            message: format!("{e}"),
                        }
                    }
                };

                let mut all_peers = state.peers.clone();
                all_peers.push(self.my_record.clone());

                let response = syfrah_core::mesh::JoinResponse {
                    accepted: true,
                    mesh_name: Some(state.mesh_name.clone()),
                    mesh_secret: Some(state.mesh_secret.clone()),
                    mesh_prefix: Some(state.mesh_prefix),
                    peers: all_peers,
                    reason: None,
                };

                match self.peering_state.accept(&request_id, response).await {
                    Ok(info) => {
                        let new_wg_pub = match Key::from_base64(&info.wg_public_key) {
                            Ok(k) => k,
                            Err(_) => {
                                return ControlResponse::Error {
                                    message: "invalid WG key".into(),
                                }
                            }
                        };
                        let new_mesh_ipv6 = addressing::derive_node_address(
                            &state.mesh_prefix,
                            new_wg_pub.as_bytes(),
                        );
                        let new_record = PeerRecord {
                            name: info.node_name.clone(),
                            wg_public_key: info.wg_public_key,
                            endpoint: info.endpoint,
                            mesh_ipv6: new_mesh_ipv6,
                            last_seen: now(),
                            status: PeerStatus::Active,
                            region: None,
                            zone: None,
                        };
                        (self.on_accepted)(new_record);
                        ControlResponse::PeeringAccepted {
                            peer_name: info.node_name,
                        }
                    }
                    Err(e) => ControlResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
            ControlRequest::PeeringReject { request_id, reason } => {
                match self.peering_state.reject(&request_id, reason).await {
                    Ok(()) => ControlResponse::Ok,
                    Err(e) => ControlResponse::Error {
                        message: e.to_string(),
                    },
                }
            }
        }
    }
}

fn build_record(
    name: &str,
    wg_keypair: &KeyPair,
    endpoint: SocketAddr,
    mesh_ipv6: std::net::Ipv6Addr,
    region: Option<&str>,
    zone: Option<&str>,
) -> PeerRecord {
    PeerRecord {
        name: name.to_string(),
        wg_public_key: wg_keypair.public.to_base64(),
        endpoint,
        mesh_ipv6,
        last_seen: now(),
        status: PeerStatus::Active,
        region: region.map(|s| s.to_string()),
        zone: zone.map(|s| s.to_string()),
    }
}

fn resolve_endpoint(config: &DaemonConfig) -> SocketAddr {
    config
        .public_endpoint
        .unwrap_or_else(|| SocketAddr::new("0.0.0.0".parse().unwrap(), config.wg_listen_port))
}

pub fn derive_prefix_from_secret(secret: &MeshSecret) -> std::net::Ipv6Addr {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest([b"mesh-prefix:" as &[u8], secret.as_bytes()].concat());
    std::net::Ipv6Addr::new(
        0xfd00 | (hash[0] as u16),
        ((hash[1] as u16) << 8) | (hash[2] as u16),
        ((hash[3] as u16) << 8) | (hash[4] as u16),
        0,
        0,
        0,
        0,
        0,
    )
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
