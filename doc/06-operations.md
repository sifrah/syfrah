# Operations Guide

Complete reference for operating a Syfrah mesh network.

## Lifecycle

```
syfrah init     Create mesh, generate secret, start daemon
     |
     v
syfrah token    Share the token with other nodes
     |
     v
syfrah join     Other nodes join with the token
     |
     v
syfrah status   Monitor the mesh
syfrah peers    See all nodes and their state
     |
     v
syfrah stop     Stop the daemon (graceful)
syfrah start    Restart the daemon from saved state
     |
     v
syfrah rotate   Rotate the mesh secret (all peers must rejoin)
     |
     v
syfrah leave    Leave the mesh, announce departure, clear state
```

---

## Command Reference

### syfrah init --name \<mesh\>

Creates a new mesh network.

```
Options:
  --name <name>            Mesh name (required)
  --node-name <name>       Node name (default: hostname)
  --port <port>            WireGuard listen port (default: 51820)
  --endpoint <ip:port>     Public endpoint for WireGuard
  -d, --daemon             Run as background daemon
```

What it does:
1. Generates mesh secret (256 bits)
2. Generates iroh ed25519 keypair + WireGuard x25519 keypair
3. Derives ULA mesh prefix and node address
4. Creates WireGuard interface `syfrah0`
5. Publishes to DHT via iroh PKARR
6. Subscribes to gossip topic
7. Saves state to `~/.syfrah/state.json`
8. Displays the token and runs daemon in foreground

### syfrah join \<token\>

Joins an existing mesh.

```
Options:
  --node-name <name>       Node name (default: hostname)
  --port <port>            WireGuard listen port (default: 51820)
  --endpoint <ip:port>     Public endpoint for WireGuard
  -d, --daemon             Run as background daemon
```

What it does:
1. Parses token (secret + bootstrap NodeId)
2. Generates own keypairs
3. Resolves bootstrap node via DHT (30s timeout)
4. Joins gossip, receives peer records
5. Configures WireGuard with all discovered peers
6. Runs daemon in foreground

**Timeout**: If the bootstrap node is unreachable, join fails after 30 seconds with a clear error message.

### syfrah start

Restarts the daemon from saved state (`~/.syfrah/state.json`).

What it does:
1. Loads saved identity, keys, and peer list
2. Recreates WireGuard interface
3. Applies known peers immediately
4. Rejoins gossip with bootstrap peer from token
5. Runs daemon in foreground

Use after `syfrah stop` or a crash.

### syfrah stop

Stops the running daemon by sending SIGTERM.

Reads the PID from `~/.syfrah/daemon.pid`, sends signal, waits 3s for clean shutdown.

### syfrah status

Shows mesh info, daemon status, and live WireGuard stats.

```
Mesh:      production
Node:      node-1
Mesh IPv6: fd9a:bc12:7800::a1f3:1
Prefix:    fd9a:bc12:7800::/48
WG port:   51820
Token:     syf_sk_...
Daemon:    running (pid 12345)

Interface: syfrah0 (up)
Listen:    :51820
WG peers:  2 configured, 2 with handshake
Traffic:   rx 12.3 MiB / tx 8.7 MiB

Known peers: 2

Metrics:
  Uptime:          2h 15m
  Gossip received: 342
  WG reconciles:   28
  Peers unreached: 1
```

### syfrah peers

Detailed peer table with live WireGuard stats.

```
NAME               MESH IP                                  ENDPOINT               STATUS   HANDSHAKE    TRAFFIC
----------------------------------------------------------------------------------------------------------------
node-2             fd9a:bc12:7800::b2e4:2                   198.51.100.5:51820       active     12s ago  4K~ 2K~
node-3             fd9a:bc12:7800::c5d6:3                   192.0.2.10:51820        unreach      never        -
```

Columns: NAME, MESH IP, ENDPOINT, STATUS, HANDSHAKE (live WG), TRAFFIC (live WG)

### syfrah token

Displays the mesh token from saved state. Use to invite new nodes.

### syfrah rotate

Rotates the mesh secret. **Requires daemon to be stopped.**

1. Generates a new 256-bit secret
2. Creates a new token with the current node's iroh identity
3. Recalculates mesh prefix and node address
4. Clears peer list (all peers must rejoin)
5. Saves updated state

After rotation:
- Restart this node with `syfrah start`
- Share the new token with all peers
- Each peer must `syfrah leave` + `syfrah join <new-token>`

