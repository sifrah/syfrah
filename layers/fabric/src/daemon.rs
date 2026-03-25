use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};
use wireguard_control::{Key, KeyPair};

use syfrah_core::addressing;
use syfrah_core::mesh::{PeerRecord, PeerStatus};
use syfrah_core::secret::MeshSecret;

use crate::config::{self, Tuning};
use crate::control::{self, ControlHandler, ControlRequest, ControlResponse};
use crate::events::{self, EventType};
use crate::peering::{self, AutoAcceptConfig, PeeringState};
use crate::sanitize::sanitize;
use crate::store::{self, NodeState};
use crate::ui;
use crate::wg;

/// Default region used when the operator does not specify `--region`.
pub const DEFAULT_REGION: &str = "default";

/// Resolve region and zone from optional user-provided values.
///
/// - If `region` is `None`, falls back to [`DEFAULT_REGION`].
/// - If `zone` is `None`, auto-generates one via [`store::generate_zone`].
///
/// This is the single source of truth used by `setup_init`, `setup_join`,
/// `auto_init`, and the join-accept handler.
pub fn resolve_region_zone(
    region: Option<&str>,
    zone: Option<&str>,
    existing_peers: &[syfrah_core::mesh::PeerRecord],
) -> (String, String) {
    let region = region
        .map(|r| r.to_string())
        .unwrap_or_else(|| DEFAULT_REGION.to_string());
    let zone = zone
        .map(|z| z.to_string())
        .unwrap_or_else(|| store::generate_zone(&region, existing_peers));
    (region, zone)
}

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
        anyhow::bail!("mesh state already exists. Run 'syfrah fabric leave' first.");
    }

    let sp = ui::spinner("Generating mesh secret...");
    let mesh_secret = MeshSecret::generate();
    let wg_keypair = wg::generate_keypair();
    let mesh_prefix = derive_prefix_from_secret(&mesh_secret);
    let mesh_ipv6 = addressing::derive_node_address(&mesh_prefix, wg_keypair.public.as_bytes());
    let endpoint = resolve_endpoint(config);
    ui::step_ok(&sp, &format!("Secret: {mesh_secret}"));

    let sp = ui::spinner("Setting up WireGuard interface...");
    wg::setup_interface(&wg_keypair, config.wg_listen_port, mesh_ipv6)?;
    ui::step_ok(&sp, &format!("Interface syfrah0 up ({mesh_ipv6})"));
    info!(flow = "init", mesh = %config.mesh_name, node = %config.node_name, "wireguard interface up");

    // Region/zone: use provided or defaults
    let (region, zone) = resolve_region_zone(config.region.as_deref(), config.zone.as_deref(), &[]);
    if config.region.is_none() {
        ui::warn("No --region specified; using 'default'. Set --region to label this node.");
    }

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

    let sp = ui::spinner("Starting daemon...");
    ui::step_ok(&sp, &format!("Mesh '{}' created", config.mesh_name));
    ui::info_line("Node", &format!("{} ({mesh_ipv6})", config.node_name));
    ui::info_line("Region", &region);
    ui::info_line("Zone", &zone);
    println!();
    println!("  \u{26a0} Peering is not active. New nodes cannot join yet.");
    println!("    To accept nodes with a PIN:  syfrah fabric peering start --pin <PIN>");
    println!("    To approve manually:         syfrah fabric peering start");

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

/// Auto-init: create mesh if none exists, used by `syfrah fabric peering` on a fresh node.
pub fn auto_init(
    node_name: &str,
    wg_port: u16,
    peering_port: u16,
) -> anyhow::Result<(MeshSecret, KeyPair)> {
    let mesh_secret = MeshSecret::generate();
    let wg_keypair = wg::generate_keypair();

    let mesh_prefix = derive_prefix_from_secret(&mesh_secret);
    let mesh_ipv6 = addressing::derive_node_address(&mesh_prefix, wg_keypair.public.as_bytes());

    let sp = ui::spinner("Setting up WireGuard interface...");
    wg::setup_interface(&wg_keypair, wg_port, mesh_ipv6)?;
    ui::step_ok(&sp, &format!("Interface syfrah0 up ({mesh_ipv6})"));

    let (region, zone) = resolve_region_zone(None, None, &[]);

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
        region: Some(region),
        zone: Some(zone),
        metrics: Default::default(),
    };
    store::save(&state)?;

    let sp = ui::spinner("Auto-creating mesh...");
    ui::step_ok(&sp, "Mesh auto-created");
    ui::info_line("Secret", &mesh_secret.to_string());
    ui::info_line("Node", &format!("{node_name} ({mesh_ipv6})"));

    Ok((mesh_secret, wg_keypair))
}

