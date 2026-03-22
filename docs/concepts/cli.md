# CLI

## Overview

`syfrah` is the single binary that operators use to manage the platform. It handles both the fabric layer (today) and will handle all cloud operations (future).

The CLI communicates with the local daemon via a Unix domain socket (`~/.syfrah/control.sock`) for operations that require a running daemon, or directly reads/writes state (`~/.syfrah/state.json`) for offline operations.

```
    Operator
       │
       ▼
    syfrah CLI ──► Unix socket ──► Daemon (local)
       │
       └──► state.json (direct read/write for offline ops)
```

## Current commands (fabric layer)

These commands are implemented today and manage the WireGuard mesh.

### `syfrah init`

Create a new mesh and start the daemon.

```bash
syfrah init --name my-cloud
syfrah init --name my-cloud --node-name par-hv-1 --port 51820 --endpoint 51.210.x.x:51820
syfrah init --name my-cloud -d   # start daemon in background
```

| Flag | Default | Description |
|---|---|---|
| `--name` | required | Mesh name |
| `--node-name` | hostname | This node's name in the mesh |
| `--port` | 51820 | WireGuard listen port |
| `--endpoint` | auto-detect | Public IP:port for this node |
| `--peering-port` | port + 1 | TCP port for peering protocol |
| `-d, --daemon` | foreground | Run daemon in background |

What it does:
1. Generates a mesh secret (`syf_sk_...`)
2. Generates a WireGuard keypair
3. Derives mesh IPv6 prefix (`/48`) and node address (`/128`)
4. Creates the `syfrah0` WireGuard interface
5. Saves state to `~/.syfrah/state.json`
6. Starts the daemon (peering listener, health checks, metrics)

Output:
```
Mesh 'my-cloud' created.
  Secret: syf_sk_6gp9gu8qfV7k2nP3x8qL4jB5mK...
  Node:   par-hv-1 (fd12:3456:7800:a1b2:c3d4:...)

Run 'syfrah peering' to accept new nodes.
Running daemon... (Ctrl+C to stop)
```

### `syfrah join`

Join an existing mesh by connecting to a node.

```bash
syfrah join 51.210.x.x                        # uses default peering port 51821
syfrah join 51.210.x.x:51821 --pin 1234       # auto-accept with PIN
syfrah join 51.210.x.x --node-name fsn-hv-1 -d
```

| Flag | Default | Description |
|---|---|---|
| `target` | required | IP or IP:port of an existing node |
| `--node-name` | hostname | This node's name |
| `--port` | 51820 | WireGuard listen port |
| `--endpoint` | auto-detect | Public IP:port |
| `--pin` | none | PIN for auto-accept (skips manual approval) |
| `-d, --daemon` | foreground | Run daemon in background |

What it does:
1. Generates a WireGuard keypair
2. Sends a join request to the target node via TCP
3. Waits for approval (manual or PIN-based)
4. Receives: mesh secret, mesh prefix, full peer list
5. Creates `syfrah0` interface, adds all peers
6. Starts the daemon

The joining node is announced to all existing mesh members via encrypted peer announcements.

### `syfrah start`

Restart the daemon from saved state. Used after a reboot or crash.

```bash
syfrah start        # foreground
syfrah start -d     # background
```

Reloads state from `~/.syfrah/state.json`, recreates the WireGuard interface, reapplies all known peers, and starts the daemon loop.

### `syfrah stop`

Stop the running daemon.

```bash
syfrah stop
```

Sends SIGTERM to the daemon process. Waits 3 seconds for graceful shutdown. The WireGuard interface is torn down and the PID file is removed. State is preserved for restart.

### `syfrah leave`

Leave the mesh and clean up all state.

```bash
syfrah leave
```

Tears down the WireGuard interface, removes the control socket, and deletes `~/.syfrah/` entirely. This is irreversible — to rejoin, you need the mesh secret again.

### `syfrah status`

Show mesh and daemon status.

```bash
syfrah status
```

