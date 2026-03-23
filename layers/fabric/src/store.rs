use std::fs;
use std::net::Ipv6Addr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use syfrah_core::mesh::PeerRecord;
use syfrah_state::LayerDb;

const LAYER_NAME: &str = "fabric";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("state error: {0}")]
    State(#[from] syfrah_state::StateError),
    #[error("no state found at {0}")]
    NotFound(PathBuf),
}

/// Persisted state for a mesh node.
/// This struct is used for backward-compatible load/save operations.
/// Internally, data is stored in redb tables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeState {
    pub mesh_name: String,
    /// The mesh secret (syf_sk_...)
    pub mesh_secret: String,
    pub wg_private_key: String,
    pub wg_public_key: String,
    pub mesh_ipv6: Ipv6Addr,
    pub mesh_prefix: Ipv6Addr,
    pub wg_listen_port: u16,
    pub node_name: String,
    #[serde(default)]
    pub public_endpoint: Option<std::net::SocketAddr>,
    #[serde(default = "default_peering_port")]
    pub peering_port: u16,
    pub peers: Vec<PeerRecord>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub zone: Option<String>,
    #[serde(default)]
    pub metrics: Metrics,
}

fn default_peering_port() -> u16 {
    51821
}

/// Path to the Unix domain socket for CLI-daemon control.
pub fn control_socket_path() -> PathBuf {
    state_dir().join("control.sock")
}

/// Simple counters for observability.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub peers_discovered: u64,
    pub wg_reconciliations: u64,
    pub peers_marked_unreachable: u64,
    pub daemon_started_at: u64,
}

fn state_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".syfrah")
}

fn state_file() -> PathBuf {
    state_dir().join("state.json")
}

/// Check if a mesh state exists.
/// Checks both legacy JSON and new redb.
pub fn exists() -> bool {
    state_file().exists() || LayerDb::layer_exists(LAYER_NAME)
}

/// Save node state.
/// Writes to both redb (primary) and JSON (backward compat for E2E tests).
pub fn save(state: &NodeState) -> Result<(), StoreError> {
    let dir = state_dir();
    fs::create_dir_all(&dir)?;

    // Write to redb
    let db = open_db()?;
    db.batch(|w| {
        w.set("config", "mesh_name", &state.mesh_name)?;
        w.set("config", "mesh_secret", &state.mesh_secret)?;
        w.set("config", "wg_private_key", &state.wg_private_key)?;
        w.set("config", "wg_public_key", &state.wg_public_key)?;
        w.set("config", "mesh_ipv6", &state.mesh_ipv6)?;
        w.set("config", "mesh_prefix", &state.mesh_prefix)?;
        w.set("config", "wg_listen_port", &state.wg_listen_port)?;
        w.set("config", "node_name", &state.node_name)?;
        w.set("config", "public_endpoint", &state.public_endpoint)?;
        w.set("config", "peering_port", &state.peering_port)?;
        w.set("config", "region", &state.region)?;
        w.set("config", "zone", &state.zone)?;
        w.set_metric("peers_discovered", state.metrics.peers_discovered)?;
        w.set_metric("wg_reconciliations", state.metrics.wg_reconciliations)?;
        w.set_metric(
            "peers_marked_unreachable",
            state.metrics.peers_marked_unreachable,
        )?;
        w.set_metric("daemon_started_at", state.metrics.daemon_started_at)?;
        Ok(())
    })?;

    // Sync peers: clear existing and re-add all
    // (for full save compat — atomic peer ops should be preferred)
    let existing: Vec<(String, PeerRecord)> = db.list("peers")?;
    for (key, _) in &existing {
        db.delete("peers", key)?;
    }
    for peer in &state.peers {
        db.set("peers", &peer.wg_public_key, peer)?;
    }

    // Also write legacy JSON for backward compat with E2E tests
    // that inspect state.json directly
    let file = state_file();
    let tmp = dir.join("state.json.tmp");
    let json = serde_json::to_string_pretty(state)?;
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, &file)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

