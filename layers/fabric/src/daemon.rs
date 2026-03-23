use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::{debug, info, warn};
use wireguard_control::{Key, KeyPair};

use syfrah_core::addressing;
use syfrah_core::mesh::{PeerRecord, PeerStatus};
use syfrah_core::secret::MeshSecret;

use crate::config::{self, Tuning};
use crate::control::{self, ControlHandler, ControlRequest, ControlResponse};
use crate::events::{self, EventType};
use crate::peering::{self, AutoAcceptConfig, PeeringState};
use crate::store::{self, NodeState};
use crate::wg;

pub struct DaemonConfig {
    pub mesh_name: String,
    pub node_name: String,
    pub wg_listen_port: u16,
    pub public_endpoint: Option<SocketAddr>,
    pub peering_port: u16,
    pub region: Option<String>,
    pub zone: Option<String>,
}

/// Data produced by `setup_init` / `setup_join`, needed to start the daemon loop.
pub struct DaemonReady {
    pub my_record: PeerRecord,
    pub wg_keypair: KeyPair,
    pub mesh_secret: MeshSecret,
    pub peering_port: u16,
}

/// Setup the init flow: create a new mesh, save state, print info.
/// Returns a DaemonReady that can be passed to run_daemon.
pub fn setup_init(config: &DaemonConfig) -> anyhow::Result<DaemonReady> {
    if store::exists() {
        anyhow::bail!("mesh state already exists. Run 'syfrah leave' first.");
    }

    let mesh_secret = MeshSecret::generate();
    let wg_keypair = wg::generate_keypair();

    let mesh_prefix = derive_prefix_from_secret(&mesh_secret);
    let mesh_ipv6 = addressing::derive_node_address(&mesh_prefix, wg_keypair.public.as_bytes());
    let endpoint = resolve_endpoint(config);

    wg::setup_interface(&wg_keypair, config.wg_listen_port, mesh_ipv6)?;
    info!(flow = "init", mesh = %config.mesh_name, node = %config.node_name, "wireguard interface up");

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

    let my_record = build_record(
        &config.node_name,
        &wg_keypair,
        endpoint,
        mesh_ipv6,
        Some(&region),
        Some(&zone),
    );
    Ok(DaemonReady {
        my_record,
        wg_keypair,
        mesh_secret,
        peering_port: config.peering_port,
    })
}