Output:
```
Mesh:      my-cloud
Node:      par-hv-1
Mesh IPv6: fd12:3456:7800:a1b2:c3d4:e5f6:7890:abcd
Prefix:    fd12:3456:7800::/48
WG port:   51820
Secret:    syf_sk_6gp9gu8qfV7k2nP3...
Peering:   port 51821

Daemon:    running (pid 12345)

Interface: syfrah0 (up)
Listen:    :51820
WG peers:  3 configured, 3 with handshake
Traffic:   rx 1.2 MiB / tx 3.4 MiB

Known peers: 3

Metrics:
  Uptime:          2h 30m
  Peers discovered: 3
  WG reconciles:   12
  Peers unreached: 0
```

### `syfrah peers`

List all known peers with live WireGuard stats.

```bash
syfrah peers
```

Output:
```
NAME               MESH IP                                  ENDPOINT               STATUS   HANDSHAKE   TRAFFIC
----------------------------------------------------------------------------------------------------------------
par-hv-2           fd12:3456:7800:1111:2222:3333:4444:5555  51.210.x.y:51820      active       5s ago  1.2K↓ 3.4K↑
fsn-hv-1           fd12:3456:7800:aaaa:bbbb:cccc:dddd:eeee  88.198.x.y:51820      active      12s ago  4.5K↓ 2.1K↑
ams-hv-1           fd12:3456:7800:ffff:0000:1111:2222:3333  51.15.x.y:51820       unreach      1h ago  -
```

Columns:
- **NAME**: peer node name (truncated to 17 chars)
- **MESH IP**: IPv6 address on the mesh
- **ENDPOINT**: public IP:port
- **STATUS**: `active`, `unreach`, or `removed`
- **HANDSHAKE**: time since last WireGuard handshake
- **TRAFFIC**: RX/TX bytes (↓ down, ↑ up)

### `syfrah token`

Display the mesh secret.

```bash
syfrah token
# syf_sk_6gp9gu8qfV7k2nP3x8qL4jB5mK6yR9wX2cF3tG4hJ5
```

Used to share the secret with other operators who need to join the mesh (in combination with `syfrah join --pin`).

### `syfrah rotate`

Rotate the mesh secret. Requires the daemon to be stopped.

```bash
syfrah stop
syfrah rotate
syfrah start
```

Generates a new mesh secret, recomputes the mesh prefix and node IPv6 address, and **clears the entire peer list**. All other nodes must rejoin with the new secret. This is a disruptive operation for security incidents.

### `syfrah peering`

Manage join requests. Has two modes: interactive (default) and non-interactive (subcommands).

#### Interactive mode (default)

```bash
syfrah peering                   # manual approval
syfrah peering --pin 1234        # auto-accept with PIN
```

Watches for incoming join requests. Prompts the operator to accept or reject each one. If a PIN is provided, matching requests are auto-accepted.

Output:
```
Peering active. Watching for join requests...
Press Ctrl+C to stop.

Join request from fsn-hv-1 (88.198.x.y:51820)
  WG pubkey: AaBbCcDdEeFfGgHhIi
  Accept? [Y/n] y
  Accepted: fsn-hv-1 joined the mesh.
```

If no mesh exists, `syfrah peering` auto-creates one (convenient for bootstrapping).

#### Non-interactive subcommands

For use with scripts or when the daemon is already running.

```bash
# Start accepting join requests
syfrah peering start --pin 1234

# Stop accepting
syfrah peering stop

# List pending requests
syfrah peering list

# Accept a specific request
syfrah peering accept abc12345

# Reject a specific request
syfrah peering reject abc12345 --reason "unknown node"
```

`syfrah peering list` output:
```
ID         NAME             ENDPOINT               WG PUBKEY
----------------------------------------------------------------------
abc12345   fsn-hv-1         88.198.x.y:51820       AaBbCcDdEeFfGgH...
def67890   ams-hv-1         51.15.x.y:51820        XxYyZzAaAaAaAaA...

2 pending request(s)
```

## Future commands (planned)

These commands will be added as the corresponding layers are implemented.

### Organization

```bash
syfrah org create acme --admin-email alice@example.com
syfrah org list
syfrah org delete acme
```

### Projects and environments

