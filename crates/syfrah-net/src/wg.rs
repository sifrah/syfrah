use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::str::FromStr;

use thiserror::Error;
use wireguard_control::{
    Backend, Device, DeviceUpdate, InterfaceName, Key, KeyPair, PeerConfigBuilder,
};

use syfrah_core::mesh::PeerRecord;

pub const INTERFACE_NAME: &str = "syfrah0";
const KEEPALIVE_INTERVAL: u16 = 25;

#[derive(Debug, Error)]
pub enum WgError {
    #[error("wireguard interface error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid interface name: {0}")]
    InvalidName(String),
    #[error("invalid key: {0}")]
    InvalidKey(String),
    #[error("failed to assign IPv6 address: {0}")]
    AddressAssign(String),
    #[error("interface {0} not found")]
    NotFound(String),
}

fn backend() -> Backend {
    Backend::default()
}

fn iface_name() -> Result<InterfaceName, WgError> {
    InterfaceName::from_str(INTERFACE_NAME)
        .map_err(|e| WgError::InvalidName(e.to_string()))
}

/// Generate a new WireGuard keypair.
pub fn generate_keypair() -> KeyPair {
    KeyPair::generate()
}

/// Create the WireGuard interface and set the private key + listen port.
/// If the interface already exists, it will be reconfigured.
pub fn create_interface(private_key: &Key, listen_port: u16) -> Result<(), WgError> {
    let iface = iface_name()?;
    DeviceUpdate::new()
        .set_private_key(private_key.clone())
        .set_listen_port(listen_port)
        .apply(&iface, backend())?;
    Ok(())
}

/// Destroy the WireGuard interface if it exists.
pub fn destroy_interface() -> Result<(), WgError> {
    let iface = iface_name()?;
    match Device::get(&iface, backend()) {
        Ok(device) => {
            device.delete()?;
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(WgError::Io(e)),
    }
}

/// Read the current state of the WireGuard interface.
pub fn get_device() -> Result<Device, WgError> {
    let iface = iface_name()?;
    Device::get(&iface, backend()).map_err(WgError::Io)
}

/// Full reconciliation: replace all peers on the interface with the given peer records.
/// Skips peers whose WG public key matches `self_pubkey` (the local node).
pub fn apply_peers(
    self_pubkey: &Key,
    peers: &[PeerRecord],
) -> Result<(), WgError> {
    let iface = iface_name()?;

    let mut update = DeviceUpdate::new().replace_peers();

    for peer in peers {
        let peer_key = Key::from_base64(&peer.wg_public_key)
            .map_err(|_| WgError::InvalidKey(peer.wg_public_key.clone()))?;

        // Don't add ourselves as a peer
        if peer_key == *self_pubkey {
            continue;
        }

        // Skip removed peers
        if peer.status == syfrah_core::mesh::PeerStatus::Removed {
            continue;
        }

        let peer_config = PeerConfigBuilder::new(&peer_key)
            .set_endpoint(peer.endpoint)
            .replace_allowed_ips()
            .add_allowed_ip(IpAddr::V6(peer.mesh_ipv6), 128)
            .set_persistent_keepalive_interval(KEEPALIVE_INTERVAL);

        update = update.add_peer(peer_config);
    }

    update.apply(&iface, backend())?;

    // Add routes for each peer's mesh IPv6 via the WireGuard interface
    for peer in peers {
        if peer.status == syfrah_core::mesh::PeerStatus::Removed {
            continue;
        }
        let peer_key = Key::from_base64(&peer.wg_public_key)
            .map_err(|_| WgError::InvalidKey(peer.wg_public_key.clone()))?;
        if peer_key == *self_pubkey {
            continue;
        }
        add_route_v6(peer.mesh_ipv6)?;
    }

    Ok(())
}

/// Incrementally add or update a single peer. Does NOT replace all peers.
/// Use this for gossip events (one peer changed at a time).
pub fn upsert_peer(
    self_pubkey: &Key,
    peer: &PeerRecord,
) -> Result<(), WgError> {
    let peer_key = Key::from_base64(&peer.wg_public_key)
        .map_err(|_| WgError::InvalidKey(peer.wg_public_key.clone()))?;

    if peer_key == *self_pubkey {
        return Ok(());
    }

    let iface = iface_name()?;

    if peer.status == syfrah_core::mesh::PeerStatus::Removed {
        // Remove this peer
        DeviceUpdate::new()
            .remove_peer_by_key(&peer_key)
            .apply(&iface, backend())?;
        remove_route_v6(peer.mesh_ipv6)?;
    } else {
        // Add or update this peer (layered on top, no replace_peers)
        let peer_config = PeerConfigBuilder::new(&peer_key)
            .set_endpoint(peer.endpoint)
            .replace_allowed_ips()
            .add_allowed_ip(IpAddr::V6(peer.mesh_ipv6), 128)
            .set_persistent_keepalive_interval(KEEPALIVE_INTERVAL);

        DeviceUpdate::new()
            .add_peer(peer_config)
            .apply(&iface, backend())?;
        add_route_v6(peer.mesh_ipv6)?;
    }

    Ok(())
}

/// Remove an IPv6 route for a peer's mesh address.
fn remove_route_v6(addr: std::net::Ipv6Addr) -> Result<(), WgError> {
    let cidr = format!("{addr}/128");

    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("ip")
            .args(["-6", "route", "del", &cidr, "dev", INTERFACE_NAME])
            .output();
    }

    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("route")
            .args(["-n", "delete", "-inet6", &cidr])
            .output();
    }

    Ok(())
}

