# Daemon

The syfrah daemon is a long-running process that manages the WireGuard interface, peering listener, and state persistence.

## Lifecycle

### Init (`syfrah init --name <mesh>`)

1. Generate MeshSecret + WireGuard keypair
2. Derive mesh prefix and node IPv6 address
3. Create WireGuard interface `syfrah0`
4. Save state to `~/.syfrah/state.json`
5. Start daemon loop

### Join (`syfrah join <ip>`)

1. Generate WireGuard keypair
2. Send JoinRequest to existing node via TCP
3. Wait for approval (up to 5 minutes)
4. On acceptance: receive mesh secret, prefix, peer list
5. Derive own mesh IPv6 from prefix + WG pubkey
6. Create WireGuard interface, apply all received peers
7. Save state, start daemon loop

### Start (`syfrah start`)

1. Load state from `~/.syfrah/state.json`
2. Recreate WireGuard interface
3. Apply saved peers to WireGuard
4. Start daemon loop

### Leave (`syfrah leave`)

1. Tear down WireGuard interface
2. Remove control socket
3. Delete `~/.syfrah/` directory

## Daemon Loop

The daemon runs these concurrent tasks via `tokio::select!`:

| Task | Interval | Purpose |
|------|----------|---------|
| **Control channel** | always | Unix socket for CLI commands |
| **Peering listener** | always | TCP listener for join requests + announcements |
| **Persist** | 30s | Save metrics to state file |
| **Unreachable check** | 60s | Mark peers silent for 5+ min as unreachable |
| **Ctrl+C handler** | — | Graceful shutdown |

## State File

`~/.syfrah/state.json` (permissions 0600):

```json
{
  "mesh_name": "production",
  "mesh_secret": "syf_sk_...",
  "wg_private_key": "base64...",
  "wg_public_key": "base64...",
  "mesh_ipv6": "fd9a:...",
  "mesh_prefix": "fd9a:...::/48",
  "wg_listen_port": 51820,
  "node_name": "node-1",
  "peering_port": 51821,
  "peers": [...],
  "metrics": {
    "peers_discovered": 5,
    "wg_reconciliations": 5,
    "peers_marked_unreachable": 0,
    "daemon_started_at": 1711100000
  }
}
```

## PID File

`~/.syfrah/daemon.pid` — written on daemon start, removed on shutdown. Used by `syfrah status` to check if the daemon is running.

**Source:** `crates/syfrah-net/src/daemon.rs`, `crates/syfrah-net/src/store.rs`
