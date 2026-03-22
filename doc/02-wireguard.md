# WireGuard Management

## Interface

Syfrah creates a single WireGuard interface `syfrah0` on each node.

| Operation | Linux command |
|-----------|--------------|
| Create + set key | `wireguard-control` DeviceUpdate |
| Assign IPv6 | `ip -6 addr add {addr}/128 dev syfrah0` |
| Bring up | `ip link set syfrah0 up` |
| Add route | `ip -6 route replace {peer_ipv6}/128 dev syfrah0` |
| Destroy | `wireguard-control` Device::delete |

## Peer Configuration

Each WireGuard peer is configured with:
- **Public key**: from PeerRecord.wg_public_key
- **Endpoint**: from PeerRecord.endpoint (IP:port)
- **Allowed IPs**: `{peer.mesh_ipv6}/128` (single address)
- **Persistent keepalive**: 25 seconds

## Reconciliation

Two modes:
- **`apply_peers`**: Full replacement — removes all existing peers and adds the new set. Used on daemon start/restart.
- **`upsert_peer`**: Incremental — adds or updates a single peer. Used when a new peer joins or an announcement arrives.

When a peer's status is `Removed`, `upsert_peer` removes it from WireGuard and deletes its route.

**Source:** `crates/syfrah-net/src/wg.rs`
