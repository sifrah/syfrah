# Compute

## What is the compute layer?

The compute layer runs tenant workloads. Every VM, every managed database, every load balancer ultimately runs as a **KVM-based microVM via [Cloud Hypervisor](https://github.com/cloud-hypervisor/cloud-hypervisor)** on a node in the fabric.

Cloud Hypervisor is an open-source Rust-based VMM (maintained by Intel and the community) that runs as a **separate process per VM**. Each VM is a `cloud-hypervisor` child process managed by the forge via a REST API. This architecture means VMs survive forge restarts — critical for zero-downtime updates. Cloud Hypervisor provides hardware-level isolation via KVM with minimal overhead.

```
    ┌──────────────────────────────────────────────────────┐
    │                     Node                              │
    │                                                       │
    │  ┌────────┐  ┌────────┐  ┌────────┐  ┌────────┐     │
    │  │ VM-1   │  │ VM-2   │  │ VM-3   │  │ VM-4   │     │
    │  │ 2 vCPU │  │ 1 vCPU │  │ 4 vCPU │  │ 1 vCPU │     │
    │  │ 4 GB   │  │ 1 GB   │  │ 8 GB   │  │ 512 MB │     │
    │  └───┬────┘  └───┬────┘  └───┬────┘  └───┬────┘     │
    │      │           │           │           │           │
    │  Each VM is a separate cloud-hypervisor process.      │
    │  Hardware isolation via KVM.                          │
    │  ~13 MB overhead per VM.                              │
    │                                                       │
    │  ┌────────────────────────────────────────────────┐   │
    │  │  Linux kernel + KVM                            │   │
    │  │  Dedicated server hardware                     │   │
    │  └────────────────────────────────────────────────┘   │
    └──────────────────────────────────────────────────────┘
```

## Why Cloud Hypervisor

| | **Cloud Hypervisor** | **QEMU** | **Firecracker** | **libkrun** |
|---|---|---|---|---|
| **Architecture** | Separate process per VM with REST API | Standalone process | Standalone process + jailer | Library (embeds in host) |
| **Overhead per VM** | ~13 MB | ~130 MB | ~5 MB | ~5 MB |
| **Boot time** | ~200 ms | Seconds | <125 ms | <125 ms |
| **Device model** | Moderate (virtio-focused) | Hundreds (full PC) | 5 devices | Minimal (virtio only) |
| **Attack surface** | Moderate | Large | Minimal | Minimal |
| **Language** | Rust | C | Rust | Rust + C |
| **virtio-fs** | Yes | Yes | No | Yes |
| **GPU passthrough** | Yes (VFIO) | Yes | No | No |
| **Live migration** | Yes | Yes | No | No |
| **VMs survive host process restart** | Yes | Yes | No | No |
| **License** | Apache 2.0 | GPL 2.0 | Apache 2.0 | Apache 2.0 |

Cloud Hypervisor's trade-off is clear: **separate process per VM with REST API management, GPU support via VFIO passthrough, and VM survival across forge restarts — at slightly higher memory overhead than Firecracker**. The REST API enables clean lifecycle management without embedding VMM code into the forge. VMs are independent processes that persist even when the forge restarts, enabling zero-downtime updates.

Key advantages over embedded VMMs: Cloud Hypervisor runs as a separate process per VM, managed by the forge via REST API. VMs survive forge restarts — critical for zero-downtime platform updates. No jailer needed (unlike Firecracker), with process isolation via cgroups v2 + namespaces.

Key advantages over QEMU: written in Rust with a much smaller attack surface, purpose-built for cloud workloads, and significantly lower memory overhead per VM.

### GPU support

Cloud Hypervisor supports two GPU modes:

- **virtio-gpu** — shared GPU for lightweight rendering workloads
- **VFIO passthrough** — dedicated GPU with full hardware access, supporting NVIDIA CUDA, multi-GPU configurations, and GPUDirect P2P for ML training and inference workloads

VFIO passthrough gives the guest VM direct access to the GPU hardware, achieving near-native performance for CUDA and ML workloads.

## Architecture of a microVM

Each Cloud Hypervisor microVM runs as a separate `cloud-hypervisor` child process managed by the forge:

```
    Forge process (manages all VMs on this node via REST API)
    ┌──────────────────────────────────────────────┐
    │                                              │
    │  cloud-hypervisor process (one per VM)        │
    │  ┌────────────────────────────────────────┐  │
    │  │                                        │  │
    │  │  VMM thread        ← device emulation  │  │
    │  │  (virtio-net,        and machine model │  │
    │  │   virtio-block,                        │  │
    │  │   virtio-vsock,                        │  │
    │  │   virtio-fs)                           │  │
    │  │                                        │  │
    │  │  vCPU thread(s)    ← one per vCPU,     │  │
    │  │  (KVM_RUN loop)      runs guest code   │  │
    │  │                                        │  │
    │  │  REST API           ← per-VM HTTP API  │  │
    │  │  (Unix socket)       for lifecycle mgmt│  │
    │  │                                        │  │
    │  └────────────────────────────────────────┘  │
    │                                              │
    │  Forge manages VMs via Cloud Hypervisor's    │
    │  REST API — each VM is an independent        │
    │  process that survives forge restarts.        │
    │                                              │
    └──────────────────────────────────────────────┘
```

A microVM is composed of four elements provided at creation time:

- **Kernel** — an uncompressed Linux kernel (`vmlinux`). Shared read-only across all VMs on the node.
- **Root filesystem** — an ext4 disk image containing the OS (Ubuntu, Alpine, etc.). Can be shared (read-only) or per-VM (copy-on-write).
- **Block devices** — additional storage volumes (ZeroFS NBD devices). See [storage.md](storage.md).
- **Network interfaces** — Linux TAP devices connecting the VM to the overlay network.

## Boot sequence

When the forge creates a VM, this is what happens:

```
    Forge receives POST /compute/vms
         │
         ▼
    1. Create TAP device (networking)
         │
    2. Create/attach NBD volume (storage, via ZeroFS)
         │
    3. Spawn cloud-hypervisor process with VM config:
         ├── --kernel           → vmlinux path
         ├── --disk             → root filesystem + data volume (ZeroFS NBD)
         ├── --net              → TAP device configuration
         ├── --cpus             → vCPU count
         ├── --memory           → memory size
         ├── --api-socket       → Unix socket for REST API
         └── (process starts, VM boots)
         │
    4. VM boots in ~200 ms
         │
    5. Guest init runs (cloud-init, agent, or application)

    Total time from API call to running VM: ~300-600 ms
```

Each VM is a separate `cloud-hypervisor` process. The forge manages VM lifecycle via the per-VM REST API on a Unix socket. VMs survive forge restarts — the forge reconnects to existing cloud-hypervisor processes on startup.

## Networking

Each VM gets one or more **TAP devices** on the host, connecting it to the overlay network.

```
    ┌──────────────┐
    │     VM       │
    │              │
    │  eth0        │  ← virtio-net device (guest side)
    └──────┬───────┘
           │
    ┌──────┴───────┐
    │   tap-vm1    │  ← TAP device (host side)
    └──────┬───────┘
           │
    ┌──────┴───────┐
    │  br-vpc-xyz  │  ← Linux bridge (one per VPC subnet on this node)
    └──────┬───────┘
           │
    ┌──────┴───────┐
    │   VXLAN      │  ← overlay encapsulation for cross-node traffic
    └──────┬───────┘
           │
    ┌──────┴───────┐
    │   syfrah0    │  ← fabric (WireGuard)
    └──────────────┘
```

The VM sees a standard network interface (`eth0`). The guest configures it with an IP from its VPC subnet. All the overlay and fabric encapsulation is invisible to the guest.

### Rate limiting

Rate limiting is applied per-VM on both network and block devices:

- **Network**: bandwidth (bytes/sec) and packet rate (ops/sec), configurable for ingress and egress via tc/nftables on the host
- **Block I/O**: bandwidth (bytes/sec) and IOPS (ops/sec) via cgroups v2

CPU limiting is handled by **cgroups v2** configured by the forge.

## Storage integration

VMs use two types of block devices:

### Root filesystem (rootfs)

A read-only (or copy-on-write) ext4 image containing the base OS. The platform provides a catalog of images:

| Image | Contents | Size |
|---|---|---|
| `ubuntu-24.04` | Ubuntu 24.04 minimal | ~500 MB |
| `alpine-3.20` | Alpine Linux | ~50 MB |
| `debian-12` | Debian 12 minimal | ~300 MB |

Kernels are shared across all VMs. A single `vmlinux` binary on disk is referenced by every Cloud Hypervisor instance — read-only, no duplication.

### Data volumes (via ZeroFS)

Persistent storage backed by S3. See [storage.md](storage.md) for the full storage design.

Cloud Hypervisor's `virtio-block` connects directly to ZeroFS's NBD devices — standard Linux block device semantics, no custom integration:

```
    Guest                 Cloud Hypervisor         Host                  Durable
    ─────                 ────────────────         ────                  ───────

    /dev/vda ──────► virtio-block ──────► rootfs.ext4            Local image file
    (root fs)             drive:rootfs    (read-only or CoW)

    /dev/vdb ──────► virtio-block ──────► /dev/nbd0 ──────► ZeroFS ──────► S3
    (data vol)            drive:data      (NBD device)       ├─ SSD cache
                                                             ├─ memory cache
                                                             └─ LZ4 + XChaCha20
```

The write path through the full stack:

```
    1. VM writes to /dev/vdb
    2. Cloud Hypervisor virtio-block passes write to /dev/nbd0
    3. ZeroFS buffers in memory (microseconds)
    4. On fsync: ZeroFS WAL flush to S3 (~10-50ms)
    5. Background: ZeroFS compacts and flushes SST chunks to S3
```

The read path:

```
    1. VM reads from /dev/vdb
    2. Cloud Hypervisor virtio-block reads from /dev/nbd0
    3. ZeroFS checks memory cache → hit? return (microseconds)
    4. ZeroFS checks SSD cache    → hit? return (~100μs)
    5. ZeroFS fetches from S3     → miss? fetch, cache, return (~10-100ms)
```

For most workloads, the cache absorbs >95% of I/O. The S3 latency only matters for cold reads (first access after migration, or data not recently used).

### Performance considerations

| Workload | Bottleneck | Expected performance |
|---|---|---|
| Web server, app server | Mostly reads, fits in cache | Near-native SSD speed |
| PostgreSQL (typical OLTP) | fsync on commit → WAL flush | ~53K TPS with warm cache |
| Large sequential reads (analytics) | Cold data from S3 | Limited by S3 throughput |
| Boot + init | Rootfs is local, no S3 | ~200ms boot, instant rootfs reads |

Per-drive rate limiting (bandwidth + IOPS via cgroups v2) is applied **before** the NBD device, so it caps the VM's I/O regardless of whether the data comes from cache or S3.

## Security

Cloud Hypervisor's security model is layered:

### Layer 1 — Hardware isolation (KVM)

Each VM runs in its own KVM virtual machine. The guest kernel has no direct access to host memory, devices, or other VMs. This is the same isolation used by all major cloud providers.

### Layer 2 — Process-level isolation (cgroups + namespaces)

The forge configures OS-level isolation for each VM:

- **cgroups v2**: CPU, memory, and I/O limits per VM
- **namespaces**: mount, PID, network isolation where applicable
- **seccomp**: syscall filtering to restrict what the cloud-hypervisor process can do

Each VM runs as a separate `cloud-hypervisor` process with its own cgroup and namespace configuration. Process isolation is enforced by the OS, not by a jailer binary.

### Layer 3 — Seccomp (syscall filtering)

A strict allowlist of system calls is enforced on each cloud-hypervisor process. Any syscall not on the list kills the process immediately. This limits what can happen even after a KVM escape.

### Layer 4 — Minimal attack surface

Cloud Hypervisor emulates only virtio devices. No legacy PCI devices, no USB, no full ACPI emulation. Every device that doesn't exist is attack surface that doesn't exist.

## VM lifecycle

| Operation | How | Time |
|---|---|---|
| **Create** | Forge spawns cloud-hypervisor process via REST API | ~100 ms |
| **Start** | Cloud Hypervisor REST API `PUT /vm.boot` | ~200 ms |
| **Stop** | Graceful shutdown signal via REST API or force stop | Instant |
| **Reboot** | Stop + Start via REST API | ~400 ms |
| **Delete** | Stop VM, cleanup TAP device, release NBD volume | Instant |

### Zero-downtime forge updates

Because each VM is a separate `cloud-hypervisor` process, VMs survive forge restarts. When the forge is updated:

1. Forge stops gracefully
2. New forge binary starts
3. Forge discovers existing cloud-hypervisor processes
4. Forge reconnects to each VM's REST API (Unix socket)
5. Full management restored — no VM downtime

This is critical for platform updates: the forge can be restarted, upgraded, or recovered without affecting running VMs.

### VM migration between nodes

Cloud Hypervisor supports live migration, but Syfrah's initial implementation uses a simpler stop-move-start approach that leverages the storage design: since data volumes are backed by S3 (via ZeroFS), migration requires no data copy.

```
    Node A                                  Node B
    ──────                                  ──────

    1. Stop VM
       ├── cloud-hypervisor process stopped
       └── ZeroFS flushes cache to S3
                                            2. Attach volume
                                               └── ZeroFS connects NBD
                                                   to same S3 data

                                            3. Start VM
                                               ├── cloud-hypervisor boots (~200ms)
                                               └── Cache warms up gradually
                                                   (first reads hit S3,
                                                    then cached locally)

    Downtime: ~5-30 seconds (flush + boot)
    Data copied: zero (S3 is the source of truth)
```

This is possible because **compute state and storage state are separated**:
- Cloud Hypervisor manages compute (CPU, memory, devices) — ephemeral, recreated on boot
- ZeroFS manages storage (volumes) — durable in S3, accessible from any node

The only cost of migration is **cache warmup** on the new node. Active working set data loads progressively from S3 into the local SSD cache over the first minutes of operation.

Live migration via Cloud Hypervisor's built-in support is a future enhancement that will reduce migration downtime to near-zero.

## Guest-host communication (vsock)

Cloud Hypervisor provides `virtio-vsock` for communication between the VM and the host without using the network:

```
    Host                              Guest
    ┌──────────────────┐              ┌──────────────┐
    │ Unix socket      │ ◄── vsock ──►│ AF_VSOCK     │
    │ /tmp/v.sock      │              │ port 5000    │
    └──────────────────┘              └──────────────┘
```

Use cases:
- **Metadata service**: deliver instance identity, configuration, secrets to the guest
- **Agent communication**: the forge communicates with an in-VM agent for provisioning
- **Metrics/logs**: the guest pushes metrics and logs to the host without network traffic

Vsock avoids the complexity of setting up a metadata HTTP endpoint on a link-local address (like AWS's 169.254.169.254). It's a direct, low-latency channel.

## Limitations

| Limitation | Impact on Syfrah | Mitigation |
|---|---|---|
| **No Windows guests (v1)** | Linux-only VMs initially | Target audience runs Linux workloads; UEFI support planned |
| **Higher per-VM overhead than Firecracker** | ~13 MB vs ~5 MB per VM | Acceptable trade-off for REST API, GPU support, and VM survival |
| **No nested virtualization** | Cannot run VMs inside VMs | Not needed for target use cases |

Cloud Hypervisor's feature set covers the primary needs: GPU passthrough (VFIO), virtio-fs, virtio-vsock, live migration support, and VMs that survive forge restarts. The limitations are minor compared to the operational benefits.

## Relationship to other concepts

```
    ┌──────────────────────────────────────────────────────┐
    │                                                      │
    │   Cloud Products  ◄── products.md      │
    │   Products create VMs via the forge                  │
    │                                                      │
    ├──────────────────────────────────────────────────────┤
    │                                                      │
    │   Compute         ◄── this document                  │
    │   Cloud Hypervisor microVMs on each node             │
    │                                                      │
    ├──────────────────────────────────────────────────────┤
    │                                                      │
    │   Storage         ◄── storage.md       │
    │   ZeroFS NBD volumes attached to VMs                 │
    │                                                      │
    ├──────────────────────────────────────────────────────┤
    │                                                      │
    │   Forge           ◄── forge.md         │
    │   Manages Cloud Hypervisor VM lifecycle on each node │
    │                                                      │
    ├──────────────────────────────────────────────────────┤
    │                                                      │
    │   Fabric          ◄── fabric.md        │
    │   WireGuard mesh carrying overlay traffic            │
    │                                                      │
    └──────────────────────────────────────────────────────┘
```
