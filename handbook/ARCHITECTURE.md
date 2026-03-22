# Architecture

Syfrah is an open-source control plane that turns rented dedicated servers into a programmable cloud. You rent servers from OVH, Hetzner, Scaleway, or any provider — Syfrah connects them, orchestrates them, and exposes cloud services on top.

This document describes the global architecture. Each layer has its own `README.md` with detailed documentation. Browse [`layers/`](../layers/) to explore.

## Design principles

1. **No external dependencies.** The control plane is embedded in every node. No etcd, no Consul, no external database. A single binary, a WireGuard mesh, and an S3 bucket — that's the entire infrastructure.

2. **Dedicated-servers-first.** The platform is designed for rented bare-metal machines, not VMs-on-VMs. Compute density, direct hardware access, and provider-agnostic operation are first-class concerns.

3. **IPv6-native, IPv4 as compatibility layer.** The fabric uses IPv6 ULA internally. Tenant VMs get private IPv4 inside the overlay and can receive public IPv6 directly. IPv4 ingress is handled via NAT and floating IPs.

4. **Reconciliation over imperative orchestration.** The control plane declares desired state (Raft). Each node's forge continuously reconciles local reality to match. Operations are idempotent. Crashes are safe. State is rebuildable.

5. **Simple products from generic primitives.** The forge manages VMs, volumes, and network interfaces — nothing more. Products (databases, load balancers) are opinionated compositions of these primitives. Adding a product never requires changing the forge.

## Non-goals

These are deliberate choices, not missing features:

- **No live migration.** Firecracker does not support it. VM migration is stop → move volume (S3-backed, no data copy) → start. Downtime is ~5-30 seconds.
- **No custom IAM policy language.** 4 built-in roles, per-org/project scoping. No JSON policies, no policy simulator, no condition keys.
- **No L2 flood-and-learn networking.** FDB entries are statically populated by the control plane. ARP is proxied. Zero broadcast traffic in steady state.
- **No distributed storage cluster.** No Ceph, no GlusterFS. Storage durability is delegated to the provider's S3. ZeroFS handles caching and block device abstraction.
- **No hyperscaler-style AZ guarantees.** Zones and regions are operator-defined labels. The platform does not enforce physical isolation between AZs — the operator's choice of providers and locations determines actual fault domain boundaries.
- **No Windows guests.** Firecracker boots Linux kernels directly (ELF), with no BIOS/UEFI. Windows requires UEFI, which Firecracker does not provide. The only non-Linux guest supported is OSv (a unikernel).
- **No GPU passthrough in v1.** Firecracker has no PCI passthrough by design (minimal attack surface). GPU workloads would require Cloud Hypervisor or QEMU with VFIO — a future consideration, not a current goal. This is the same split used by Koyeb and AWS (Firecracker for CPU, different stack for GPU).

## The stack