/// Load node state.
/// Tries redb first, falls back to legacy JSON.
pub fn load() -> Result<NodeState, StoreError> {
    // Try redb first
    if LayerDb::layer_exists(LAYER_NAME) {
        if let Ok(state) = load_from_redb() {
            return Ok(state);
        }
    }

    // Fallback to legacy JSON
    let file = state_file();
    if !file.exists() {
        return Err(StoreError::NotFound(file));
    }
    let json = fs::read_to_string(&file)?;
    let state: NodeState = serde_json::from_str(&json)?;
    Ok(state)
}

/// Load state from redb.
fn load_from_redb() -> Result<NodeState, StoreError> {
    let db = open_db()?;

    let mesh_name: String = db
        .get("config", "mesh_name")?
        .ok_or_else(|| StoreError::NotFound(syfrah_state::db_path(LAYER_NAME)))?;
    let mesh_secret: String = db
        .get("config", "mesh_secret")?
        .ok_or_else(|| StoreError::NotFound(syfrah_state::db_path(LAYER_NAME)))?;
    let wg_private_key: String = db
        .get("config", "wg_private_key")?
        .ok_or_else(|| StoreError::NotFound(syfrah_state::db_path(LAYER_NAME)))?;
    let wg_public_key: String = db
        .get("config", "wg_public_key")?
        .ok_or_else(|| StoreError::NotFound(syfrah_state::db_path(LAYER_NAME)))?;
    let mesh_ipv6: Ipv6Addr = db
        .get("config", "mesh_ipv6")?
        .ok_or_else(|| StoreError::NotFound(syfrah_state::db_path(LAYER_NAME)))?;
    let mesh_prefix: Ipv6Addr = db
        .get("config", "mesh_prefix")?
        .ok_or_else(|| StoreError::NotFound(syfrah_state::db_path(LAYER_NAME)))?;
    let wg_listen_port: u16 = db.get("config", "wg_listen_port")?.unwrap_or(51820);
    let node_name: String = db.get("config", "node_name")?.unwrap_or_default();
    let public_endpoint: Option<std::net::SocketAddr> =
        db.get("config", "public_endpoint")?.unwrap_or(None);
    let peering_port: u16 = db.get("config", "peering_port")?.unwrap_or(51821);
    let region: Option<String> = db.get("config", "region")?.unwrap_or(None);
    let zone: Option<String> = db.get("config", "zone")?.unwrap_or(None);

    let peer_entries: Vec<(String, PeerRecord)> = db.list("peers")?;
    let peers: Vec<PeerRecord> = peer_entries.into_iter().map(|(_, p)| p).collect();

    let metrics = Metrics {
        peers_discovered: db.get_metric("peers_discovered")?,
        wg_reconciliations: db.get_metric("wg_reconciliations")?,
        peers_marked_unreachable: db.get_metric("peers_marked_unreachable")?,
        daemon_started_at: db.get_metric("daemon_started_at")?,
    };

    Ok(NodeState {
        mesh_name,
        mesh_secret,
        wg_private_key,
        wg_public_key,
        mesh_ipv6,
        mesh_prefix,
        wg_listen_port,
        node_name,
        public_endpoint,
        peering_port,
        peers,
        region,
        zone,
        metrics,
    })
}

/// Generate a zone name based on region and existing peers.
/// Format: {region}-zone-{next_index}
pub fn generate_zone(region: &str, existing_peers: &[PeerRecord]) -> String {
    let prefix = format!("{region}-zone-");
    let max_index = existing_peers
        .iter()
        .filter_map(|p| {
            p.zone.as_ref().and_then(|z| {
                z.strip_prefix(&prefix)
                    .and_then(|suffix| suffix.parse::<u32>().ok())
            })
        })
        .max()
        .unwrap_or(0);
    format!("{prefix}{}", max_index + 1)
}

