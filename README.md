# Syfrah

Open-source platform to transform dedicated servers into a cloud provider.

Take dedicated servers from OVHcloud, Hetzner, Scaleway or any provider and turn them into a coherent, multi-zone, multi-tenant cloud with VMs, VPCs, load balancers and managed PostgreSQL.

## Why

Cloud providers are powerful but expensive. Dedicated servers are cheap but raw. Syfrah bridges the gap: a control plane, encrypted mesh networking, compute orchestration and a modern API on top of bare metal you already rent.

## Current State

The first building block is implemented: a **decentralized WireGuard mesh network** with auto-discovery, encrypted gossip, and full operational tooling.

```bash
# Node 1: create a mesh
$ syfrah init --name production
Mesh 'production' created.
  Secret: syf_sk_5HueCGU8rMjxEXxiPuD5BDku4MkFq...
  Node:   node-1 (fd9a:bc12:7800::a1f3:1)
Share the secret with other nodes to join.

# Node 2: join with just the secret
$ syfrah join syf_sk_5HueCGU8rMjxEXxiPuD5BDku4MkFq...
Joined mesh 'mesh'.
  Node: node-2 (fd9a:bc12:7800::b2e4:2)

# Monitor
$ syfrah status
Mesh:      production
Node:      node-1
Daemon:    running (pid 12345)
Interface: syfrah0 (up)
WG peers:  2 configured, 2 with handshake
Traffic:   rx 12.3 MiB / tx 8.7 MiB
Metrics:
  Uptime:          2h 15m
  Gossip received: 342

$ syfrah peers
NAME               MESH IP                                  ENDPOINT               STATUS   HANDSHAKE    TRAFFIC
----------------------------------------------------------------------------------------------------------------
node-2             fd9a:bc12:7800::b2e4:2                   198.51.100.5:51820       active     12s ago  4K~ 2K~
node-3             fd9a:bc12:7800::c5d6:3                   192.0.2.10:51820         active      3m ago  1M~ 800K~
```

## How It Works

### Two planes

