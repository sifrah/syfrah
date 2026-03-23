# Syfrah

[![CI](https://github.com/sifrah/syfrah/actions/workflows/ci.yml/badge.svg)](https://github.com/sifrah/syfrah/actions/workflows/ci.yml)
[![License: Apache 2.0](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

An open-source control plane that turns rented dedicated servers into a programmable cloud.

Take servers from OVH, Hetzner, Scaleway, or any provider. Syfrah connects them into an encrypted mesh, orchestrates compute, storage, and networking, and exposes cloud services on top.

## Install

**Download a pre-built binary** from the [latest release](https://github.com/sifrah/syfrah/releases/latest):

```bash
# Example: Linux amd64
curl -LO https://github.com/sifrah/syfrah/releases/latest/download/syfrah-v0.1.0-x86_64-unknown-linux-musl.tar.gz
tar xzf syfrah-v0.1.0-x86_64-unknown-linux-musl.tar.gz
sudo mv syfrah /usr/local/bin/
```

**Via cargo install** (requires Rust toolchain):

```bash
cargo install --git https://github.com/sifrah/syfrah.git
```

**From source:**

```bash
git clone https://github.com/sifrah/syfrah.git
cd syfrah
cargo build --release
# Binary at target/release/syfrah
```

## Quick Start

```bash
# Build
cargo build

# Server 1: create a mesh
syfrah fabric init --name my-cloud
syfrah fabric peering --pin 4829

# Server 2: join the mesh
syfrah fabric join 203.0.113.1 --pin 4829

# Check status
syfrah fabric status
syfrah fabric peers
```

Three commands, one minute, encrypted WireGuard mesh between your servers.

## Architecture

```
    Tenant API          ← HTTP REST, any node, forwarded to Raft leader
    IAM + Org Model     ← 4 roles, Org/Project/Environment
    Cloud Products      ← VMs, LBs, managed DBs (forge primitives + config)
    Control Plane       ← Raft (openraft) + gossip (SWIM), embedded on every node
    Compute + Storage   ← Firecracker microVMs + ZeroFS (S3-backed block devices)
    Overlay             ← VXLAN, VPC, security groups, private DNS
    Forge               ← Per-node REST API, manages local resources
    Fabric              ← WireGuard full-mesh, IPv6 ULA, manual peering
    Dedicated Servers   ← OVH, Hetzner, Scaleway + S3 buckets
```

Each layer is a self-contained crate in `layers/`. See [handbook/ARCHITECTURE.md](handbook/ARCHITECTURE.md) for the full design.

## Repository Structure

```
layers/
  core/           syfrah-core     Foundation types, crypto, addressing (no I/O)
  fabric/         syfrah-fabric   WireGuard mesh, peering, daemon, CLI [implemented]
  forge/                          Per-node control and debug [planned]
  compute/                        Firecracker microVMs [planned]
  storage/                        ZeroFS + S3 block storage [planned]
  overlay/                        VXLAN, VPC, security groups [planned]
  controlplane/                   Raft + gossip + scheduler [planned]
  org/                            Org / Project / Environment [planned]
  iam/                            Users, roles, API keys [planned]
  products/                       Product orchestration [planned]

bin/syfrah/       The CLI binary (composes all layers)
handbook/         Project handbook (cross-cutting docs)
```

Each layer has a `README.md` with its concept documentation. Browse any layer folder to understand it.

## Documentation

- [handbook/ARCHITECTURE.md](handbook/ARCHITECTURE.md) — Global architecture, design principles, non-goals, failure model
- [layers/fabric/README.md](layers/fabric/README.md) — Fabric (WireGuard mesh)
- [layers/forge/README.md](layers/forge/README.md) — Forge (per-node control)
- [layers/compute/README.md](layers/compute/README.md) — Compute (Firecracker)
- [layers/storage/README.md](layers/storage/README.md) — Storage (ZeroFS + S3)
- [layers/overlay/README.md](layers/overlay/README.md) — Overlay (VXLAN, VPC)
- [layers/controlplane/README.md](layers/controlplane/README.md) — Control Plane (Raft + gossip)
- [layers/org/README.md](layers/org/README.md) — Organization Model
- [layers/iam/README.md](layers/iam/README.md) — IAM
- [handbook/state-and-reconciliation.md](handbook/state-and-reconciliation.md) — State ownership, reconciliation loop, resource phases
- [handbook/repository.md](handbook/repository.md) — Repo structure conventions

## Building

Requires Rust (see [rust-toolchain.toml](rust-toolchain.toml)).

```bash
cargo build           # build all crates
cargo test            # run tests
cargo clippy          # lint
cargo run -- --help   # run the CLI
```

## Current Status

The **fabric layer** is implemented: WireGuard mesh with manual TCP peering, encrypted peer announcements, IPv6 ULA addressing, daemon with health checks, and a full CLI.

Everything else is architecturally designed (see the concept docs) and ready to implement.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[Apache 2.0](LICENSE)