```bash
syfrah project create backend-api --org acme
syfrah project list --org acme
syfrah project delete backend-api

syfrah env create production --project backend-api --deletion-protection
syfrah env create staging --project backend-api
syfrah env create feat/auth --project backend-api --ttl 48h
syfrah env list --project backend-api
syfrah env destroy feat/auth
```

### Compute

```bash
syfrah vm create --project backend-api --env production \
  --name web-1 --vcpu 2 --memory 4096 --image ubuntu-24.04 --vpc prod

syfrah vm list --project backend-api --env production
syfrah vm start web-1
syfrah vm stop web-1
syfrah vm reboot web-1
syfrah vm delete web-1
syfrah vm ssh web-1
```

### Networking

```bash
syfrah vpc create prod --project backend-api --cidr 10.0.0.0/16
syfrah vpc list --project backend-api
syfrah vpc delete prod

syfrah subnet create web --vpc prod --cidr 10.0.1.0/24
syfrah subnet list --vpc prod

syfrah sg create web-sg --vpc prod
syfrah sg add-rule web-sg --ingress --tcp --port 443 --from 0.0.0.0/0
syfrah sg add-rule web-sg --ingress --tcp --port 22 --from 10.0.0.0/16
syfrah sg list --vpc prod
```

### Storage

```bash
syfrah volume create --project backend-api --env production \
  --name data --size 100

syfrah volume list --project backend-api --env production
syfrah volume attach data --vm web-1
syfrah volume detach data
syfrah volume delete data
syfrah volume snapshot data --name daily-backup
```

### IAM

```bash
syfrah user create --email bob@example.com --name "Bob"
syfrah user list
syfrah user disable bob@example.com

syfrah iam assign bob@example.com --role developer --project backend-api
syfrah iam list --org acme
syfrah iam revoke bob@example.com --project backend-api

syfrah apikey create --project backend-api --role developer --name ci-deploy
syfrah apikey list --project backend-api
syfrah apikey rotate --project backend-api --name ci-deploy
syfrah apikey delete --project backend-api --name ci-deploy

syfrah login --email alice@example.com
syfrah logout
```

## Design principles

### One binary, all operations

Everything goes through `syfrah`. No separate binaries for daemon, CLI, or API. The command determines the behavior.

### Verb-noun ordering

Commands follow `syfrah <noun> <verb>` pattern for resource management (`syfrah vm create`, `syfrah vpc list`) and `syfrah <verb>` for fabric operations (`syfrah init`, `syfrah join`, `syfrah status`).

### Daemon communication

Commands that modify running state (peering, VM lifecycle) communicate with the daemon via Unix domain socket (`~/.syfrah/control.sock`). Commands that only read or modify static state (status, token, rotate) access `~/.syfrah/state.json` directly.

### Output

- Human-readable by default (tables, aligned columns)
- `--json` flag (future) for machine-parseable output
- Errors go to stderr, data goes to stdout
- No emojis, no colors by default (infrastructure tool, often used over SSH)

## Files

All state lives in `~/.syfrah/`:

| File | Purpose |
|---|---|
| `state.json` | Mesh config, peers, metrics (permissions: `0600`) |
| `control.sock` | Unix domain socket for CLI ↔ daemon (permissions: `0600`) |
| `daemon.pid` | Running daemon PID |
| `syfrah.log` | Daemon logs (auto-rotated at 10 MB) |
| `syfrah.log.old` | Previous log file |

## Relationship to other concepts

```
    ┌──────────────────────────────────────────────────────┐
    │                                                      │
    │   CLI              ◄── this document                 │
    │   syfrah binary: fabric ops + future cloud ops       │
    │                                                      │
    ├──────────────────────────────────────────────────────┤
    │                                                      │
    │   Control Plane    ◄── docs/concepts/control-plane.md│
    │   CLI talks to local API, forwarded to Raft leader   │
    │                                                      │
    ├──────────────────────────────────────────────────────┤
    │                                                      │
    │   IAM              ◄── docs/concepts/iam.md          │
    │   login, apikey commands                             │
    │                                                      │
    ├──────────────────────────────────────────────────────┤
    │                                                      │
    │   Fabric           ◄── docs/concepts/fabric.md       │
    │   init, join, peering, peers, status                 │
    │                                                      │
    └──────────────────────────────────────────────────────┘
```