| Layer | Role | Technology |
|-------|------|------------|
| **Control plane** | Peer discovery, key exchange, state sync | [iroh](https://iroh.computer) (PKARR/DHT + gossip) |
| **Data plane** | Encrypted tunnels, IPv6 mesh | WireGuard via [wireguard-control](https://crates.io/crates/wireguard-control) |

### One secret rules them all

A single shared secret (`syf_sk_...`) derives everything:

```
mesh_secret (256 bits)
  |-- mesh_id           identifies the mesh
  |-- dht_topic_key     DHT lookup key
  |-- encryption_key    AES-256-GCM for gossip records
  |-- gossip_topic      iroh-gossip topic
  |-- mesh_prefix       deterministic fd::/48 ULA prefix
```

The token also embeds the bootstrap node's iroh PublicKey, so joining nodes find the mesh on the DHT automatically.

### Auto-discovery flow

1. **Init**: creates iroh endpoint, auto-publishes to PKARR/DHT, subscribes to gossip
2. **Join**: parses token, resolves bootstrap node via DHT (30s timeout), joins gossip
3. **Gossip**: encrypted `PeerRecord` broadcasts (WG pubkey, endpoint, mesh IPv6)
4. **Reconciliation**: each gossip event triggers WireGuard interface update
5. **Heartbeat**: every 60s, each node re-broadcasts with updated `last_seen`
6. **Unreachable detection**: peers silent for 5+ minutes are marked unreachable

### Security

| Threat | Mitigation |
|--------|-----------|
| Eavesdropping on DHT | Records encrypted with AES-256-GCM |
| Unauthorized mesh join | Requires the shared secret |
| NAT/firewall blocking | iroh relay servers (~100% traversal) |
| Key compromise | `syfrah rotate` + rejoin all peers |
| State file exposure | Permissions 0600, contains private keys |

## CLI Reference

| Command | Description |
|---------|-------------|
| `syfrah init --name <mesh>` | Create a new mesh, display the token, run daemon |
| `syfrah join <token>` | Join a mesh using the shared token, run daemon |
| `syfrah start` | Restart daemon from saved state (after stop/crash) |
| `syfrah stop` | Stop the running daemon (SIGTERM) |
| `syfrah status` | Show mesh info, daemon status, WG stats, metrics |
| `syfrah peers` | List peers with handshake times and traffic |
| `syfrah token` | Display the mesh token for sharing |
| `syfrah rotate` | Rotate the mesh secret (all peers must rejoin) |
| `syfrah leave` | Announce departure, tear down interface, clear state |

Options for `init` and `join`:
- `--node-name <name>` — node name (defaults to hostname)
- `--port <port>` — WireGuard listen port (default: 51820)
- `--endpoint <ip:port>` — public endpoint for WireGuard

## Architecture

```
syfrah/
  crates/
    syfrah-core/          Pure types, crypto, addressing (no I/O)
      src/
        secret.rs         MeshSecret + MeshToken (key derivation)
        identity.rs       NodeIdentity
        addressing.rs     ULA IPv6 prefix + node address derivation
        mesh.rs           PeerRecord, AES-256-GCM encrypt/decrypt

    syfrah-net/           Network layer (WireGuard + iroh + daemon)
      src/
        wg.rs             WireGuard interface management
        discovery.rs      iroh DHT + gossip (MeshNode)
        daemon.rs         Daemon loop, init/join/start/leave flows
        store.rs          State persistence + PID file (~/.syfrah/)

    syfrah-cli/           CLI binary
      src/
        main.rs           clap command routing
        commands/
          init.rs         syfrah init
          join.rs         syfrah join
          start.rs        syfrah start
          stop.rs         syfrah stop
          status.rs       syfrah status
          peers.rs        syfrah peers
          token.rs        syfrah token
          rotate.rs       syfrah rotate
          leave.rs        syfrah leave

  doc/
    01-core-types.md      Secret, token, identity, addressing, encryption
    02-wireguard-wrapper.md  WireGuard interface management
    03-discovery.md       iroh DHT + gossip architecture
    04-init-join.md       Init/join flows, daemon loop
    05-daemon-commands.md Status, peers, leave commands
    06-operations.md      Full operations guide (all commands, lifecycle, security)

  tests/
    e2e/
      Dockerfile          Docker image with syfrah + WireGuard
      docker-compose.yml  3-node test setup
      run.sh              Automated E2E test script
```

## Building

Requires Rust 1.89+.

```bash
cargo build
cargo test       # 32 tests
cargo clippy     # 0 warnings
```

Run the CLI:
```bash
cargo run --bin syfrah -- init --name test
cargo run --bin syfrah -- status
cargo run --bin syfrah -- peers
cargo run --bin syfrah -- token
```

E2E test (requires Docker):
```bash
bash tests/e2e/run.sh
```

## Roadmap

- [x] **Mesh networking** — WireGuard mesh with auto-discovery via iroh DHT/gossip
- [x] **Daemon management** — start/stop/restart, PID file, graceful shutdown
- [x] **Peer lifecycle** — heartbeat, unreachable detection, departure announcement
- [x] **Secret rotation** — rotate mesh secret with `syfrah rotate`
- [x] **Observability** — metrics (gossip, reconciliations, uptime), live WG stats
- [x] **E2E testing** — Docker-based 3-node test setup
- [ ] **Compute** — Firecracker microVM orchestration
- [ ] **VPC/Subnets** — Network isolation, security rules
- [ ] **Load balancers** — L4 load balancing across VMs
- [ ] **Managed PostgreSQL** — Provisioning, backups, restore
- [ ] **Multi-tenant** — Organizations, projects, IAM
- [ ] **SaaS layer** — Web dashboard, IPv4 gateway, onboarding

## Tech Stack

- **Language**: Rust
- **Networking**: WireGuard (wireguard-control), iroh 0.97 (QUIC/DHT/gossip)
- **Crypto**: AES-256-GCM, x25519, ed25519, SHA-256
- **Addressing**: IPv6 ULA (fd::/8)
- **CLI**: clap
- **Async**: tokio

## License

Apache 2.0