/// Setup the join flow: send join request, save state, print info.
/// Returns a DaemonReady that can be passed to run_daemon.
///
/// On failure, any partial state (WireGuard interface, persisted state) is
/// rolled back so the user can retry without running `leave` first.
pub async fn setup_join(
    target: SocketAddr,
    config: &DaemonConfig,
    pin: Option<String>,
) -> anyhow::Result<DaemonReady> {
    if store::exists() {
        anyhow::bail!("mesh state already exists. Run 'syfrah fabric leave' first.");
    }

    let sp = ui::spinner(&format!("Connecting to {target}..."));
    let wg_keypair = wg::generate_keypair();
    let endpoint = resolve_endpoint(config);

    // Send region/zone in the request so the leader can store them.
    // If the user provided explicit values, include them; otherwise send
    // the region default and leave zone as None so the leader can
    // auto-generate it from its peer list.
    let req_region = Some(
        config
            .region
            .as_deref()
            .unwrap_or(DEFAULT_REGION)
            .to_string(),
    );
    if config.region.is_none() {
        ui::warn("No --region specified; using 'default'. Set --region to label this node.");
    }
    let req_zone = config.zone.clone();

    let request = syfrah_core::mesh::JoinRequest {
        request_id: peering::generate_request_id(),
        node_name: config.node_name.clone(),
        wg_public_key: wg_keypair.public.to_base64(),
        endpoint,
        wg_listen_port: config.wg_listen_port,
        pin,
        region: req_region,
        zone: req_zone,
    };
    ui::step_ok(&sp, &format!("Connected to {target}"));

    let sp = ui::spinner("Waiting for approval...");
    let response = match peering::send_join_request(target, request).await {
        Ok(resp) => resp,
        Err(e) => {
            ui::step_fail(&sp, &format!("Failed: {e}"));
            return Err(map_join_error(e, target));
        }
    };

    if !response.accepted {
        let reason = response.reason.unwrap_or_else(|| "no reason given".into());
        ui::step_fail(&sp, &format!("Rejected: {reason}"));
        anyhow::bail!("Join request rejected: {reason}");
    }
    ui::step_ok(&sp, "Approved");

    // Everything below writes state — wrap in rollback on error.
    match finalize_join(config, &wg_keypair, endpoint, &response).await {
        Ok(ready) => Ok(ready),
        Err(e) => {
            warn!(flow = "join", error = %e, "join failed after approval, rolling back");
            rollback_join_state();
            Err(e)
        }
    }
}

/// Finalize the join after approval: setup WG interface, save state, print info.
/// Separated from `setup_join` so the caller can rollback on any error here.
async fn finalize_join(
    config: &DaemonConfig,
    wg_keypair: &KeyPair,
    endpoint: SocketAddr,
    response: &syfrah_core::mesh::JoinResponse,
) -> anyhow::Result<DaemonReady> {
    let mesh_secret_str = response
        .mesh_secret
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("accepted but no mesh secret"))?;
    let mesh_secret: MeshSecret = mesh_secret_str
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid mesh secret: {e}"))?;
    let mesh_name = response.mesh_name.as_deref().unwrap_or("mesh");
    let mesh_prefix = response
        .mesh_prefix
        .ok_or_else(|| anyhow::anyhow!("accepted but no mesh prefix"))?;

    let mesh_ipv6 = addressing::derive_node_address(&mesh_prefix, wg_keypair.public.as_bytes());

    // Region/zone: use provided or auto-generate from existing peers
    let (region, zone) = resolve_region_zone(
        config.region.as_deref(),
        config.zone.as_deref(),
        &response.peers,
    );

    let sp = ui::spinner("Setting up WireGuard interface...");
    wg::setup_interface(wg_keypair, config.wg_listen_port, mesh_ipv6)?;
    ui::step_ok(&sp, &format!("Interface syfrah0 up ({mesh_ipv6})"));
    info!(flow = "join", node = %config.node_name, "wireguard interface up");

    if !response.peers.is_empty() {
        let sp = ui::spinner("Syncing peers...");
        info!(
            flow = "join",
            count = response.peers.len(),
            "applying peers from join response"
        );
        if let Err(e) = wg::apply_peers(&wg_keypair.public, &response.peers) {
            warn!(flow = "join", error = %e, "failed to apply peers");
        }
        ui::step_ok(&sp, &format!("{} peers configured", response.peers.len()));
    }

    let state = NodeState {
        mesh_name: mesh_name.to_string(),
        mesh_secret: mesh_secret_str.clone(),
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

    let sp = ui::spinner("Starting daemon...");
    ui::step_ok(&sp, &format!("Joined mesh '{mesh_name}'"));
    match response.approved_by.as_deref() {
        Some("pin") => ui::info_line("Approval", "PIN accepted by the target node"),
        Some("manual") => {
            ui::info_line("Approval", "Approved by the target node (manual approval)")
        }
        _ => ui::info_line("Approval", "Approved by the target node"),
    }
    ui::info_line("Node", &format!("{} ({mesh_ipv6})", config.node_name));
    ui::info_line("Region", &region);
    ui::info_line("Zone", &zone);
    println!();
    ui::warn("The mesh secret is stored in ~/.syfrah/state.json");
    println!("    Keep this file safe \u{2014} it grants full mesh access.");

    let my_record = build_record(
        &config.node_name,
        wg_keypair,
        endpoint,
        mesh_ipv6,
        Some(&region),
        Some(&zone),
    );
    Ok(DaemonReady {
        my_record,
        wg_keypair: KeyPair::from_private(wg_keypair.private.clone()),
        mesh_secret,
        peering_port: config.peering_port,
    })
}