### syfrah leave

Leaves the mesh and clears all state.

1. Broadcasts a `Removed` PeerRecord via gossip (best-effort, 10s timeout)
2. Tears down WireGuard interface
3. Deletes `~/.syfrah/` directory

---

## Background Mode

All daemon commands (`init`, `join`, `start`) support the `--daemon` / `-d` flag to run in background.

```bash
# Start mesh in background
syfrah init --name production -d
# Output: "Starting daemon in background..."
#         "Daemon started. Use 'syfrah status' to check."

# Stop background daemon
syfrah stop

# Restart in background
syfrah start -d
```

When running in daemon mode:
- The process forks to background (double-fork on Unix)
- stdin/stdout/stderr redirected to /dev/null
- Logs written to `~/.syfrah/syfrah.log`
- Log file rotated at 10MB (`.log` renamed to `.log.old`)
- PID file written to `~/.syfrah/daemon.pid`

### Service Files

For production deployments, service files are provided in `contrib/`:

**systemd (Linux):**
```bash
sudo cp contrib/syfrah.service /etc/systemd/system/
sudo systemctl enable --now syfrah
```

**launchd (macOS):**
```bash
sudo cp contrib/com.syfrah.daemon.plist /Library/LaunchDaemons/
sudo launchctl load /Library/LaunchDaemons/com.syfrah.daemon.plist
```

---

## Daemon Behavior

### Concurrent tasks

The daemon runs 4 concurrent tasks via `tokio::select!`:

| Task | Interval | Description |
|------|----------|-------------|
| Gossip event loop | continuous | Receives + decrypts peer records, updates peer list, triggers WG reconciliation |
| Heartbeat | 60s | Re-broadcasts own PeerRecord with updated `last_seen` |
| State persistence | 30s | Saves peer list + metrics to `~/.syfrah/state.json` |
| Unreachable detection | 60s | Marks peers with `last_seen` > 5 min as `Unreachable` |

### Graceful shutdown

On Ctrl+C or SIGTERM:
1. Broadcasts departure record (`status: Removed`)
2. Waits 500ms for gossip propagation
3. Tears down WireGuard interface
4. Shuts down iroh endpoint
5. Removes PID file

### PID file

Written to `~/.syfrah/daemon.pid` on daemon start. Used by `syfrah stop` and `syfrah status` to detect a running daemon.

---

## Peer Lifecycle

```
                join/gossip
                    |
                    v
     +--------> Active <--------+
     |             |             |
     |       no heartbeat       heartbeat
     |        for 5 min         resumes
     |             |             |
     |             v             |
     |        Unreachable ------+
     |
     |     syfrah leave / Ctrl+C
     |             |
     |             v
     +---------- Removed  (propagated via gossip)
```

- **Active**: receiving heartbeats, WireGuard tunnel configured
- **Unreachable**: no heartbeat for 5 minutes, WireGuard peer kept (may reconnect)
- **Removed**: tombstone propagated via gossip, WireGuard peer removed

---

## Metrics

Persisted in `~/.syfrah/state.json` under the `metrics` key:

| Metric | Description |
|--------|-------------|
| `gossip_received` | Total gossip messages received |
| `gossip_sent` | Approximate messages sent |
| `wg_reconciliations` | Times WireGuard config was updated |
| `peers_marked_unreachable` | Times a peer was marked unreachable |
| `daemon_started_at` | Unix timestamp of daemon start |

Displayed by `syfrah status`.

---

## Security

| Threat | Mitigation |
|--------|-----------|
| Eavesdropping on DHT | Records encrypted with AES-256-GCM |
| Unauthorized mesh join | Requires the shared secret |
| NAT/firewall blocking | iroh relay servers (~100% traversal) |
| Key compromise | `syfrah rotate` + rejoin all peers |
| Stale PID file | `syfrah stop` checks process liveness |
| State file exposure | Permissions 0600, contains private keys |

---

## E2E Testing

A Docker-based E2E test setup is provided in `tests/e2e/`:

```bash
# From repo root
bash tests/e2e/run.sh
```

This:
1. Builds a Docker image with syfrah + WireGuard tools
2. Starts node1 with `syfrah init`
3. Extracts the token from node1 logs
4. Starts node2 and node3 with `syfrah join`
5. Checks peer lists and IPv6 connectivity via ping6
