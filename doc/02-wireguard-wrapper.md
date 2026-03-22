# WireGuard Wrapper

This document describes the WireGuard interface management layer in `syfrah-net/src/wg.rs`. This is the data plane of the mesh — it creates and manages the encrypted tunnels between nodes.

## Overview

The wrapper uses the `wireguard-control` crate (from the innernet project) to manage the `syfrah0` WireGuard interface. It handles:

- Interface lifecycle (create, destroy)
- Key management
- Peer reconciliation
- IPv6 address assignment
- Interface status queries

```
syfrah-net/src/wg.rs
  |
  |-- wireguard-control crate  (WireGuard API)
  |-- system commands           (ip / ifconfig for IPv6)
  |
  +-- manages interface "syfrah0"
```

---

## Architecture

### Two layers

The mesh uses two distinct layers:

```
+----------------------------------------------------------+
|  Control Plane (iroh)                                    |
|  - Peer discovery via DHT                                |
|  - State sync via gossip                                 |
|  - Encrypted with mesh secret (AES-256-GCM)              |
+----------------------------------------------------------+
        |  PeerRecords (wg pubkey, endpoint, mesh IPv6)
        v
+----------------------------------------------------------+
|  Data Plane (WireGuard)                         wg.rs    |
|  - Interface: syfrah0                                    |
|  - x25519 encryption per-tunnel                          |
|  - ULA IPv6 addresses                                    |
|  - NAT traversal via persistent keepalive (25s)          |
+----------------------------------------------------------+
        |
        v
   Kernel networking (encrypted UDP tunnels between nodes)
```

The control plane tells the data plane **who** the peers are. The data plane creates the encrypted tunnels.

---

## Interface lifecycle

### Setup sequence

```
setup_interface(keypair, listen_port, mesh_ipv6)
  |
  1. create_interface(private_key, listen_port)
  |    --> DeviceUpdate::new()
  |          .set_private_key(key)
  |          .set_listen_port(port)
  |          .apply("syfrah0")
  |
  2. assign_ipv6(mesh_ipv6)
  |    Linux:  ip -6 addr add fd9a:bc12::1/128 dev syfrah0
  |    macOS:  ifconfig syfrah0 inet6 fd9a:bc12::1/128
  |
  3. bring_interface_up()
       Linux:  ip link set syfrah0 up
       macOS:  (automatic)
```

### Teardown

```
teardown_interface()
  |
  destroy_interface()
    --> Device::get("syfrah0") --> device.delete()
```

---

## Peer reconciliation

The core operation. Called every time the mesh state changes (new peer, peer removed, peer updated).

### Strategy: full replacement

```
apply_peers(self_pubkey, peer_records)
  |
  DeviceUpdate::new()
    .replace_peers()              <-- removes all existing peers
    .add_peer(peer1_config)       <-- re-adds each active peer
    .add_peer(peer2_config)
    ...
    .apply("syfrah0")
```

**Why full replacement instead of diff?**
- Simpler, no stale state bugs
- At our scale (tens of peers) the overhead is negligible (<1ms)
- Guarantees the interface exactly matches the mesh state

### Per-peer configuration

For each `PeerRecord`:

```
PeerConfigBuilder::new(wg_public_key)
  .set_endpoint(endpoint)                        // public IP:port
  .replace_allowed_ips()
  .add_allowed_ip(mesh_ipv6, 128)                // only the peer's ULA /128
  .set_persistent_keepalive_interval(25)          // NAT traversal
```

### Filtering rules

- **Skip self**: peers whose public key matches the local node are not added
- **Skip removed**: peers with `PeerStatus::Removed` are not added

```
peers = [node-1 (self), node-2 (active), node-3 (removed), node-4 (active)]
                  |              |                |                 |
               SKIP           ADD             SKIP               ADD

Result: WireGuard configured with node-2 and node-4 only.
```

---

## NAT traversal

Dedicated servers across providers (OVH, Hetzner, Scaleway) are behind different network configurations. WireGuard handles NAT traversal with:

```
persistent_keepalive_interval = 25 seconds
```

This ensures:
- UDP hole punching stays active through NAT/firewalls
- The peer's endpoint stays reachable even through CG-NAT
- The tunnel recovers quickly after network changes

---

## Interface summary

`interface_summary()` queries the live WireGuard interface and returns:

```
InterfaceSummary
  +-- name: "syfrah0"
  +-- public_key: "base64..."
  +-- listen_port: 51820
  +-- peer_count: 3
  +-- peers: [
        PeerSummary {
          public_key: "base64..."
          endpoint: 203.0.113.1:51820
          allowed_ips: ["fd9a:bc12::2/128"]
          last_handshake: Some(2024-01-15T10:30:00Z)
          rx_bytes: 1048576
          tx_bytes: 524288
        },
        ...
      ]
```

Used by `syfrah status` and `syfrah peers` commands.

---

## Backend selection

```
Linux   --> Backend::Kernel     (netlink, most efficient)
macOS   --> Backend::Userspace  (wireguard-go via unix socket)
OpenBSD --> Backend::OpenBSD
```

`Backend::default()` auto-selects the right backend for the platform.

---

## Testing

Unit tests (run without root):
- `keypair_generation` — keypair is valid, public != private
- `keypair_base64_roundtrip` — base64 encoding/decoding is lossless
- `iface_name_valid` — "syfrah0" is a valid interface name

Integration tests (require root, `#[ignore]`):
- `create_and_destroy_interface` — full lifecycle
- `apply_peers_integration` — add peers and verify via interface query

Run integration tests with:
```bash
sudo cargo test -- --ignored
```

---

## Error handling

All operations return `Result<_, WgError>`:

| Variant | Cause |
|---------|-------|
| `Io` | wireguard-control syscall failure |
| `InvalidName` | bad interface name |
| `InvalidKey` | malformed base64 WG public key in a PeerRecord |
| `AddressAssign` | `ip` or `ifconfig` command failed |
| `NotFound` | interface does not exist |