/// Roll back any partial state left by a failed join attempt.
/// Tears down WireGuard interface and clears persisted state so the user
/// can retry without running `leave` first.
fn rollback_join_state() {
    if let Err(e) = wg::teardown_interface() {
        debug!(error = %e, "rollback: no interface to tear down (expected)");
    }
    if store::exists() {
        if let Err(e) = store::clear() {
            warn!(error = %e, "rollback: failed to clear state");
        }
    }
}

/// Map peering errors during join to user-friendly messages.
fn map_join_error(err: peering::PeeringError, target: SocketAddr) -> anyhow::Error {
    match &err {
        peering::PeeringError::Io(io_err) if io_err.kind() == std::io::ErrorKind::UnexpectedEof => {
            anyhow::anyhow!(
                "Connection closed by {target}. The target node may not have peering active.\n  \
                 Ask the operator to run: syfrah fabric peering start"
            )
        }
        peering::PeeringError::Timeout => {
            anyhow::anyhow!(
                "Connection to {target} timed out. The target node may not be reachable or peering may not be active.\n  \
                 Ask the operator to run: syfrah fabric peering start"
            )
        }
        _ => err.into(),
    }
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
        anyhow::anyhow!(
            "no mesh state found. Run 'syfrah fabric init' or 'syfrah fabric join' first."
        )
    })?;

    let mesh_secret: MeshSecret = state
        .mesh_secret
        .parse()
        .map_err(|e| anyhow::anyhow!("corrupt secret in state: {e}"))?;
    let wg_private = Key::from_base64(&state.wg_private_key)
        .map_err(|_| anyhow::anyhow!("corrupt WG private key in state"))?;
    let wg_keypair = KeyPair::from_private(wg_private);

    let sp = ui::spinner("Setting up WireGuard interface...");
    wg::setup_interface(&wg_keypair, state.wg_listen_port, state.mesh_ipv6)?;
    ui::step_ok(&sp, &format!("Interface syfrah0 up ({})", state.mesh_ipv6));

    if !state.peers.is_empty() {
        let sp_peers = ui::spinner("Syncing peers...");
        info!(
            flow = "start",
            count = state.peers.len(),
            "applying peers from saved state"
        );
        if let Err(e) = wg::apply_peers(&wg_keypair.public, &state.peers) {
            warn!(flow = "start", error = %e, "failed to apply saved peers");
        }
        ui::step_ok(
            &sp_peers,
            &format!("{} peers configured", state.peers.len()),
        );
    }

    let sp = ui::spinner("Starting daemon...");
    ui::step_ok(&sp, &format!("Restarting mesh '{}'", state.mesh_name));
    ui::info_line(
        "Node",
        &format!("{} ({})", state.node_name, state.mesh_ipv6),
    );

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
///
/// Returns `true` if there was a mesh to leave, `false` if nothing was configured.
pub async fn run_leave() -> anyhow::Result<bool> {
    // Stop the daemon first (if running) so it cannot recreate state files.
    if let Some(pid) = store::daemon_running() {
        #[cfg(unix)]
        {
            if store::is_syfrah_process(pid) {
                let sp = ui::spinner("Stopping daemon...");
                unsafe { libc::kill(pid as i32, libc::SIGTERM) };
                // Wait for the daemon process to actually exit (up to 10s).
                // Just sleeping a fixed 2s was not enough — the daemon may
                // still hold file locks when we try to remove the state dir.
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
                loop {
                    let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
                    if !alive {
                        break;
                    }
                    if tokio::time::Instant::now() >= deadline {
                        // Force-kill if it didn't exit gracefully.
                        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                ui::step_ok(&sp, "Daemon stopped");
            }
        }
    }

    if !store::exists() {
        // Even when no mesh state exists, clean up any leftover runtime files
        // (PID file, control socket) that might remain from a partial cleanup.
        store::remove_pid();
        let _ = std::fs::remove_file(store::control_socket_path());
        return Ok(false);
    }

    let sp = ui::spinner("Tearing down WireGuard interface...");
    if let Err(e) = wg::teardown_interface() {
        ui::step_fail(&sp, &format!("Could not tear down interface: {e}"));
    } else {
        ui::step_ok(&sp, "Interface removed");
    }
    let sp = ui::spinner("Cleaning up state...");
    // Clear all state atomically: redb, JSON, PID file, control socket, and
    // any other files in ~/.syfrah. store::clear() removes the entire directory.
    // Retry once after a short delay if the first attempt fails (e.g. a file
    // lock was not yet released by the dying daemon process).
    if let Err(e) = store::clear() {
        debug!(error = %e, "first clear attempt failed, retrying");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        store::clear()?;
    }
    ui::step_ok(&sp, "Left the mesh. State cleared.");
    Ok(true)
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
        "daemon tuning: health_check={}s reconcile={}s persist={}s unreachable={}s max_peers={} max_concurrent_announces={}",
        tuning.health_check_interval.as_secs(),
        tuning.reconcile_interval.as_secs(),
        tuning.persist_interval.as_secs(),
        tuning.unreachable_timeout.as_secs(),
        tuning.max_peers,
        tuning.max_concurrent_announces,
    );

    let announce_semaphore = Arc::new(Semaphore::new(tuning.max_concurrent_announces));
    let max_peers = tuning.max_peers;

    // Acquire exclusive PID file lock. The returned file handle must be kept
    // alive for the entire daemon lifetime — dropping it releases the flock.
    let _pid_lock = store::write_pid()?;
    events::emit(
        EventType::DaemonStarted,
        None,
        None,
        None,
        Some(tuning.max_events),
    );

    let wg_pubkey = wg_keypair.public.clone();
    let peering_state = Arc::new(PeeringState::with_limits(
        tuning.max_concurrent_connections,
        tuning.max_pending_joins,
    ));
    let enc_key = mesh_secret.encryption_key();

    let metrics_received = Arc::new(AtomicU64::new(0));
    let metrics_reconciliations = Arc::new(AtomicU64::new(0));
    let metrics_unreachable = Arc::new(AtomicU64::new(0));
    let metrics_announces_dropped = Arc::new(AtomicU64::new(0));
    let metrics_peer_limit_reached = Arc::new(AtomicU64::new(0));
    let metrics_health_check_failures = Arc::new(AtomicU64::new(0));
    let metrics_reconcile_failures = Arc::new(AtomicU64::new(0));
    let metrics_store_failures = Arc::new(AtomicU64::new(0));
    let daemon_started = now();

    // on_accepted callback: when a peer is accepted (manual or PIN), add to WG + store + announce
    let accepted_wg_pubkey = wg_pubkey.clone();
    let accepted_recv = metrics_received.clone();
    let accepted_recon = metrics_reconciliations.clone();
    let accepted_enc_key = enc_key;
    let accepted_peering_port = peering_port;
    let accepted_max_events = tuning.max_events;
    let accepted_max_peers = max_peers;
    let accepted_peer_limit_counter = metrics_peer_limit_reached.clone();
    let accepted_store_failures = metrics_store_failures.clone();
    let on_accepted: peering::OnAccepted = Arc::new(move |new_record| {
        accepted_recv.fetch_add(1, Ordering::Relaxed);
        let pubkey = accepted_wg_pubkey.clone();
        let recon = accepted_recon.clone();
        let record = new_record.clone();
        let enc = accepted_enc_key;
        let pp = accepted_peering_port;
        let max_ev = accepted_max_events;
        let mp = accepted_max_peers;
        let plr = accepted_peer_limit_counter.clone();
        let sf = accepted_store_failures.clone();
        tokio::spawn(async move {
            // Check store peer limit before adding
            let current_count = match store::peer_count() {
                Ok(c) => c,
                Err(e) => {
                    error!(error = %e, "on_accepted: failed to read peer count from store, aborting");
                    sf.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            };
            let threshold = mp * 4 / 5; // 80%
            if current_count >= threshold && current_count < mp {
                warn!(
                    current = current_count,
                    max = mp,
                    "peer count approaching limit ({}%)",
                    current_count * 100 / mp
                );
            }

            // Add to WG (bounded)
            match wg::upsert_peer_bounded(&pubkey, &record, mp) {
                Err(e) => {
                    if matches!(e, wg::WgError::PeerLimitExceeded(_, _)) {
                        plr.fetch_add(1, Ordering::Relaxed);
                        let _ = store::inc_metric("peer_limit_reached", 1);
                        events::emit(
                            EventType::PeerLimitReached,
                            Some(&sanitize(&record.name)),
                            Some(&record.endpoint.to_string()),
                            Some(&format!("max_peers={mp}")),
                            Some(max_ev),
                        );
                    }
                    warn!(peer = %sanitize(&record.name), endpoint = %record.endpoint, error = %e, "failed to add peer to WG");
                }
                Ok(()) => {
                    recon.fetch_add(1, Ordering::Relaxed);
                    info!(peer = %sanitize(&record.name), endpoint = %record.endpoint, "peer accepted and added to WG");
                    events::emit(
                        EventType::PeerActive,
                        Some(&sanitize(&record.name)),
                        Some(&record.endpoint.to_string()),
                        Some(&format!("mesh_ipv6={}", record.mesh_ipv6)),
                        Some(max_ev),
                    );
                }
            }
            // Save to store (bounded — reject if over limit)
            match store::upsert_peer_bounded(&record, mp) {
                Ok(false) => {
                    warn!(peer = %sanitize(&record.name), max = mp, "peer limit reached, not persisting new peer");
                    plr.fetch_add(1, Ordering::Relaxed);
                    let _ = store::inc_metric("peer_limit_reached", 1);
                    events::emit(
                        EventType::PeerLimitReached,
                        Some(&sanitize(&record.name)),
                        Some(&record.endpoint.to_string()),
                        Some(&format!("store max_peers={mp}")),
                        Some(max_ev),
                    );
                }
                Err(e) => {
                    warn!(peer = %sanitize(&record.name), error = %e, "failed to persist peer");
                }
                Ok(true) => {}
            }
            // Announce to existing peers
            let known = match store::get_peers() {
                Ok(peers) => peers,
                Err(e) => {
                    error!(error = %e, "on_accepted: failed to load peers for announce, skipping");
                    sf.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            };
            let (_ok, failed) = peering::announce_peer_to_mesh(&record, &known, &enc, pp).await;
            if failed > 0 {
                let _ = store::inc_metric("announcements_failed", failed as u64);
                events::emit(
                    EventType::PeerAnnounceFailed,
                    Some(&sanitize(&record.name)),
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
        max_peers,
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
    let announce_sem = announce_semaphore.clone();
    let announce_dropped = metrics_announces_dropped.clone();
    let announce_peer_limit = metrics_peer_limit_reached.clone();
    let announce_max_peers = max_peers;
    let announce_store_failures = metrics_store_failures.clone();
    let on_announce: Arc<dyn Fn(PeerRecord) + Send + Sync> = Arc::new(move |record| {
        announce_recv.fetch_add(1, Ordering::Relaxed);
        events::emit(
            EventType::PeerAnnounceReceived,
            Some(&sanitize(&record.name)),
            Some(&record.endpoint.to_string()),
            Some(&format!("mesh_ipv6={}", record.mesh_ipv6)),
            Some(announce_max_events),
        );

        // Bound concurrent announce processing with a semaphore
        let permit = match announce_sem.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                warn!(peer = %sanitize(&record.name), "announce processing at capacity, dropping");
                announce_dropped.fetch_add(1, Ordering::Relaxed);
                let _ = store::inc_metric("announces_dropped", 1);
                events::emit(
                    EventType::AnnounceDropped,
                    Some(&record.name),
                    Some(&record.endpoint.to_string()),
                    Some("semaphore full"),
                    Some(announce_max_events),
                );
                return;
            }
        };

        let pubkey = announce_wg_pubkey.clone();
        let recon = announce_recon.clone();
        let record = record.clone();
        let mp = announce_max_peers;
        let plr = announce_peer_limit.clone();
        let sf = announce_store_failures.clone();
        let max_ev = announce_max_events;
        tokio::spawn(async move {
            let _permit = permit; // held until task completes

            // Check store peer count before processing
            let current_count = match store::peer_count() {
                Ok(c) => c,
                Err(e) => {
                    error!(error = %e, "on_announce: failed to read peer count from store, aborting");
                    sf.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            };
            let threshold = mp * 4 / 5; // 80%
            if current_count >= threshold && current_count < mp {
                warn!(
                    current = current_count,
                    max = mp,
                    "peer count approaching limit ({}%)",
                    current_count * 100 / mp
                );
            }

            // Add to WG (bounded)
            match wg::upsert_peer_bounded(&pubkey, &record, mp) {
                Err(e) => {
                    if matches!(e, wg::WgError::PeerLimitExceeded(_, _)) {
                        plr.fetch_add(1, Ordering::Relaxed);
                        let _ = store::inc_metric("peer_limit_reached", 1);
                        events::emit(
                            EventType::PeerLimitReached,
                            Some(&sanitize(&record.name)),
                            Some(&record.endpoint.to_string()),
                            Some(&format!("max_peers={mp}")),
                            Some(max_ev),
                        );
                    }
                    warn!(peer = %sanitize(&record.name), endpoint = %record.endpoint, error = %e, "failed to upsert announced peer");
                }
                Ok(()) => {
                    recon.fetch_add(1, Ordering::Relaxed);
                    debug!(peer = %sanitize(&record.name), endpoint = %record.endpoint, "peer upserted via announce");
                }
            }
            // Save to store (bounded)
            match store::upsert_peer_bounded(&record, mp) {
                Ok(false) => {
                    warn!(peer = %sanitize(&record.name), max = mp, "peer limit reached, not persisting announced peer");
                    plr.fetch_add(1, Ordering::Relaxed);
                    let _ = store::inc_metric("peer_limit_reached", 1);
                    events::emit(
                        EventType::PeerLimitReached,
                        Some(&sanitize(&record.name)),
                        Some(&record.endpoint.to_string()),
                        Some(&format!("store max_peers={mp}")),
                        Some(max_ev),
                    );
                }
                Err(e) => {
                    warn!(peer = %sanitize(&record.name), error = %e, "failed to persist announced peer");
                }
                Ok(true) => {}
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
    let persist_dropped = metrics_announces_dropped.clone();
    let persist_peer_limit = metrics_peer_limit_reached.clone();
    let persist_health_failures = metrics_health_check_failures.clone();
    let persist_reconcile_failures = metrics_reconcile_failures.clone();
    let persist_store_failures = metrics_store_failures.clone();
    let persist_peering_state = peering_state.clone();
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
            let _ = store::set_metric(
                "connections_rejected",
                persist_peering_state.connections_rejected(),
            );
            let _ = store::set_metric(
                "connections_active",
                persist_peering_state.connections_active(),
            );
            let _ = store::set_metric("announces_dropped", persist_dropped.load(Ordering::Relaxed));
            let _ = store::set_metric(
                "peer_limit_reached",
                persist_peer_limit.load(Ordering::Relaxed),
            );
            if let Err(e) = store::set_metric(
                "health_check_failures",
                persist_health_failures.load(Ordering::Relaxed),
            ) {
                debug!(error = %e, "failed to persist health_check_failures metric");
            }
            if let Err(e) = store::set_metric(
                "reconcile_failures",
                persist_reconcile_failures.load(Ordering::Relaxed),
            ) {
                debug!(error = %e, "failed to persist reconcile_failures metric");
            }
            if let Err(e) = store::set_metric(
                "store_failures",
                persist_store_failures.load(Ordering::Relaxed),
            ) {
                debug!(error = %e, "failed to persist store_failures metric");
            }
        }
    };

    // Health check: unreachable detection + recovery + last_seen update
    let unreachable_timeout_secs = tuning.unreachable_timeout.as_secs();
    let health_counter = metrics_unreachable.clone();
    let health_recon = metrics_reconciliations.clone();
    let health_max_events = tuning.max_events;
    let health_failures = metrics_health_check_failures.clone();
    let health_check = async {
        let mut interval = tokio::time::interval(tuning.health_check_interval);
        loop {
            interval.tick().await;

            // Get WireGuard handshake data for all peers
            let wg_peers = match wg::interface_summary() {
                Ok(s) => s.peers,
                Err(e) => {
                    warn!(error = %e, "health check: WireGuard interface unavailable");
                    health_failures.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            };

            let mut peers = match store::get_peers() {
                Ok(p) => p,
                Err(e) => {
                    error!(error = %e, "health check: failed to load peers from store");
                    health_failures.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            };
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
                        info!(peer = %sanitize(&peer.name), last_seen = peer.last_seen, "peer recovered, marking active");
                    }
                    if old_status == PeerStatus::Active && peer.status == PeerStatus::Unreachable {
                        info!(peer = %sanitize(&peer.name), last_seen = peer.last_seen, timeout_secs = unreachable_timeout_secs, "marking peer as unreachable");
                        health_counter.fetch_add(1, Ordering::Relaxed);
                    }
                }

                // Recovery: unreachable → active if recent handshake
                if peer.status == PeerStatus::Unreachable
                    && current.saturating_sub(peer.last_seen) < unreachable_timeout_secs
                {
                    info!(peer = %sanitize(&peer.name), last_seen = peer.last_seen, "peer recovered, marking active");
                    peer.status = PeerStatus::Active;
                    changed = true;
                    events::emit(
                        EventType::PeerRecovered,
                        Some(&sanitize(&peer.name)),
                        Some(&peer.endpoint.to_string()),
                        Some("handshake resumed"),
                        Some(health_max_events),
                    );
                }

                // Detection: active → unreachable if no handshake for too long
                if peer.status == PeerStatus::Active
                    && current.saturating_sub(peer.last_seen) > unreachable_timeout_secs
                {
                    info!(peer = %sanitize(&peer.name), last_seen = peer.last_seen, timeout_secs = unreachable_timeout_secs, "marking peer as unreachable");
                    peer.status = PeerStatus::Unreachable;
                    health_counter.fetch_add(1, Ordering::Relaxed);
                    changed = true;
                    events::emit(
                        EventType::PeerUnreachable,
                        Some(&sanitize(&peer.name)),
                        Some(&peer.endpoint.to_string()),
                        Some(&format!("no handshake for {unreachable_timeout_secs}s")),
                        Some(health_max_events),
                    );
                }
            }

            // Persist changes atomically
            if changed {
                for peer in &peers {
                    if let Err(e) = store::upsert_peer(peer) {
                        warn!(peer = %sanitize(&peer.name), error = %e, "health check: failed to persist peer state");
                        health_failures.fetch_add(1, Ordering::Relaxed);
                    }
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
    let reconcile_failures = metrics_reconcile_failures.clone();
    let reconcile = async {
        let mut interval = tokio::time::interval(tuning.reconcile_interval);
        loop {
            interval.tick().await;

            let stored_peers = match store::get_peers() {
                Ok(p) => p,
                Err(e) => {
                    error!(error = %e, "reconciliation: failed to load peers from store");
                    reconcile_failures.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            };
            let wg_summary = match wg::interface_summary() {
                Ok(s) => s,
                Err(e) => {
                    warn!(error = %e, "reconciliation: WireGuard interface unavailable");
                    reconcile_failures.fetch_add(1, Ordering::Relaxed);
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
                info!(peer = %sanitize(&peer.name), endpoint = %peer.endpoint, "reconciling: adding missing peer to WireGuard");
                if let Err(e) = wg::upsert_peer(&reconcile_wg_pubkey, peer) {
                    warn!(peer = %sanitize(&peer.name), error = %e, "reconciliation failed");
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

    // Listen for both SIGINT (Ctrl+C) and SIGTERM so the daemon shuts down
    // gracefully in either case. Without SIGTERM handling, `run_leave` sends
    // SIGTERM but the daemon dies immediately without cleanup.
    #[cfg(unix)]
    let terminate = async {
        let mut sig = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to register SIGTERM handler");
        sig.recv().await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = control_task => {}
        _ = peering_task => {}
        _ = persist => {}
        _ = health_check => {}
        _ = reconcile => {}
        _ = tokio::signal::ctrl_c() => {
            info!("received SIGINT, shutting down");
        }
        _ = terminate => {
            info!("received SIGTERM, shutting down");
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
    max_peers: usize,
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
                            max_peers: self.max_peers,
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
                    approved_by: Some("manual".into()),
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
                        // Use the joiner's region/zone from the request.
                        // If zone was not provided, auto-generate one
                        // using the current peer list.
                        let (region, zone) = resolve_region_zone(
                            info.region.as_deref(),
                            info.zone.as_deref(),
                            &state.peers,
                        );
                        let new_record = PeerRecord {
                            name: info.node_name.clone(),
                            wg_public_key: info.wg_public_key,
                            endpoint: info.endpoint,
                            mesh_ipv6: new_mesh_ipv6,
                            last_seen: now(),
                            status: PeerStatus::Active,
                            region: Some(region),
                            zone: Some(zone),
                        };
                        events::emit(
                            EventType::JoinManuallyAccepted,
                            Some(&sanitize(&info.node_name)),
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

    // ── map_join_error tests ──

    #[test]
    fn map_join_error_unexpected_eof_is_friendly() {
        let io_err = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "early eof");
        let peering_err = crate::peering::PeeringError::Io(io_err);
        let target: SocketAddr = "203.0.113.1:51821".parse().unwrap();
        let mapped = map_join_error(peering_err, target);
        let msg = mapped.to_string();
        assert!(
            msg.contains("Connection closed by"),
            "expected friendly message, got: {msg}"
        );
        assert!(
            msg.contains("peering"),
            "should suggest peering, got: {msg}"
        );
    }

    #[test]
    fn map_join_error_timeout_is_friendly() {
        let peering_err = crate::peering::PeeringError::Timeout;
        let target: SocketAddr = "203.0.113.1:51821".parse().unwrap();
        let mapped = map_join_error(peering_err, target);
        let msg = mapped.to_string();
        assert!(
            msg.contains("timed out"),
            "expected timeout message, got: {msg}"
        );
        assert!(
            msg.contains("peering"),
            "should suggest peering, got: {msg}"
        );
    }

    #[test]
    fn map_join_error_other_passes_through() {
        let peering_err = crate::peering::PeeringError::Protocol("something weird".into());
        let target: SocketAddr = "203.0.113.1:51821".parse().unwrap();
        let mapped = map_join_error(peering_err, target);
        let msg = mapped.to_string();
        assert!(
            msg.contains("something weird"),
            "other errors pass through, got: {msg}"
        );
    }

    #[test]
    fn rollback_join_state_is_safe_when_no_state() {
        // rollback should not panic even when there's nothing to clean up
        rollback_join_state();
    }

    // ── resolve_region_zone tests (exercises the real production function) ──

    #[test]
    fn resolve_region_zone_defaults_when_none() {
        let (region, zone) = super::resolve_region_zone(None, None, &[]);
        assert_eq!(
            region, "default",
            "region should fall back to DEFAULT_REGION"
        );
        assert_eq!(
            zone, "zone-1",
            "zone should be auto-generated as zone-1 with no peers"
        );
    }

    #[test]
    fn resolve_region_zone_explicit_region_overrides_default() {
        let (region, zone) = super::resolve_region_zone(Some("us-east"), None, &[]);
        assert_eq!(region, "us-east");
        assert_eq!(zone, "zone-1", "zone should still be auto-generated");
    }

    #[test]
    fn resolve_region_zone_explicit_zone_overrides_generation() {
        let (region, zone) = super::resolve_region_zone(Some("eu-west"), Some("eu-west-a"), &[]);
        assert_eq!(region, "eu-west");
        assert_eq!(zone, "eu-west-a", "explicit zone should be used as-is");
    }

    #[test]
    fn resolve_region_zone_increments_with_existing_peers() {
        use syfrah_core::mesh::{PeerRecord, PeerStatus};
        let peers = vec![PeerRecord {
            name: "node-1".into(),
            wg_public_key: "key".into(),
            endpoint: "127.0.0.1:51820".parse().unwrap(),
            mesh_ipv6: "fd00::1".parse().unwrap(),
            last_seen: 0,
            status: PeerStatus::Active,
            region: Some("default".into()),
            zone: Some("zone-1".into()),
        }];
        let (region, zone) = super::resolve_region_zone(None, None, &peers);
        assert_eq!(region, "default");
        assert_eq!(
            zone, "zone-2",
            "zone index should increment past existing peers"
        );
    }

    // ── reconciliation edge cases ──

    #[test]
    fn reconciliation_empty_stored_peers() {
        let peers: Vec<PeerRecord> = vec![];
        let wg_keys = vec!["key-node-1".to_string()];
        let needing = peers_needing_reconciliation(&peers, &wg_keys);
        assert!(needing.is_empty());
    }

    #[test]
    fn reconciliation_empty_wg_keys_returns_all_active() {
        let peers = vec![
            sample_peer("node-1", PeerStatus::Active, 1000),
            sample_peer("node-2", PeerStatus::Active, 1000),
        ];
        let wg_keys: Vec<String> = vec![];
        let needing = peers_needing_reconciliation(&peers, &wg_keys);
        assert_eq!(needing.len(), 2);
    }

    #[test]
    fn reconciliation_unreachable_peer_still_reconciled() {
        // Unreachable peers should be re-added to WG (only Removed are skipped)
        let peers = vec![sample_peer("node-1", PeerStatus::Unreachable, 1000)];
        let wg_keys: Vec<String> = vec![];
        let needing = peers_needing_reconciliation(&peers, &wg_keys);
        assert_eq!(needing.len(), 1);
        assert_eq!(needing[0].name, "node-1");
    }

    #[test]
    fn reconciliation_mixed_statuses() {
        let peers = vec![
            sample_peer("active-1", PeerStatus::Active, 1000),
            sample_peer("unreach-1", PeerStatus::Unreachable, 1000),
            sample_peer("removed-1", PeerStatus::Removed, 1000),
        ];
        // None are in WG
        let wg_keys: Vec<String> = vec![];
        let needing = peers_needing_reconciliation(&peers, &wg_keys);
        // Only active and unreachable should be reconciled, not removed
        assert_eq!(needing.len(), 2);
        let names: Vec<&str> = needing.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"active-1"));
        assert!(names.contains(&"unreach-1"));
        assert!(!names.contains(&"removed-1"));
    }

    // ── health check lifecycle ──

    #[test]
    fn health_full_lifecycle_active_unreachable_active() {
        let mut peer = sample_peer("node-2", PeerStatus::Active, 1000);

        // Step 1: active, still within timeout — no change
        let changed = evaluate_peer_health(&mut peer, None, 1100, 300);
        assert!(!changed);
        assert_eq!(peer.status, PeerStatus::Active);

        // Step 2: active → unreachable (timeout exceeded)
        let changed = evaluate_peer_health(&mut peer, None, 1301, 300);
        assert!(changed);
        assert_eq!(peer.status, PeerStatus::Unreachable);

        // Step 3: unreachable → active (fresh handshake arrives)
        let changed = evaluate_peer_health(&mut peer, Some(1350), 1360, 300);
        assert!(changed);
        assert_eq!(peer.status, PeerStatus::Active);
        assert_eq!(peer.last_seen, 1350);
    }

    #[test]
    fn health_multiple_peers_independent() {
        let mut peer_a = sample_peer("node-a", PeerStatus::Active, 1000);
        let mut peer_b = sample_peer("node-b", PeerStatus::Active, 1200);

        // At time 1301: peer_a times out (1301-1000=301>300), peer_b is fine (1301-1200=101<300)
        let a_changed = evaluate_peer_health(&mut peer_a, None, 1301, 300);
        let b_changed = evaluate_peer_health(&mut peer_b, None, 1301, 300);

        assert!(a_changed);
        assert_eq!(peer_a.status, PeerStatus::Unreachable);
        assert!(!b_changed);
        assert_eq!(peer_b.status, PeerStatus::Active);
    }

    #[test]
    fn health_handshake_at_exact_last_seen_no_update() {
        let mut peer = sample_peer("node-2", PeerStatus::Active, 1000);
        // Handshake at exactly last_seen — not newer, so no update
        let changed = evaluate_peer_health(&mut peer, Some(1000), 1100, 300);
        assert!(!changed);
        assert_eq!(peer.last_seen, 1000);
    }

    #[test]
    fn health_zero_timestamps() {
        let mut peer = sample_peer("node-2", PeerStatus::Active, 0);
        // With last_seen=0, current=0, timeout=300: 0-0=0 which is not > 300
        let changed = evaluate_peer_health(&mut peer, None, 0, 300);
        assert!(!changed);
        assert_eq!(peer.status, PeerStatus::Active);
    }

    #[test]
    fn health_handshake_updates_then_prevents_unreachable() {
        let mut peer = sample_peer("node-2", PeerStatus::Active, 1000);
        // Without handshake at time 1301, peer would go unreachable.
        // But a handshake at 1100 updates last_seen, keeping gap at 201 < 300.
        let changed = evaluate_peer_health(&mut peer, Some(1100), 1301, 300);
        assert!(changed); // last_seen updated
        assert_eq!(peer.last_seen, 1100);
        assert_eq!(peer.status, PeerStatus::Active);
    }

    // ── build_record edge cases ──

    #[test]
    fn build_record_no_region_no_zone() {
        let keypair = KeyPair::generate();
        let endpoint: SocketAddr = "10.0.0.1:51820".parse().unwrap();
        let ipv6 = Ipv6Addr::new(0xfd12, 0, 0, 0, 0, 0, 0, 1);

        let record = build_record("node-1", &keypair, endpoint, ipv6, None, None);

        assert!(record.region.is_none());
        assert!(record.zone.is_none());
    }

    // ── derive_prefix edge cases ──

    #[test]
    fn derive_prefix_always_ula_range() {
        // Every secret should produce an fd00::/8 prefix
        for i in 0..10u8 {
            let secret = MeshSecret::from_bytes([i; 32]);
            let prefix = derive_prefix_from_secret(&secret);
            let first_byte = (prefix.segments()[0] >> 8) as u8;
            assert_eq!(first_byte, 0xfd, "secret [{i};32] gave non-ULA prefix");
        }
    }

    #[test]
    fn derive_prefix_host_segments_always_zero() {
        // The prefix is /48, so segments 3..7 must always be zero
        for i in 0..5u8 {
            let secret = MeshSecret::from_bytes([i * 50; 32]);
            let prefix = derive_prefix_from_secret(&secret);
            let segs = prefix.segments();
            assert_eq!(segs[3], 0, "segment 3 non-zero for secret [{};32]", i * 50);
            assert_eq!(segs[4], 0, "segment 4 non-zero for secret [{};32]", i * 50);
            assert_eq!(segs[5], 0, "segment 5 non-zero for secret [{};32]", i * 50);
            assert_eq!(segs[6], 0, "segment 6 non-zero for secret [{};32]", i * 50);
            assert_eq!(segs[7], 0, "segment 7 non-zero for secret [{};32]", i * 50);
        }
    }
}