/// Delete all state (redb + JSON + entire directory).
pub fn clear() -> Result<(), StoreError> {
    let dir = state_dir();
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

// ── Atomic peer operations (new, fixes race condition) ──────

/// Add a peer atomically. If the peer already exists (by WG key), it's updated.
pub fn upsert_peer(peer: &PeerRecord) -> Result<(), StoreError> {
    let db = open_db()?;
    db.set("peers", &peer.wg_public_key, peer)?;

    // Also update legacy JSON
    if let Ok(mut state) = load() {
        if let Some(existing) = state
            .peers
            .iter_mut()
            .find(|p| p.wg_public_key == peer.wg_public_key)
        {
            *existing = peer.clone();
        } else {
            state.peers.push(peer.clone());
        }
        let _ = save_json_only(&state);
    }
    Ok(())
}

/// Get all peers from redb.
pub fn get_peers() -> Result<Vec<PeerRecord>, StoreError> {
    if !LayerDb::layer_exists(LAYER_NAME) {
        return Ok(vec![]);
    }
    let db = open_db()?;
    let entries: Vec<(String, PeerRecord)> = db.list("peers")?;
    Ok(entries.into_iter().map(|(_, p)| p).collect())
}

// ── Metrics (atomic) ────────────────────────────────────────

/// Increment a metric atomically.
pub fn inc_metric(name: &str, delta: u64) -> Result<u64, StoreError> {
    let db = open_db()?;
    Ok(db.inc_metric(name, delta)?)
}

/// Set a metric atomically.
pub fn set_metric(name: &str, value: u64) -> Result<(), StoreError> {
    let db = open_db()?;
    Ok(db.set_metric(name, value)?)
}

// ── Internal helpers ────────────────────────────────────────

fn open_db() -> Result<LayerDb, StoreError> {
    Ok(LayerDb::open(LAYER_NAME)?)
}

/// Write JSON only (no redb) for backward compat.
fn save_json_only(state: &NodeState) -> Result<(), StoreError> {
    let dir = state_dir();
    fs::create_dir_all(&dir)?;
    let file = state_file();
    let tmp = dir.join("state.json.tmp");
    let json = serde_json::to_string_pretty(state)?;
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, &file)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

// ── PID management (filesystem-based, not redb) ─────────────

fn pid_file() -> PathBuf {
    state_dir().join("daemon.pid")
}

/// Write the current process PID to the PID file.
pub fn write_pid() -> Result<(), StoreError> {
    let dir = state_dir();
    fs::create_dir_all(&dir)?;
    fs::write(pid_file(), std::process::id().to_string())?;
    Ok(())
}

/// Read the daemon PID from the PID file. Returns None if not found.
pub fn read_pid() -> Option<u32> {
    fs::read_to_string(pid_file())
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Remove the PID file.
pub fn remove_pid() {
    let _ = fs::remove_file(pid_file());
}

/// Check if a daemon is currently running (PID file exists and process alive).
pub fn daemon_running() -> Option<u32> {
    let pid = read_pid()?;
    #[cfg(unix)]
    {
        let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
        if alive {
            Some(pid)
        } else {
            None
        }
    }
    #[cfg(not(unix))]
    {
        Some(pid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv6Addr;

    #[test]
    fn save_and_load_roundtrip() {
        // Use a temp dir to avoid polluting ~/.syfrah
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("state.json");
        let state = NodeState {
            mesh_name: "test".into(),
            mesh_secret: "syf_sk_test".into(),
            wg_private_key: "priv".into(),
            wg_public_key: "pub".into(),
            mesh_ipv6: Ipv6Addr::new(0xfd12, 0, 0, 0, 0, 0, 0, 1),
            mesh_prefix: Ipv6Addr::new(0xfd12, 0, 0, 0, 0, 0, 0, 0),
            wg_listen_port: 51820,
            node_name: "node-1".into(),
            public_endpoint: None,
            peering_port: 51821,
            peers: vec![],
            region: Some("region-1".into()),
            zone: Some("region-1-zone-1".into()),
            metrics: Default::default(),
        };

        let json = serde_json::to_string_pretty(&state).unwrap();
        std::fs::write(&file, &json).unwrap();
        let loaded: NodeState =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(loaded.mesh_name, "test");
        assert_eq!(loaded.node_name, "node-1");
        assert_eq!(loaded.mesh_ipv6, state.mesh_ipv6);
    }
}
