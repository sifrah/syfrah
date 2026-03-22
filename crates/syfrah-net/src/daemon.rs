use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::{info, warn};
use wireguard_control::{Key, KeyPair};

use syfrah_core::addressing;
use syfrah_core::mesh::{PeerRecord, PeerStatus};
use syfrah_core::secret::MeshSecret;

use crate::discovery::IpfsDiscovery;
use crate::store::{self, NodeState};
use crate::wg;

const UNREACHABLE_TIMEOUT: Duration = Duration::from_secs(300);
const PERSIST_INTERVAL: Duration = Duration::from_secs(30);

/// Configuration for starting a daemon.
pub struct DaemonConfig {
    pub mesh_name: String,
    pub node_name: String,
    pub wg_listen_port: u16,
    pub public_endpoint: Option<SocketAddr>,
    pub ipfs_api: Option<String>,
}

/// Run the init flow.
pub async fn run_init(config: DaemonConfig) -> anyhow::Result<()> {
    if store::exists() {
        anyhow::bail!("mesh state already exists. Run 'syfrah leave' first.");
    }

    let mesh_secret = MeshSecret::generate();
    let wg_keypair = wg::generate_keypair();

    let mesh_prefix = derive_prefix_from_secret(&mesh_secret);
    let mesh_ipv6 = addressing::derive_node_address(&mesh_prefix, wg_keypair.public.as_bytes());
    let endpoint = resolve_endpoint(&config);

    // Setup WireGuard
    wg::setup_interface(&wg_keypair, config.wg_listen_port, mesh_ipv6)?;
    info!("wireguard interface syfrah0 up");

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
        ipfs_api: config.ipfs_api.clone(),
        peers: vec![],
        metrics: Default::default(),
    };
    store::save(&state)?;

    println!("Mesh '{}' created.", config.mesh_name);
    println!("  Secret: {mesh_secret}");
    println!("  Node:   {} ({})", config.node_name, mesh_ipv6);
    println!();
    println!("Share the secret with other nodes to join.");
    println!("Running daemon... (Ctrl+C to stop)");

    let my_record = build_record(&config.node_name, &wg_keypair, endpoint, mesh_ipv6);
    let discovery = IpfsDiscovery::new(mesh_secret, config.ipfs_api);

    run_daemon(discovery, my_record, &wg_keypair).await
}

/// Run the join flow.
pub async fn run_join(secret_str: &str, config: DaemonConfig) -> anyhow::Result<()> {
    if store::exists() {
        anyhow::bail!("mesh state already exists. Run 'syfrah leave' first.");
    }

    let mesh_secret: MeshSecret = secret_str
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid secret: {e}"))?;

    let wg_keypair = wg::generate_keypair();
    let mesh_prefix = derive_prefix_from_secret(&mesh_secret);
    let mesh_ipv6 = addressing::derive_node_address(&mesh_prefix, wg_keypair.public.as_bytes());
    let endpoint = resolve_endpoint(&config);

    // Setup WireGuard
    wg::setup_interface(&wg_keypair, config.wg_listen_port, mesh_ipv6)?;
    info!("wireguard interface syfrah0 up");

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
        ipfs_api: config.ipfs_api.clone(),
        peers: vec![],
        metrics: Default::default(),
    };
    store::save(&state)?;

    println!("Joined mesh '{}'.", config.mesh_name);
    println!("  Node: {} ({})", config.node_name, mesh_ipv6);
    println!("Running daemon... (Ctrl+C to stop)");

    let my_record = build_record(&config.node_name, &wg_keypair, endpoint, mesh_ipv6);
    let discovery = IpfsDiscovery::new(mesh_secret, config.ipfs_api);

    run_daemon(discovery, my_record, &wg_keypair).await
}

/// Restart the daemon from saved state.
pub async fn run_start() -> anyhow::Result<()> {
    let state = store::load().map_err(|_| {
        anyhow::anyhow!("no mesh state found. Run 'syfrah init' or 'syfrah join' first.")
    })?;

    let mesh_secret: MeshSecret = state.mesh_secret.parse()
        .map_err(|e| anyhow::anyhow!("corrupt secret in state: {e}"))?;

    let wg_private = Key::from_base64(&state.wg_private_key)
        .map_err(|_| anyhow::anyhow!("corrupt WG private key in state"))?;
    let wg_keypair = KeyPair::from_private(wg_private);

    // Setup WireGuard
    wg::setup_interface(&wg_keypair, state.wg_listen_port, state.mesh_ipv6)?;

    // Apply known peers immediately
    if !state.peers.is_empty() {
        info!("applying {} known peers from state", state.peers.len());
        if let Err(e) = wg::apply_peers(&wg_keypair.public, &state.peers) {
            warn!("failed to apply saved peers: {e}");
        }
    }

    println!("Restarting daemon for mesh '{}'...", state.mesh_name);
    println!("  Node: {} ({})", state.node_name, state.mesh_ipv6);
    println!("Running daemon... (Ctrl+C to stop)");

    let endpoint_addr = state.public_endpoint.unwrap_or_else(|| {
        SocketAddr::new("0.0.0.0".parse().unwrap(), state.wg_listen_port)
    });
    let my_record = build_record(&state.node_name, &wg_keypair, endpoint_addr, state.mesh_ipv6);

    let discovery = IpfsDiscovery::new(mesh_secret, state.ipfs_api.clone());

    // Seed discovery with known peers
    {
        let peers_ref = discovery.peers();
        let mut peers = peers_ref.write().await;
        *peers = state.peers.clone();
    }

    run_daemon(discovery, my_record, &wg_keypair).await
}