```
    ┌─────────────────────────────────────────────────────────────┐
    │                                                             │
    │   Tenant API                                                │
    │   HTTP REST on every node's fabric address                  │
    │   Any node accepts requests (forwarded to Raft leader)      │
    │                                                             │
    ├─────────────────────────────────────────────────────────────┤
    │                                                             │
    │   IAM                           Organization Model          │
    │   4 roles (owner, admin,        Org → Project → Env         │
    │   developer, viewer)            Custom env names, TTL,      │
    │   Per-org/project scoping       deletion protection         │
    │   API keys per project                                      │
    │                                                             │
    ├─────────────────────────────────────────────────────────────┤
    │                                                             │
    │   Cloud Products                                            │
    │   VMs, Load Balancers, Managed PostgreSQL, ...              │
    │   Each product = forge primitives + product-specific config  │
    │                                                             │
    ├─────────────────────────────────────────────────────────────┤
    │                                                             │
    │   Control Plane                                             │
    │   Raft (openraft) for strongly consistent state             │
    │   Gossip (SWIM) for health, metrics, status                 │
    │   Embedded on every node — no external dependencies         │
    │                                                             │
    ├──────────┬──────────────────┬───────────────────────────────┤
    │          │                  │                               │
    │  Compute │     Overlay      │        Storage                │
    │          │                  │                               │
    │  Firecracker   VXLAN/VPC    │  ZeroFS + S3                  │
    │  microVMs      Security     │  Block devices backed         │
    │  <125ms boot   groups       │  by object storage            │
    │  ~5MB overhead  Private DNS │  Local SSD cache              │
    │          │                  │                               │
    ├──────────┴──────────────────┴───────────────────────────────┤
    │                                                             │
    │   Forge                                                     │
    │   Per-node REST API on the fabric                           │
    │   Manages Firecracker, bridges, VXLAN, ZeroFS, nftables    │
    │                                                             │
    ├─────────────────────────────────────────────────────────────┤
    │                                                             │
    │   Fabric                                                    │
    │   WireGuard full-mesh between all nodes                     │
    │   IPv6 ULA addressing, encrypted, manual peering            │
    │                                                             │
    ├─────────────────────────────────────────────────────────────┤
    │                                                             │
    │   Dedicated Servers        +        S3 Buckets              │
    │   (OVH, Hetzner, Scaleway)          (same providers)        │
    │                                                             │
    └─────────────────────────────────────────────────────────────┘
```

## Layer by layer

### Fabric — the network foundation

The **fabric** is a WireGuard full-mesh connecting all nodes. It replaces the physical datacenter network that cloud providers have. Every node can reach every other node over encrypted IPv6 tunnels, regardless of where they are hosted.

- WireGuard (ChaCha20-Poly1305) encrypts all inter-node traffic
- IPv6 ULA `/48` per mesh, `/128` per node, derived from mesh secret
- Manual peering: operator approves every join request (PIN or manual accept)
- No central coordinator — the mesh is fully decentralized

**Concept doc:** [`layers/fabric/README.md`](../layers/fabric/README.md)

### Forge — per-node control

The **forge** runs on every node. It exposes a REST API bound exclusively to the fabric interface (`syfrah0`), invisible from the internet.

The forge is the bridge between the control plane and the local hardware. It manages:
- Firecracker processes (VM lifecycle)
- Linux bridges and TAP devices (overlay networking)
- VXLAN interfaces (VPC isolation)
- ZeroFS volumes (block storage)
- nftables rules (security groups)

The forge is intentionally generic: it manages compute, storage, and networking primitives, not product semantics. It creates VMs, volumes, and network interfaces. Products add the domain knowledge on top.

**Concept doc:** [`layers/forge/README.md`](../layers/forge/README.md)

### Compute — Firecracker microVMs

Every workload runs as a **Firecracker microVM**. Firecracker provides hardware-level isolation via KVM with minimal overhead (~5 MB per VM, <125ms boot).

- One Firecracker process per VM (3 threads: API, VMM, vCPU)
- 5 emulated devices only (virtio-net, virtio-block, virtio-vsock, serial, i8042)
- Jailer for OS-level isolation (namespaces, chroot, cgroups, seccomp)
- Shared read-only kernel across all VMs
- Snapshots with lazy memory restore (<30ms)
- VM migration via stop → move volume → start (no live migration)

**Concept doc:** [`layers/compute/README.md`](../layers/compute/README.md)

### Storage — ZeroFS + S3

