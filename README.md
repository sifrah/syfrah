# Syfrah

Open-source platform to transform dedicated servers into a cloud provider.

Take dedicated servers from OVHcloud, Hetzner, Scaleway or any provider and turn them into a coherent, multi-zone, multi-tenant cloud with VMs, VPCs, load balancers and managed PostgreSQL.

## Why

Cloud providers are powerful but expensive. Dedicated servers are cheap but raw. Syfrah bridges the gap: a control plane, encrypted mesh networking, compute orchestration and a modern API on top of bare metal you already rent.

## Quick Start

```bash
# Server 1: create a mesh and start accepting peers
$ syfrah init --name production
$ syfrah peering --pin 4829
Peering active (auto-accept with PIN: 4829)
New nodes can join with: syfrah join <this-ip> --pin 4829

# Server 2: join with just the IP
$ syfrah join 203.0.113.1 --pin 4829
Joined mesh 'production'.
  Node: node-2 (fd9a:bc12:7800::b2e4:2)

# Server 3: same thing
$ syfrah join 203.0.113.1 --pin 4829
Joined mesh 'production'.
  Node: node-3 (fd9a:bc12:7800::c5d6:3)
```

That's it. All servers now have encrypted WireGuard tunnels and can reach each other via IPv6.

```bash
$ syfrah peers
NAME               MESH IP                                  ENDPOINT               STATUS   HANDSHAKE    TRAFFIC
----------------------------------------------------------------------------------------------------------------
node-2             fd9a:bc12:7800::b2e4:2                   198.51.100.5:51820       active     12s ago  4K↓ 2K↑
node-3             fd9a:bc12:7800::c5d6:3                   192.0.2.10:51820         active      3s ago  1M↓ 800K↑
```

## How It Works

### Manual Peering

Syfrah uses a **manual peering** model. No automatic discovery, no DHT, no gossip. An operator explicitly approves each node that joins the mesh.

1. **Init**: creates a mesh with a shared secret and WireGuard interface
2. **Peering**: the operator starts listening for join requests (`syfrah peering`)
3. **Join**: a new node connects to an existing node and sends a join request
4. **Accept/Reject**: the operator approves or denies (or uses a PIN for auto-accept)
5. **Propagation**: when accepted, all existing mesh members are automatically notified of the new peer

### Two planes

| Layer | Role | Technology |
|-------|------|------------|
| **Control plane** | Peering, key exchange, peer announcements | TCP + AES-256-GCM |
| **Data plane** | Encrypted tunnels, IPv6 mesh | WireGuard |

### Security

| Threat | Mitigation |
|--------|-----------|
| Unauthorized mesh join | Manual operator approval or PIN |
| Eavesdropping on peer records | AES-256-GCM encryption (mesh secret) |
| Key compromise | `syfrah rotate` + rejoin all peers |
| State file exposure | Permissions 0600, contains private keys |
| Control socket access | Unix domain socket, mode 0600 |

## CLI Reference

| Command | Description |
|---------|-------------|
| `syfrah init --name <mesh>` | Create a new mesh, run daemon |
| `syfrah join <ip>` | Join a mesh via an existing node |
| `syfrah join <ip> --pin <pin>` | Join with auto-accept PIN |
| `syfrah peering` | Interactive mode: watch and accept/reject requests |
| `syfrah peering --pin <pin>` | Auto-accept mode with PIN |
| `syfrah peering start` | Start peering listener (non-interactive) |
| `syfrah peering list` | List pending join requests |
| `syfrah peering accept <id>` | Accept a pending request |
| `syfrah peering reject <id>` | Reject a pending request |
| `syfrah start` | Restart daemon from saved state |
| `syfrah stop` | Stop the running daemon |
| `syfrah status` | Show mesh info, daemon status, WG stats |
| `syfrah peers` | List peers with handshake times and traffic |
| `syfrah token` | Display the mesh secret |
| `syfrah leave` | Tear down interface, clear state |

Options for `init` and `join`:
- `--node-name <name>` — node name (defaults to hostname)
- `--port <port>` — WireGuard listen port (default: 51820)
- `--endpoint <ip:port>` — public endpoint for WireGuard
- `--peering-port <port>` — TCP peering port (default: WG port + 1)

## Architecture

```
syfrah/
  crates/
    syfrah-core/          Pure types, crypto, addressing (no I/O)
      src/
        secret.rs         MeshSecret (key derivation)
        identity.rs       NodeIdentity
        addressing.rs     ULA IPv6 prefix + node address derivation
        mesh.rs           PeerRecord, JoinRequest/Response, encryption

    syfrah-net/           Network layer (WireGuard + peering + daemon)
      src/
        wg.rs             WireGuard interface management
        peering.rs        TCP peering protocol + peer announcements
        control.rs        Unix domain socket (CLI <-> daemon)
        daemon.rs         Daemon loop, init/join/start/leave flows
        store.rs          State persistence (~/.syfrah/)

    syfrah-cli/           CLI binary
      src/
        main.rs           clap command routing
        commands/
          init.rs         syfrah init
          join.rs         syfrah join
          peering.rs      syfrah peering (interactive + subcommands)
          start.rs        syfrah start
          stop.rs         syfrah stop
          status.rs       syfrah status
          peers.rs        syfrah peers
          token.rs        syfrah token
          rotate.rs       syfrah rotate
          leave.rs        syfrah leave
```

## Building

Requires Rust 1.89+.

```bash
cargo build
cargo clippy
```

Run the CLI:
```bash
cargo run --bin syfrah -- init --name test
cargo run --bin syfrah -- status
cargo run --bin syfrah -- peers
```

## Roadmap

- [x] **Mesh networking** — WireGuard mesh with manual peering
- [x] **Daemon management** — start/stop/restart, PID file, graceful shutdown
- [x] **Peer lifecycle** — heartbeat, unreachable detection, peer announcements
- [x] **Peering UX** — interactive mode, PIN auto-accept, auto-init
- [x] **Observability** — metrics, live WG stats
- [ ] **Compute** — Firecracker microVM orchestration
- [ ] **VPC/Subnets** — Network isolation, security rules
- [ ] **Load balancers** — L4 load balancing across VMs
- [ ] **Managed PostgreSQL** — Provisioning, backups, restore
- [ ] **Multi-tenant** — Organizations, projects, IAM
- [ ] **SaaS layer** — Web dashboard, IPv4 gateway, onboarding

## Tech Stack

- **Language**: Rust
- **Networking**: WireGuard (wireguard-control)
- **Crypto**: AES-256-GCM, x25519, SHA-256
- **Addressing**: IPv6 ULA (fd::/8)
- **CLI**: clap
- **Async**: tokio

## License

Apache 2.0