/// Assign an IPv6 address to the WireGuard interface.
/// This calls system commands (ip on Linux, ifconfig on macOS).
pub fn assign_ipv6(addr: Ipv6Addr) -> Result<(), WgError> {
    let cidr = format!("{addr}/128");

    #[cfg(target_os = "linux")]
    {
        let output = std::process::Command::new("ip")
            .args(["-6", "addr", "add", &cidr, "dev", INTERFACE_NAME])
            .output()
            .map_err(|e| WgError::AddressAssign(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // "RTNETLINK answers: File exists" means the address is already assigned
            if !stderr.contains("File exists") {
                return Err(WgError::AddressAssign(stderr.into_owned()));
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("ifconfig")
            .args([INTERFACE_NAME, "inet6", &cidr])
            .output()
            .map_err(|e| WgError::AddressAssign(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(WgError::AddressAssign(stderr.into_owned()));
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        return Err(WgError::AddressAssign(
            "unsupported platform for IPv6 address assignment".into(),
        ));
    }

    Ok(())
}

/// Add an IPv6 route for a peer's mesh address via the WireGuard interface.
fn add_route_v6(addr: std::net::Ipv6Addr) -> Result<(), WgError> {
    let cidr = format!("{addr}/128");

    #[cfg(target_os = "linux")]
    {
        let output = std::process::Command::new("ip")
            .args(["-6", "route", "replace", &cidr, "dev", INTERFACE_NAME])
            .output()
            .map_err(|e| WgError::AddressAssign(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("failed to add route for {cidr}: {stderr}");
        }
    }

    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("route")
            .args(["-n", "add", "-inet6", &cidr, "-interface", INTERFACE_NAME])
            .output()
            .map_err(|e| WgError::AddressAssign(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Ignore "File exists" (route already present)
            if !stderr.contains("File exists") {
                tracing::warn!("failed to add route for {cidr}: {stderr}");
            }
        }
    }

    Ok(())
}

/// Bring the interface up (Linux only; macOS interfaces are up after ifconfig).
pub fn bring_interface_up() -> Result<(), WgError> {
    #[cfg(target_os = "linux")]
    {
        let output = std::process::Command::new("ip")
            .args(["link", "set", INTERFACE_NAME, "up"])
            .output()
            .map_err(|e| WgError::AddressAssign(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(WgError::AddressAssign(stderr.into_owned()));
        }
    }

    Ok(())
}

/// Convenience: full setup sequence for a node.
/// Creates the interface, sets the key, assigns the IPv6 address, brings it up.
pub fn setup_interface(
    keypair: &KeyPair,
    listen_port: u16,
    mesh_ipv6: Ipv6Addr,
) -> Result<(), WgError> {
    create_interface(&keypair.private, listen_port)?;
    assign_ipv6(mesh_ipv6)?;
    bring_interface_up()?;
    Ok(())
}

/// Convenience: full teardown.
pub fn teardown_interface() -> Result<(), WgError> {
    destroy_interface()
}

/// Get a summary of the current interface state for display.
pub fn interface_summary() -> Result<InterfaceSummary, WgError> {
    let device = get_device()?;
    Ok(InterfaceSummary {
        name: INTERFACE_NAME.to_string(),
        public_key: device.public_key.map(|k| k.to_base64()),
        listen_port: device.listen_port,
        peer_count: device.peers.len(),
        peers: device
            .peers
            .iter()
            .map(|p| PeerSummary {
                public_key: p.config.public_key.to_base64(),
                endpoint: p.config.endpoint,
                allowed_ips: p
                    .config
                    .allowed_ips
                    .iter()
                    .map(|ip| format!("{}/{}", ip.address, ip.cidr))
                    .collect(),
                last_handshake: p.stats.last_handshake_time,
                rx_bytes: p.stats.rx_bytes,
                tx_bytes: p.stats.tx_bytes,
            })
            .collect(),
    })
}

#[derive(Debug, Clone)]
pub struct InterfaceSummary {
    pub name: String,
    pub public_key: Option<String>,
    pub listen_port: Option<u16>,
    pub peer_count: usize,
    pub peers: Vec<PeerSummary>,
}

#[derive(Debug, Clone)]
pub struct PeerSummary {
    pub public_key: String,
    pub endpoint: Option<SocketAddr>,
    pub allowed_ips: Vec<String>,
    pub last_handshake: Option<std::time::SystemTime>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keypair_generation() {
        let kp = generate_keypair();
        assert_ne!(kp.private, kp.public);
        // Public key derivation is deterministic
        assert_eq!(kp.private.get_public(), kp.public);
    }

    #[test]
    fn keypair_base64_roundtrip() {
        let kp = generate_keypair();
        let b64 = kp.public.to_base64();
        let parsed = Key::from_base64(&b64).unwrap();
        assert_eq!(parsed, kp.public);
    }

    #[test]
    fn iface_name_valid() {
        let name = iface_name().unwrap();
        assert_eq!(name.as_str_lossy(), INTERFACE_NAME);
    }

    // Integration tests (require root) are skipped in normal CI.
    // Run with: sudo cargo test -- --ignored
    #[test]
    #[ignore]
    fn create_and_destroy_interface() {
        let kp = generate_keypair();
        create_interface(&kp.private, 51820).unwrap();

        let device = get_device().unwrap();
        assert_eq!(device.public_key, Some(kp.public.clone()));
        assert_eq!(device.listen_port, Some(51820));

        destroy_interface().unwrap();
        assert!(get_device().is_err());
    }

    #[test]
    #[ignore]
    fn apply_peers_integration() {
        let kp = generate_keypair();
        create_interface(&kp.private, 51820).unwrap();

        let peer_kp = generate_keypair();
        let peer = PeerRecord {
            name: "test-peer".into(),
            wg_public_key: peer_kp.public.to_base64(),
            endpoint: "203.0.113.1:51820".parse().unwrap(),
            mesh_ipv6: "fd12:3456:7800::1".parse().unwrap(),
            last_seen: 0,
            status: syfrah_core::mesh::PeerStatus::Active,
            iroh_node_id: None,
        };

        apply_peers(&kp.public, &[peer]).unwrap();

        let device = get_device().unwrap();
        assert_eq!(device.peers.len(), 1);
        assert_eq!(device.peers[0].config.public_key, peer_kp.public);

        destroy_interface().unwrap();
    }
}
