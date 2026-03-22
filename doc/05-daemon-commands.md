# Daemon & Commands

This document describes the syfrah daemon behavior and the CLI commands for operating the mesh.

## Daemon Architecture

The daemon runs in the foreground after `syfrah init` or `syfrah join`. It manages three concurrent tasks:

```
tokio::select!
  |
  +-- Gossip event loop
  |     - Receives encrypted PeerRecords
  |     - Decrypts with mesh encryption key
  |     - Upserts peer in internal list
  |     - Triggers WireGuard reconciliation
  |
  +-- Heartbeat (every 60s)
  |     - Re-broadcasts own PeerRecord
  |     - Updates last_seen timestamp
  |     - Keeps gossip neighbors aware we're alive
  |
  +-- State persistence (every 30s)
  |     - Writes current peer list to ~/.syfrah/state.json
  |     - Atomic write (tmp + rename)
  |
  +-- Ctrl+C handler
        - Tears down syfrah0 WireGuard interface
        - Shuts down iroh endpoint + router
```

### WireGuard Reconciliation

On each `PeerDiscovered` gossip event:

```
New PeerRecord
  |
  +-- Read current peer list
  +-- wg::apply_peers(self_pubkey, peers)
        |
        DeviceUpdate::new()
          .replace_peers()         // full replacement
          .add_peer(peer1)         // pubkey + endpoint + allowed_ip + keepalive
          .add_peer(peer2)
          ...
          .apply("syfrah0")
```

Full replacement ensures the WireGuard config always matches the gossip state exactly.

---

## Commands

### syfrah status

Shows mesh info from persisted state + live WireGuard interface stats.

```
$ syfrah status
Mesh:      production
Node:      node-1
Mesh IPv6: fd9a:bc12:7800::a1f3:1
Prefix:    fd9a:bc12:7800::/48
WG port:   51820
Token:     syf_sk_5HueCGU8rMjxEXxiPuD5BDku...

Interface: syfrah0 (up)
Listen:    :51820
WG peers:  2 configured, 2 with handshake
Traffic:   rx 12.3 MiB / tx 8.7 MiB

Known peers: 2
```

**Data sources:**
- Mesh info: `~/.syfrah/state.json`
- Interface status, peer count, traffic: live query via `wg::interface_summary()`
- If the interface is down, shows "(down)" and skips live stats

### syfrah peers

Shows a detailed table combining persisted peer info with live WireGuard stats.

```
$ syfrah peers
NAME               MESH IP                                  ENDPOINT               STATUS   HANDSHAKE    TRAFFIC
----------------------------------------------------------------------------------------------------------------
node-2             fd9a:bc12:7800::b2e4:2                   198.51.100.5:51820       active     12s ago  4K↓ 2K↑
node-3             fd9a:bc12:7800::c5d6:3                   192.0.2.10:51820         active      3m ago  1M↓ 800K↑
node-4             fd9a:bc12:7800::d7e8:4                   203.0.113.50:51820      unreach      never        -
```

**Columns:**
| Column | Source | Description |
|--------|--------|-------------|
| NAME | state.json | Node name from PeerRecord |
| MESH IP | state.json | ULA IPv6 address |
| ENDPOINT | state.json | Public WireGuard endpoint |
| STATUS | state.json | active / unreach / removed |
| HANDSHAKE | live WG | Time since last WireGuard handshake |
| TRAFFIC | live WG | rx↓ tx↑ since last interface reset |

**Handshake column** indicates real connectivity:
- `12s ago` — tunnel is active, packets flowing
- `never` — WireGuard peer configured but no handshake yet
- `n/a` — peer not found in live WireGuard config

### syfrah leave

```
$ syfrah leave
Left the mesh. State cleared.
```

1. Tears down `syfrah0` WireGuard interface (best-effort)
2. Deletes `~/.syfrah/` directory
3. Does not broadcast departure (improvement for later)

---

## State File

`~/.syfrah/state.json` — persisted every 30s by the daemon.

```
~/.syfrah/
  state.json        Node identity + mesh config + peer list
```

Permissions: `0600` (contains WG private key and iroh secret key).

Written atomically: write to `state.json.tmp` then `rename()` to `state.json`.

Deleted entirely by `syfrah leave`.