/// Run the init flow: create a new mesh and run daemon (foreground).
pub async fn run_init(config: DaemonConfig) -> anyhow::Result<()> {
    let ready = setup_init(&config)?;
    println!();
    println!("Run 'syfrah peering' to accept new nodes.");
    println!("Running daemon... (Ctrl+C to stop)");
    run_daemon(
        ready.my_record,
        &ready.wg_keypair,
        ready.mesh_secret,
        ready.peering_port,
    )
    .await
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

/// Setup the join flow: send join request, save state, print info.
/// Returns a DaemonReady that can be passed to run_daemon.
pub async fn setup_join(
    target: SocketAddr,
    config: &DaemonConfig,
    pin: Option<String>,
) -> anyhow::Result<DaemonReady> {
    if store::exists() {
        anyhow::bail!("mesh state already exists. Run 'syfrah leave' first.");
    }

    let wg_keypair = wg::generate_keypair();
    let endpoint = resolve_endpoint(config);

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
    info!(flow = "join", node = %config.node_name, "wireguard interface up");

    if !response.peers.is_empty() {
        info!(
            flow = "join",
            count = response.peers.len(),
            "applying peers from join response"
        );
        if let Err(e) = wg::apply_peers(&wg_keypair.public, &response.peers) {
            warn!(flow = "join", error = %e, "failed to apply peers");
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

    let my_record = build_record(
        &config.node_name,
        &wg_keypair,
        endpoint,
        mesh_ipv6,
        Some(&region),
        Some(&zone),
    );
    Ok(DaemonReady {
        my_record,
        wg_keypair,
        mesh_secret,
        peering_port: config.peering_port,
    })
}

/// Run the join flow: join mesh and run daemon (foreground).
pub async fn run_join(
    target: SocketAddr,
    config: DaemonConfig,
    pin: Option<String>,
) -> anyhow::Result<()> {
    let ready = setup_join(target, &config, pin).await?;
    println!("Running daemon... (Ctrl+C to stop)");
    run_daemon(
        ready.my_record,
        &ready.wg_keypair,
        ready.mesh_secret,
        ready.peering_port,
    )
    .await
}

/// Setup restart from saved state: load state, setup WG, print info.
/// Returns a DaemonReady that can be passed to run_daemon.
pub fn setup_start() -> anyhow::Result<DaemonReady> {
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
        info!(
            flow = "start",
            count = state.peers.len(),
            "applying peers from saved state"
        );
        if let Err(e) = wg::apply_peers(&wg_keypair.public, &state.peers) {
            warn!(flow = "start", error = %e, "failed to apply saved peers");
        }
    }

    println!("Restarting daemon for mesh '{}'...", state.mesh_name);
    println!("  Node: {} ({})", state.node_name, state.mesh_ipv6);

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
    Ok(DaemonReady {
        my_record,
        wg_keypair,
        mesh_secret,
        peering_port: state.peering_port,
    })
}

/// Restart daemon from saved state (foreground).
pub async fn run_start() -> anyhow::Result<()> {
    let ready = setup_start()?;
    println!("Running daemon... (Ctrl+C to stop)");
    run_daemon(
        ready.my_record,
        &ready.wg_keypair,
        ready.mesh_secret,
        ready.peering_port,
    )
    .await
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
    let tuning = config::load_tuning().unwrap_or_else(|e| {
        warn!("failed to load config.toml: {e}, using defaults");
        Tuning::default()
    });
    info!(
        "daemon tuning: health_check={}s reconcile={}s persist={}s unreachable={}s",
        tuning.health_check_interval.as_secs(),
        tuning.reconcile_interval.as_secs(),
        tuning.persist_interval.as_secs(),
        tuning.unreachable_timeout.as_secs(),
    );

    store::write_pid()?;
    events::emit(
        EventType::DaemonStarted,
        None,
        None,
        None,
        Some(tuning.max_events),
    );

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
    let accepted_max_events = tuning.max_events;
    let on_accepted: peering::OnAccepted = Arc::new(move |new_record| {
        accepted_recv.fetch_add(1, Ordering::Relaxed);
        let pubkey = accepted_wg_pubkey.clone();
        let recon = accepted_recon.clone();
        let record = new_record.clone();
        let enc = accepted_enc_key;
        let pp = accepted_peering_port;
        let max_ev = accepted_max_events;
        tokio::spawn(async move {
            // Add to WG
            if let Err(e) = wg::upsert_peer(&pubkey, &record) {
                warn!(peer = %record.name, endpoint = %record.endpoint, error = %e, "failed to add peer to WG");
            } else {
                recon.fetch_add(1, Ordering::Relaxed);
                info!(peer = %record.name, endpoint = %record.endpoint, "peer accepted and added to WG");
                events::emit(
                    EventType::PeerActive,
                    Some(&record.name),
                    Some(&record.endpoint.to_string()),
                    Some(&format!("mesh_ipv6={}", record.mesh_ipv6)),
                    Some(max_ev),
                );
            }
            // Save to store (atomic — no more load+push+save race)
            if let Err(e) = store::upsert_peer(&record) {
                warn!(peer = %record.name, error = %e, "failed to persist peer");
            }
            // Announce to existing peers
            let known = store::get_peers().unwrap_or_default();
            let (_ok, failed) = peering::announce_peer_to_mesh(&record, &known, &enc, pp).await;
            if failed > 0 {
                let _ = store::inc_metric("announcements_failed", failed as u64);
                events::emit(
                    EventType::PeerAnnounceFailed,
                    Some(&record.name),
                    None,
                    Some(&format!("failed_count={failed}")),
                    Some(max_ev),
                );
            }
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
        max_events: tuning.max_events,
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
    let announce_max_events = tuning.max_events;
    let on_announce: Arc<dyn Fn(PeerRecord) + Send + Sync> = Arc::new(move |record| {
        announce_recv.fetch_add(1, Ordering::Relaxed);
        events::emit(
            EventType::PeerAnnounceReceived,
            Some(&record.name),
            Some(&record.endpoint.to_string()),
            Some(&format!("mesh_ipv6={}", record.mesh_ipv6)),
            Some(announce_max_events),
        );
        let pubkey = announce_wg_pubkey.clone();
        let recon = announce_recon.clone();
        let record = record.clone();
        tokio::spawn(async move {
            if let Err(e) = wg::upsert_peer(&pubkey, &record) {
                warn!(peer = %record.name, endpoint = %record.endpoint, error = %e, "failed to upsert announced peer");
            } else {
                recon.fetch_add(1, Ordering::Relaxed);
                debug!(peer = %record.name, endpoint = %record.endpoint, "peer upserted via announce");
            }
            // Save to store (atomic)
            if let Err(e) = store::upsert_peer(&record) {
                warn!(peer = %record.name, error = %e, "failed to persist announced peer");
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
        let mut interval = tokio::time::interval(tuning.persist_interval);
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
    let unreachable_timeout_secs = tuning.unreachable_timeout.as_secs();
    let health_counter = metrics_unreachable.clone();
    let health_recon = metrics_reconciliations.clone();
    let health_max_events = tuning.max_events;
    let health_check = async {
        let mut interval = tokio::time::interval(tuning.health_check_interval);
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

                let wg_handshake_epoch = wg_peer.and_then(|wp| {
                    wp.last_handshake
                        .map(|ht| ht.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())
                });

                let old_status = peer.status;
                let peer_changed = evaluate_peer_health(
                    peer,
                    wg_handshake_epoch,
                    current,
                    unreachable_timeout_secs,
                );

                if peer_changed {
                    changed = true;

                    if old_status == PeerStatus::Unreachable && peer.status == PeerStatus::Active {
                        info!(peer = %peer.name, last_seen = peer.last_seen, "peer recovered, marking active");
                    }
                    if old_status == PeerStatus::Active && peer.status == PeerStatus::Unreachable {
                        info!(peer = %peer.name, last_seen = peer.last_seen, timeout_secs = unreachable_timeout_secs, "marking peer as unreachable");
                        health_counter.fetch_add(1, Ordering::Relaxed);
                    }
                }

                // Recovery: unreachable → active if recent handshake
                if peer.status == PeerStatus::Unreachable
                    && current.saturating_sub(peer.last_seen) < unreachable_timeout_secs
                {
                    info!(peer = %peer.name, last_seen = peer.last_seen, "peer recovered, marking active");
                    peer.status = PeerStatus::Active;
                    changed = true;
                    events::emit(
                        EventType::PeerRecovered,
                        Some(&peer.name),
                        Some(&peer.endpoint.to_string()),
                        Some("handshake resumed"),
                        Some(health_max_events),
                    );
                }

                // Detection: active → unreachable if no handshake for too long
                if peer.status == PeerStatus::Active
                    && current.saturating_sub(peer.last_seen) > unreachable_timeout_secs
                {
                    info!(peer = %peer.name, last_seen = peer.last_seen, timeout_secs = unreachable_timeout_secs, "marking peer as unreachable");
                    peer.status = PeerStatus::Unreachable;
                    health_counter.fetch_add(1, Ordering::Relaxed);
                    changed = true;
                    events::emit(
                        EventType::PeerUnreachable,
                        Some(&peer.name),
                        Some(&peer.endpoint.to_string()),
                        Some(&format!("no handshake for {unreachable_timeout_secs}s")),
                        Some(health_max_events),
                    );
                }
            }

            // Persist changes atomically
            if changed {
                for peer in &peers {
                    let _ = store::upsert_peer(peer);
                }
            }

            events::emit(
                EventType::HealthCheckRun,
                None,
                None,
                Some(&format!("peers_checked={}", peers.len())),
                Some(health_max_events),
            );
        }
    };

    // Reconciliation loop: compare stored peers with WireGuard config
    let reconcile_wg_pubkey = wg_pubkey.clone();
    let reconcile_recon = health_recon;
    let reconcile_max_events = tuning.max_events;
    let reconcile = async {
        let mut interval = tokio::time::interval(tuning.reconcile_interval);
        loop {
            interval.tick().await;

            let stored_peers = store::get_peers().unwrap_or_default();
            let wg_summary = match wg::interface_summary() {
                Ok(s) => s,
                Err(e) => {
                    warn!("reconciliation: WireGuard interface unavailable: {e}");
                    continue;
                }
            };

            // For each stored peer, ensure it's in WireGuard
            let wg_keys: Vec<String> = wg_summary
                .peers
                .iter()
                .map(|p| p.public_key.clone())
                .collect();
            let missing = peers_needing_reconciliation(&stored_peers, &wg_keys);
            for peer in missing {
                info!(peer = %peer.name, endpoint = %peer.endpoint, "reconciling: adding missing peer to WireGuard");
                if let Err(e) = wg::upsert_peer(&reconcile_wg_pubkey, peer) {
                    warn!(peer = %peer.name, error = %e, "reconciliation failed");
                } else {
                    reconcile_recon.fetch_add(1, Ordering::Relaxed);
                }
            }

            events::emit(
                EventType::ReconciliationRun,
                None,
                None,
                Some(&format!("stored_peers={}", stored_peers.len())),
                Some(reconcile_max_events),
            );
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
    events::emit(
        EventType::DaemonStopped,
        None,
        None,
        None,
        Some(tuning.max_events),
    );
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
    max_events: u64,
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
                        events::emit(
                            EventType::JoinManuallyAccepted,
                            Some(&info.node_name),
                            Some(&info.endpoint.to_string()),
                            None,
                            Some(self.max_events),
                        );
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
                match self.peering_state.reject(&request_id, reason.clone()).await {
                    Ok(()) => {
                        events::emit(
                            EventType::JoinRejected,
                            None,
                            None,
                            reason.as_deref(),
                            Some(self.max_events),
                        );
                        ControlResponse::Ok
                    }
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

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Evaluate a single peer's health state based on handshake data and timeout.
///
/// Updates `peer.last_seen` and `peer.status` as appropriate.
/// Returns `true` if any field was changed.
pub fn evaluate_peer_health(
    peer: &mut PeerRecord,
    wg_handshake_epoch: Option<u64>,
    current_time: u64,
    unreachable_timeout_secs: u64,
) -> bool {
    let mut changed = false;

    // Update last_seen from WireGuard handshake timestamp
    if let Some(handshake_epoch) = wg_handshake_epoch {
        if handshake_epoch > peer.last_seen {
            peer.last_seen = handshake_epoch;
            changed = true;
        }
    }

    // Recovery: unreachable → active if recent handshake
    if peer.status == PeerStatus::Unreachable
        && current_time.saturating_sub(peer.last_seen) < unreachable_timeout_secs
    {
        peer.status = PeerStatus::Active;
        changed = true;
    }

    // Detection: active → unreachable if no handshake for too long
    if peer.status == PeerStatus::Active
        && current_time.saturating_sub(peer.last_seen) > unreachable_timeout_secs
    {
        peer.status = PeerStatus::Unreachable;
        changed = true;
    }

    changed
}

/// Determine which stored peers are missing from WireGuard and need reconciliation.
///
/// Returns references to peers that should be re-added (non-Removed peers not in WG).
pub fn peers_needing_reconciliation<'a>(
    stored: &'a [PeerRecord],
    wg_keys: &[String],
) -> Vec<&'a PeerRecord> {
    stored
        .iter()
        .filter(|peer| peer.status != PeerStatus::Removed && !wg_keys.contains(&peer.wg_public_key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv6Addr, SocketAddr};
    use wireguard_control::KeyPair;

    fn sample_peer(name: &str, status: PeerStatus, last_seen: u64) -> PeerRecord {
        PeerRecord {
            name: name.to_string(),
            wg_public_key: format!("key-{name}"),
            endpoint: "203.0.113.1:51820".parse::<SocketAddr>().unwrap(),
            mesh_ipv6: Ipv6Addr::new(0xfd12, 0x3456, 0x7800, 0, 0, 0, 0, 1),
            last_seen,
            status,
            region: None,
            zone: None,
        }
    }

    // ── derive_prefix_from_secret tests ──

    #[test]
    fn derive_prefix_deterministic() {
        let secret = MeshSecret::from_bytes([42u8; 32]);
        let p1 = derive_prefix_from_secret(&secret);
        let p2 = derive_prefix_from_secret(&secret);
        assert_eq!(p1, p2);
    }

    #[test]
    fn derive_prefix_unique_for_different_secrets() {
        let s1 = MeshSecret::from_bytes([1u8; 32]);
        let s2 = MeshSecret::from_bytes([2u8; 32]);
        let p1 = derive_prefix_from_secret(&s1);
        let p2 = derive_prefix_from_secret(&s2);
        assert_ne!(p1, p2);
    }

    #[test]
    fn derive_prefix_known_value() {
        // The prefix must be in the fd00::/8 ULA range
        let secret = MeshSecret::from_bytes([0u8; 32]);
        let prefix = derive_prefix_from_secret(&secret);
        let segments = prefix.segments();
        // First nibble of first segment must be 0xfd
        assert_eq!(
            segments[0] >> 8,
            0xfd,
            "prefix must be in fd00::/8 ULA range"
        );
        // Last 4 segments must be zero (it's a /48 prefix)
        assert_eq!(segments[4], 0);
        assert_eq!(segments[5], 0);
        assert_eq!(segments[6], 0);
        assert_eq!(segments[7], 0);
    }

    // ── build_record tests ──

    #[test]
    fn build_record_fields() {
        let keypair = KeyPair::generate();
        let endpoint: SocketAddr = "10.0.0.1:51820".parse().unwrap();
        let ipv6 = Ipv6Addr::new(0xfd12, 0, 0, 0, 0, 0, 0, 1);

        let record = build_record(
            "node-1",
            &keypair,
            endpoint,
            ipv6,
            Some("us-east"),
            Some("us-east-1a"),
        );

        assert_eq!(record.name, "node-1");
        assert_eq!(record.wg_public_key, keypair.public.to_base64());
        assert_eq!(record.endpoint, endpoint);
        assert_eq!(record.mesh_ipv6, ipv6);
        assert_eq!(record.status, PeerStatus::Active);
        assert_eq!(record.region.as_deref(), Some("us-east"));
        assert_eq!(record.zone.as_deref(), Some("us-east-1a"));
    }

    #[test]
    fn build_record_timestamp_is_recent() {
        let keypair = KeyPair::generate();
        let endpoint: SocketAddr = "10.0.0.1:51820".parse().unwrap();
        let ipv6 = Ipv6Addr::new(0xfd12, 0, 0, 0, 0, 0, 0, 1);

        let before = now();
        let record = build_record("node-1", &keypair, endpoint, ipv6, None, None);
        let after = now();

        assert!(record.last_seen >= before);
        assert!(record.last_seen <= after);
    }

    // ── resolve_endpoint tests ──

    #[test]
    fn resolve_endpoint_default_fallback() {
        let config = DaemonConfig {
            mesh_name: "test".into(),
            node_name: "node".into(),
            wg_listen_port: 51820,
            public_endpoint: None,
            peering_port: 7946,
            region: None,
            zone: None,
        };
        let ep = resolve_endpoint(&config);
        assert_eq!(ep, "0.0.0.0:51820".parse::<SocketAddr>().unwrap());
    }

    #[test]
    fn resolve_endpoint_custom() {
        let custom: SocketAddr = "203.0.113.5:9999".parse().unwrap();
        let config = DaemonConfig {
            mesh_name: "test".into(),
            node_name: "node".into(),
            wg_listen_port: 51820,
            public_endpoint: Some(custom),
            peering_port: 7946,
            region: None,
            zone: None,
        };
        let ep = resolve_endpoint(&config);
        assert_eq!(ep, custom);
    }

    // ── now() test ──

    #[test]
    fn now_returns_reasonable_value() {
        let t = now();
        // Should be after 2024-01-01 and before 2100-01-01
        assert!(t > 1_704_067_200, "now() should be after 2024-01-01");
        assert!(t < 4_102_444_800, "now() should be before 2100-01-01");
    }

    // ── evaluate_peer_health tests ──

    #[test]
    fn health_active_to_unreachable_after_timeout() {
        let mut peer = sample_peer("node-2", PeerStatus::Active, 1000);
        // current_time = 1301 (301s after last_seen), timeout = 300
        let changed = evaluate_peer_health(&mut peer, None, 1301, 300);
        assert!(changed);
        assert_eq!(peer.status, PeerStatus::Unreachable);
    }

    #[test]
    fn health_active_stays_active_before_timeout() {
        let mut peer = sample_peer("node-2", PeerStatus::Active, 1000);
        // current_time = 1299 (299s after last_seen), timeout = 300
        let changed = evaluate_peer_health(&mut peer, None, 1299, 300);
        assert!(!changed);
        assert_eq!(peer.status, PeerStatus::Active);
    }

    #[test]
    fn health_active_stays_active_at_exact_boundary() {
        let mut peer = sample_peer("node-2", PeerStatus::Active, 1000);
        // current_time = 1300 (exactly 300s after last_seen), timeout = 300
        // saturating_sub(1300, 1000) = 300, which is NOT > 300, so stays active
        let changed = evaluate_peer_health(&mut peer, None, 1300, 300);
        assert!(!changed);
        assert_eq!(peer.status, PeerStatus::Active);
    }

    #[test]
    fn health_unreachable_to_active_on_recent_handshake() {
        let mut peer = sample_peer("node-2", PeerStatus::Unreachable, 1000);
        // Handshake at 1260 (current - 1260 = 40s ago), timeout = 300
        let changed = evaluate_peer_health(&mut peer, Some(1260), 1300, 300);
        assert!(changed);
        assert_eq!(peer.status, PeerStatus::Active);
        assert_eq!(peer.last_seen, 1260);
    }

    #[test]
    fn health_unreachable_stays_unreachable_no_recent_handshake() {
        let mut peer = sample_peer("node-2", PeerStatus::Unreachable, 1000);
        // current_time = 1400, no new handshake, timeout = 300
        // 1400 - 1000 = 400 >= 300, stays unreachable
        let changed = evaluate_peer_health(&mut peer, None, 1400, 300);
        assert!(!changed);
        assert_eq!(peer.status, PeerStatus::Unreachable);
    }

    // ── last_seen update tests ──

    #[test]
    fn health_updates_last_seen_from_newer_handshake() {
        let mut peer = sample_peer("node-2", PeerStatus::Active, 1000);
        let changed = evaluate_peer_health(&mut peer, Some(1050), 1100, 300);
        assert!(changed);
        assert_eq!(peer.last_seen, 1050);
    }

    #[test]
    fn health_does_not_update_last_seen_from_older_handshake() {
        let mut peer = sample_peer("node-2", PeerStatus::Active, 1000);
        let changed = evaluate_peer_health(&mut peer, Some(900), 1100, 300);
        assert!(!changed);
        assert_eq!(peer.last_seen, 1000);
    }

    #[test]
    fn health_no_handshake_no_update() {
        let mut peer = sample_peer("node-2", PeerStatus::Active, 1000);
        let changed = evaluate_peer_health(&mut peer, None, 1100, 300);
        assert!(!changed);
        assert_eq!(peer.last_seen, 1000);
    }

    // ── peers_needing_reconciliation tests ──

    #[test]
    fn reconciliation_missing_peer_needs_readd() {
        let peers = vec![
            sample_peer("node-1", PeerStatus::Active, 1000),
            sample_peer("node-2", PeerStatus::Active, 1000),
        ];
        let wg_keys = vec!["key-node-1".to_string()]; // node-2 is missing
        let needing = peers_needing_reconciliation(&peers, &wg_keys);
        assert_eq!(needing.len(), 1);
        assert_eq!(needing[0].name, "node-2");
    }

    #[test]
    fn reconciliation_present_peer_no_action() {
        let peers = vec![sample_peer("node-1", PeerStatus::Active, 1000)];
        let wg_keys = vec!["key-node-1".to_string()];
        let needing = peers_needing_reconciliation(&peers, &wg_keys);
        assert!(needing.is_empty());
    }

    #[test]
    fn reconciliation_removed_peer_not_readded() {
        let peers = vec![sample_peer("node-1", PeerStatus::Removed, 1000)];
        let wg_keys = vec![]; // not in WG, but it's Removed
        let needing = peers_needing_reconciliation(&peers, &wg_keys);
        assert!(needing.is_empty());
    }
}