Block storage is backed by **S3-compatible object storage** (from the same providers where servers are rented). [ZeroFS](https://github.com/Barre/ZeroFS) turns S3 into usable block devices.

- Data is chunked (256KB), compressed (LZ4), and encrypted (XChaCha20-Poly1305)
- Local SSD + memory cache absorbs >95% of I/O
- NBD (Network Block Device) endpoints connect directly to Firecracker's virtio-block
- Volumes can move between nodes without copying data (S3 is the source of truth)
- Snapshots are lightweight (no data copy, just SST file metadata)

No Ceph, no GlusterFS, no storage cluster. The S3 provider handles durability and replication.

**Concept doc:** [`layers/storage/README.md`](../layers/storage/README.md)

### Overlay — tenant networking

The **overlay** provides VPCs, subnets, and network isolation for tenant VMs. It runs on top of the fabric using VXLAN encapsulation.

- One VXLAN VNI per VPC (24-bit, ~16 million possible VPCs)
- One Linux bridge + VXLAN interface per VPC per node
- Static FDB entries (no flood-and-learn, control plane populates everything)
- ARP proxy eliminates broadcast traffic
- Security groups via nftables per TAP device (stateful, conntrack)
- Anti-spoofing on every VM (MAC + IP source validation)
- Distributed routing between subnets within a VPC
- Per-node SNAT for internet egress, floating IPs for ingress
- Direct public IPv6 (no NAT) via the fabric's IPv6-native design
- Private DNS: CoreDNS per node, `{vm}.{vpc}.syfrah.internal`, auto-registered

**Concept doc:** [`layers/overlay/README.md`](../layers/overlay/README.md)

### Control plane — distributed brain

The control plane runs on **every node**. No dedicated controller. Internally, one node is elected Raft leader — but this is automatic and invisible to the operator.

Two complementary protocols:

| Protocol | What it handles | Consistency |
|---|---|---|
| **Raft** (openraft) | IP allocation, VM scheduling, VPC config, org/project/env, IAM | Strong (linearizable) |
| **Gossip** (SWIM) | Node health, available resources, VM status, FDB entries, DNS | Eventual (~2-5 seconds) |

Raft state is **prescriptive** (what should exist). Gossip state is **descriptive** (what is actually happening). The forge on each node reconciles the two: it reads desired state from Raft, compares with local reality, and acts on the differences. Every resource follows a phase model (Pending → Provisioning → Running → Deleting → Deleted) that the reconciliation loop drives forward.

- 1 node: single-node Raft, zero overhead
- 2 nodes: replication but no fault tolerance
- 3+ nodes: full HA with automatic failover
- 7+ nodes: cap voters at 5-7, rest are learners

All Raft and gossip traffic travels over the WireGuard fabric — encrypted for free.

**Concept docs:** [`layers/controlplane/README.md`](../layers/controlplane/README.md), [`handbook/state-and-reconciliation.md`](state-and-reconciliation.md)

### Organization model — multi-tenancy

Three levels, no more:

```
    Organization
    └── Project
        └── Environment
            └── Resources
```

- **Organization**: the root tenant (company, team)
- **Project**: a logical grouping (product, service)
- **Environment**: a runtime context (production, staging, feat/auth-v2)

Environments are first-class with custom names. No tiers, no categories. Just a name + optional TTL (auto-destroy) + optional deletion protection. Cost rolls up automatically through the hierarchy — no tags needed.

**Concept doc:** [`layers/org/README.md`](../layers/org/README.md)

### IAM — permissions

4 built-in roles, per-org or per-project scoping, per-project API keys.

| Role | Can do |
|---|---|
| **Owner** | Everything + manage users, billing |
| **Admin** | Manage infra: projects, VPCs, deletion protection |
| **Developer** | Day-to-day: VMs, environments, volumes, security groups |
| **Viewer** | Read-only: logs, metrics, status |

API keys are scoped to one project with a role. Format: `syf_key_{project}_{random}`. Auth is email + password (self-hosted), OAuth later (SaaS).

No custom roles. No per-environment permissions. No policy language. One permission table.

**Concept doc:** [`layers/iam/README.md`](../layers/iam/README.md)

### Cloud products — the services tenants use

Products are opinionated compositions of generic infrastructure primitives. Every product is ultimately one or more VMs with specific provisioning on top.

```
    Product = forge primitives + product-specific configuration
```

A managed database is a VM + volume + database software configured inside. A load balancer is a VM + reverse proxy configured inside. The forge provides the infrastructure; the product provides the intelligence.

**Concept doc:** [`layers/products/README.md`](../layers/products/README.md)

### Zones and regions — topology

Regions and availability zones are logical labels on nodes. They represent where servers are physically located and enable topology-aware decisions (placement, resilience, routing).

- The fabric is **flat** — all nodes connected to all nodes, topology-unaware
- Zones and regions are **metadata** used by the control plane and overlay
- A region = a geographic area (eu-west, us-east)
- An AZ = an isolated group within a region (eu-west-1, eu-west-ovh)

**Concept doc:** [`handbook/zones-and-regions.md`](zones-and-regions.md)

## How it all connects

### Creating a VM: the full flow

```
    Tenant: POST /v1/vms { vcpu: 2, memory: 4GB, image: ubuntu-24.04, vpc: prod }
         │
    1. API (any node) → authenticates (IAM) → resolves org/project/env
         │
    2. Control plane (Raft leader):
       ├── Allocates IP from subnet pool (strongly consistent)
       ├── Picks a node (scheduler reads gossip for capacity)
       └── Commits scheduling decision to Raft log
         │
    3. Target node's forge:
       ├── Creates TAP device, attaches to VPC bridge
       ├── Adds VXLAN FDB entries on remote nodes (via gossip)
       ├── Creates ZeroFS NBD volume (backed by S3)
       ├── Starts Firecracker process (jailer + seccomp)
       ├── Configures VM: kernel, rootfs, data volume, network
       └── Applies security group rules (nftables)
         │
    4. VM boots (<125ms), gets IP via config drive
         │
    5. DNS record auto-created: web-1.prod.syfrah.internal
         │
    6. Gossip propagates: "VM web-1 is running on Node B"
```

### Packet flow: VM-A → VM-B across nodes

```
    VM-A (10.0.1.5, Node 1)                    VM-B (10.0.1.9, Node 2)

    Application sends packet
         │
    eth0 (virtio-net) → TAP device
         │
    nftables: security group check (egress)
         │
    Linux bridge (br-100) → FDB lookup
         │
    VXLAN encapsulation (VNI 100)
    [UDP:4789][VXLAN header][original frame]
         │
    syfrah0 (WireGuard) → encrypted
    [WireGuard][VXLAN][original frame]
         │
    ────── Internet (opaque to observers) ──────
         │
    Node 2: WireGuard decrypts
         │
    VXLAN decapsulates → bridge → TAP
         │
    nftables: security group check (ingress)
         │
    VM-B receives packet
```

### Storage flow: VM write → S3

```
    VM writes to /dev/vdb
         │
    Firecracker virtio-block
         │
    ZeroFS NBD device
         │
    ├── Memory buffer (microseconds)
    ├── SSD cache (persistent, fast)
    └── S3 flush (background, durable)
         │
    Data is chunked, compressed (LZ4),
    encrypted (XChaCha20-Poly1305),
    and stored in the provider's S3 bucket.
```

## What the operator provides vs. what Syfrah adds

```
    Operator provides:               Syfrah adds:
    ──────────────────               ───────────────

    Dedicated servers                Fabric (WireGuard mesh)
    (OVH, Hetzner, Scaleway,        Forge (per-node control)
     or any provider)                Compute (Firecracker)
                                     Overlay (VXLAN, VPC, DNS)
    S3 bucket                        Storage (ZeroFS)
    (same provider or any            Control plane (Raft + gossip)
     S3-compatible storage)          Organization model
                                     IAM
                                     Cloud products
```

## Technology choices

| Component | Technology | Why |
|---|---|---|
| Language | Rust | Performance, safety, single binary |
| Async runtime | Tokio | Standard for async Rust |
| WireGuard | `wireguard-control` crate | Kernel WireGuard management |
| Consensus | `openraft` | Embedded Raft, async, supports 1-N nodes |
| Gossip | `foca` | Transport-agnostic SWIM, works over WireGuard |
| State store | `redb` | Embedded KV, pure Rust, ACID, no C deps |
| API server | `axum` | HTTP framework, tokio-native |
| Compute | Firecracker | MicroVMs, <125ms boot, ~5MB overhead, Apache 2.0 |
| Storage engine | ZeroFS | S3-backed block devices, Rust, local cache |
| Overlay encap | VXLAN | Standard, kernel-native, 16M VNIs |
| Firewall | nftables | Per-VM security groups, stateful, conntrack |
| DNS | CoreDNS | Lightweight, per-VPC zones |
| Serialization | serde + JSON | All public types are Serialize/Deserialize |
| Errors | thiserror (lib), anyhow (bin) | Project convention |

## Repository structure

The repo is organized by architectural layer. Each layer is a self-contained folder with its own code, documentation (README.md), and CLI commands. See [`handbook/repository.md`](repository.md) for the full conventions.

```
    syfrah/
    ├── layers/
    │   ├── core/                 syfrah-core: pure types, crypto, addressing
    │   │                         No I/O, no async. The foundation.
    │   │
    │   ├── fabric/               syfrah-fabric: WireGuard mesh + peering + CLI
    │   │                         Implemented. The first layer.
    │   │
    │   ├── forge/                Per-node debug/ops (planned)
    │   ├── compute/              Firecracker microVMs (planned)
    │   ├── storage/              ZeroFS + S3 block storage (planned)
    │   ├── overlay/              VXLAN, VPC, security groups (planned)
    │   ├── controlplane/         Raft + gossip + scheduler (planned)
    │   ├── org/                  Org / Project / Environment (planned)
    │   ├── iam/                  Users, roles, API keys (planned)
    │   └── products/             Product orchestration (planned)
    │
    ├── bin/
    │   └── syfrah/               Binary — composes all layers, zero logic
    │
    ├── handbook/
    │   ├── repository.md         Repo structure conventions
    │   ├── cli.md                CLI command tree
    │   ├── state-and-reconciliation.md
    │   └── zones-and-regions.md
    │
    ├── ARCHITECTURE.md       This file
    ├── repository.md         Repo conventions
    ├── cli.md                CLI command tree
    ├── state-and-reconciliation.md
    └── zones-and-regions.md
```

## Failure model

The architecture assumes a hostile operational environment. Nodes are rented machines across different providers, connected over the public internet. Failures are expected, not exceptional.

**Assumptions the system is built around:**

- **A node may disappear permanently.** Hardware failure, provider outage, or operator error. The control plane detects this via gossip (~15s), declares the node dead via Raft (~60s), and reschedules affected VMs.

- **Gossip may lag.** A node's view of the cluster may be seconds behind reality. The scheduler uses gossip as a hint, not as truth. Scheduling decisions are committed through Raft, which is authoritative.

- **The Raft leader may change during an operation.** Leader election takes 1-5 seconds. In-flight writes are retried by the client. The new leader replays uncommitted entries. Operations are designed to be re-entrant.

- **Forge actions must be idempotent.** Creating a bridge that already exists is a no-op. Starting a Firecracker process that is already running is a no-op. The reconciliation loop can run any number of times and produce the same result.

- **Derived state must be rebuildable from scratch.** FDB entries, nftables rules, DNS zone files, VXLAN bridges — all are derived from Raft state. If a node loses all derived state (reboot, crash), the next reconciliation loop rebuilds everything from Raft.

- **S3 may be temporarily unavailable.** ZeroFS buffers writes in local cache. Short S3 outages are absorbed. Extended outages cause I/O stalls — the VM continues running but write-heavy workloads will eventually block.

- **The network may partition.** The majority Raft partition continues operating. The minority partition becomes read-only. Existing VMs on both sides keep running. When the partition heals, Raft replays and gossip converges. No split-brain.

**See also:** [`handbook/state-and-reconciliation.md`](state-and-reconciliation.md) for detailed phase models and reconciliation loop design.

## Current status

The **fabric layer** is implemented and functional: WireGuard mesh with manual TCP peering, encrypted peer announcements, IPv6 ULA addressing, daemon with health checks, and a full CLI (init, join, start, stop, leave, peers, peering, status, token, rotate).

Everything else described in this document is the architectural plan, documented in detail in the concept docs, ready to be implemented.