/// Broadcast departure before leaving.
pub async fn run_leave() -> anyhow::Result<()> {
    if !store::exists() {
        println!("No mesh configured.");
        return Ok(());
    }

    // Tear down WireGuard
    if let Err(e) = wg::teardown_interface() {
        eprintln!("Warning: could not tear down WireGuard interface: {e}");
    }

    store::clear()?;
    println!("Left the mesh. State cleared.");
    Ok(())
}

/// The main daemon loop.
async fn run_daemon(
    discovery: IpfsDiscovery,
    my_record: PeerRecord,
    wg_keypair: &KeyPair,
) -> anyhow::Result<()> {
    store::write_pid()?;

    let wg_pubkey = wg_keypair.public.clone();
    let peers_ref = discovery.peers();

    // Metrics
    let metrics_received = Arc::new(AtomicU64::new(0));
    let metrics_reconciliations = Arc::new(AtomicU64::new(0));
    let metrics_unreachable = Arc::new(AtomicU64::new(0));
    let daemon_started = now();

    // Callback: when a peer is discovered/updated, upsert WG
    let wg_pubkey_cb = wg_pubkey.clone();
    let recv_counter = metrics_received.clone();
    let recon_counter = metrics_reconciliations.clone();
    let on_change: Arc<dyn Fn(&PeerRecord) + Send + Sync> = Arc::new(move |record| {
        recv_counter.fetch_add(1, Ordering::Relaxed);
        let pubkey = wg_pubkey_cb.clone();
        let record = record.clone();
        let recon = recon_counter.clone();
        tokio::spawn(async move {
            if let Err(e) = wg::upsert_peer(&pubkey, &record) {
                warn!("failed to upsert wireguard peer {}: {e}", record.name);
            } else {
                recon.fetch_add(1, Ordering::Relaxed);
                info!("wireguard peer upserted: {}", record.name);
            }
        });
    });

    // Persist peers + metrics
    let persist_peers = peers_ref.clone();
    let persist_recv = metrics_received.clone();
    let persist_recon = metrics_reconciliations.clone();
    let persist_unreach = metrics_unreachable.clone();
    let persist = async {
        let mut interval = tokio::time::interval(PERSIST_INTERVAL);
        loop {
            interval.tick().await;
            let peer_list = persist_peers.read().await;
            if let Ok(mut state) = store::load() {
                state.peers = peer_list.clone();
                state.metrics.peers_discovered = persist_recv.load(Ordering::Relaxed);
                state.metrics.wg_reconciliations = persist_recon.load(Ordering::Relaxed);
                state.metrics.peers_marked_unreachable = persist_unreach.load(Ordering::Relaxed);
                state.metrics.daemon_started_at = daemon_started;
                if let Err(e) = store::save(&state) {
                    warn!("failed to persist state: {e}");
                }
            }
        }
    };

    // Unreachable detection
    let unreachable_peers = peers_ref.clone();
    let unreachable_wg_pubkey = wg_pubkey.clone();
    let unreachable_counter = metrics_unreachable.clone();
    let unreachable_check = async {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let current = now();
            let mut peer_list = unreachable_peers.write().await;
            let mut stale = vec![];
            for peer in peer_list.iter_mut() {
                if peer.status == PeerStatus::Active
                    && current.saturating_sub(peer.last_seen) > UNREACHABLE_TIMEOUT.as_secs()
                {
                    info!("marking peer {} as unreachable", peer.name);
                    peer.status = PeerStatus::Unreachable;
                    unreachable_counter.fetch_add(1, Ordering::Relaxed);
                    stale.push(peer.clone());
                }
            }
            drop(peer_list);
            for peer in &stale {
                if let Err(e) = wg::upsert_peer(&unreachable_wg_pubkey, peer) {
                    warn!("failed to remove unreachable peer {}: {e}", peer.name);
                }
            }
        }
    };

    // Run discovery + persist + unreachable + ctrl-c
    tokio::select! {
        result = discovery.run(my_record.clone(), on_change) => {
            if let Err(e) = result {
                warn!("discovery loop ended: {e}");
            }
        }
        _ = persist => {}
        _ = unreachable_check => {}
        _ = tokio::signal::ctrl_c() => {
            println!("\nShutting down...");
        }
    }

    wg::teardown_interface()?;
    store::remove_pid();
    info!("daemon stopped");

    Ok(())
}

fn build_record(
    name: &str,
    wg_keypair: &KeyPair,
    endpoint: SocketAddr,
    mesh_ipv6: std::net::Ipv6Addr,
) -> PeerRecord {
    PeerRecord {
        name: name.to_string(),
        wg_public_key: wg_keypair.public.to_base64(),
        endpoint,
        mesh_ipv6,
        last_seen: now(),
        status: PeerStatus::Active,
        iroh_node_id: None,
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
        0, 0, 0, 0, 0,
    )
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
