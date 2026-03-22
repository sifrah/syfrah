use std::fs;
use std::net::Ipv6Addr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use syfrah_core::mesh::PeerRecord;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no state found at {0}")]
    NotFound(PathBuf),
}

/// Persisted state for a mesh node. Stored in ~/.syfrah/
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
    pub metrics: Metrics,
}

fn default_peering_port() -> u16 {
    51821
}

/// Path to the Unix domain socket for CLI-daemon control.
pub fn control_socket_path() -> std::path::PathBuf {
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
pub fn exists() -> bool {
    state_file().exists()
}

/// Save node state atomically (write tmp + rename).
pub fn save(state: &NodeState) -> Result<(), StoreError> {
    let dir = state_dir();
    fs::create_dir_all(&dir)?;

    let file = state_file();
    let tmp = dir.join("state.json.tmp");
    let json = serde_json::to_string_pretty(state)?;
    fs::write(&tmp, &json)?;
    fs::rename(&tmp, &file)?;

    // Restrict permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

/// Load node state from disk.
pub fn load() -> Result<NodeState, StoreError> {
    let file = state_file();
    if !file.exists() {
        return Err(StoreError::NotFound(file));
    }
    let json = fs::read_to_string(&file)?;
    let state: NodeState = serde_json::from_str(&json)?;
    Ok(state)
}

/// Delete the state directory.
pub fn clear() -> Result<(), StoreError> {
    let dir = state_dir();
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

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
    // Check if process is alive (signal 0)
    #[cfg(unix)]
    {
        let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
        if alive { Some(pid) } else { None }
    }
    #[cfg(not(unix))]
    {
        Some(pid) // can't check on non-unix, assume alive
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
            metrics: Default::default(),
        };

        let json = serde_json::to_string_pretty(&state).unwrap();
        std::fs::write(&file, &json).unwrap();
        let loaded: NodeState = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(loaded.mesh_name, "test");
        assert_eq!(loaded.node_name, "node-1");
        assert_eq!(loaded.mesh_ipv6, state.mesh_ipv6);
    }
}
