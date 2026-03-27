# Compute

## What is the compute layer?

The compute layer runs tenant workloads. Every VM, every managed database, every load balancer ultimately runs as a **KVM-based microVM via [libkrun](https://github.com/containers/libkrun/)** on a node in the fabric.

libkrun is an open-source library (maintained by the containers community, with Red Hat backing) that allows running workloads inside lightweight microVMs using KVM. Unlike standalone VMMs, libkrun embeds directly into the host process — no separate daemon, no API socket, no jailer. It provides hardware-level isolation via KVM with minimal overhead.

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
    │  Each VM is a separate libkrun microVM instance.       │
    │  Hardware isolation via KVM.                          │
    │  ~5 MB overhead per VM.                               │
    │                                                       │
    │  ┌────────────────────────────────────────────────┐   │
    │  │  Linux kernel + KVM                            │   │
    │  │  Dedicated server hardware                     │   │
    │  └────────────────────────────────────────────────┘   │
    └──────────────────────────────────────────────────────┘
```

## Why libkrun

| | **libkrun** | **QEMU** | **Cloud Hypervisor** | **Firecracker** |
|---|---|---|---|---|
| **Architecture** | Library (embeds in host) | Standalone process | Standalone process | Standalone process + jailer |
| **Overhead per VM** | ~5 MB | ~130 MB | ~13 MB | ~5 MB |
| **Boot time** | <125 ms | Seconds | ~200 ms | <125 ms |
| **Device model** | Minimal (virtio only) | Hundreds (full PC) | Moderate | 5 devices |
| **Attack surface** | Minimal | Large | Moderate | Minimal |
| **Language** | Rust + C | C | Rust | Rust |
| **virtio-fs** | Yes | Yes | Yes | No |
| **GPU passthrough** | No | Yes | Yes | No |
| **Live migration** | No | Yes | Yes | No |
| **License** | Apache 2.0 | GPL 2.0 | Apache 2.0 | Apache 2.0 |

libkrun's trade-off is clear: **maximum density, security, and integration simplicity at the cost of flexibility**. No GPU, no live migration, no Windows guests. For a cloud platform running Linux VMs, databases, and load balancers on dedicated servers, these trade-offs are acceptable.

Key advantages over standalone VMMs: libkrun embeds directly into the forge process — no separate daemon to manage, no API socket to configure, no jailer to set up. The forge calls libkrun as a library to create and manage VMs. This simplifies the compute stack and reduces the operational surface area.

## Architecture of a microVM

Each libkrun microVM runs as threads within the forge process:

```
    Forge process (hosts all VMs on this node)
    ┌──────────────────────────────────────────────┐
    │                                              │
    │  libkrun VM instance (one per VM)            │
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
    │  └────────────────────────────────────────┘  │
    │                                              │
    │  Configuration is done via libkrun's C API   │
    │  directly — no socket, no REST, no IPC.      │
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
    3. Configure libkrun VM via library API:
         ├── krun_set_vm_config()    → vCPU count, memory
         ├── krun_set_root()         → root filesystem
         ├── krun_set_mapped_volumes() → data volume (ZeroFS NBD)
         ├── krun_set_port_map()     → network configuration
         └── krun_start_enter()      → start the VM
         │
    4. VM boots in <125 ms
         │
    5. Guest init runs (cloud-init, agent, or application)

    Total time from API call to running VM: ~200-500 ms
```

No jailer, no Unix socket, no separate process. The forge calls libkrun's C API directly to configure and start VMs. This eliminates the complexity of managing external VMM processes.

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

Kernels are shared across all VMs. A single `vmlinux` binary on disk is referenced by every libkrun instance — read-only, no duplication.

### Data volumes (via ZeroFS)

Persistent storage backed by S3. See [storage.md](storage.md) for the full storage design.

libkrun's `virtio-block` connects directly to ZeroFS's NBD devices — standard Linux block device semantics, no custom integration:

```
    Guest                 libkrun                  Host                  Durable
    ─────                 ───────                  ────                  ───────

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
    2. libkrun virtio-block passes write to /dev/nbd0
    3. ZeroFS buffers in memory (microseconds)
    4. On fsync: ZeroFS WAL flush to S3 (~10-50ms)
    5. Background: ZeroFS compacts and flushes SST chunks to S3
```

The read path:

```
    1. VM reads from /dev/vdb
    2. libkrun virtio-block reads from /dev/nbd0
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
| Boot + init | Rootfs is local, no S3 | <125ms boot, instant rootfs reads |

Per-drive rate limiting (bandwidth + IOPS via cgroups v2) is applied **before** the NBD device, so it caps the VM's I/O regardless of whether the data comes from cache or S3.

## Security

libkrun's security model is layered:

### Layer 1 — Hardware isolation (KVM)

Each VM runs in its own KVM virtual machine. The guest kernel has no direct access to host memory, devices, or other VMs. This is the same isolation used by all major cloud providers.

### Layer 2 — Process-level isolation (cgroups + namespaces)

The forge configures OS-level isolation for each VM:

- **cgroups v2**: CPU, memory, and I/O limits per VM
- **namespaces**: mount, PID, network isolation where applicable
- **seccomp**: syscall filtering to restrict what the host process can do

Since libkrun embeds into the forge process, there is no separate jailer binary. Instead, the forge itself manages the isolation boundaries using standard Linux primitives.

### Layer 3 — Seccomp (syscall filtering)

A strict allowlist of system calls is enforced. Any syscall not on the list kills the process immediately. This limits what can happen even after a KVM escape.

### Layer 4 — Minimal attack surface

libkrun emulates only virtio devices. No PCI bus, no USB, no ACPI, no legacy hardware. Every device that doesn't exist is attack surface that doesn't exist.

## VM lifecycle

| Operation | How | Time |
|---|---|---|
| **Create** | Forge calls libkrun API to configure VM | ~50 ms |
| **Start** | `krun_start_enter()` | <125 ms |
| **Stop** | Graceful shutdown signal or force stop | Instant |
| **Reboot** | Stop + Start | <250 ms |
| **Delete** | Stop VM, cleanup TAP device, release NBD volume | Instant |

### VM migration between nodes

libkrun does not support live migration. Syfrah compensates with the storage design: since data volumes are backed by S3 (via ZeroFS), migration is a stop-move-start sequence with no data copy.

```
    Node A                                  Node B
    ──────                                  ──────

    1. Stop VM
       ├── libkrun VM stopped
       └── ZeroFS flushes cache to S3
                                            2. Attach volume
                                               └── ZeroFS connects NBD
                                                   to same S3 data

                                            3. Start VM
                                               ├── libkrun boots (<125ms)
                                               └── Cache warms up gradually
                                                   (first reads hit S3,
                                                    then cached locally)

    Downtime: ~5-30 seconds (flush + boot)
    Data copied: zero (S3 is the source of truth)
```

This is possible because **compute state and storage state are separated**:
- libkrun manages compute (CPU, memory, devices) — ephemeral, recreated on boot
- ZeroFS manages storage (volumes) — durable in S3, accessible from any node

The only cost of migration is **cache warmup** on the new node. Active working set data loads progressively from S3 into the local SSD cache over the first minutes of operation.

## Guest-host communication (vsock)

libkrun provides `virtio-vsock` for communication between the VM and the host without using the network:

```
    Host                              Guest
    ┌──────────────┐                  ┌──────────────┐
    │ Unix socket  │ ◄── vsock ────► │ AF_VSOCK     │
    │ /tmp/v.sock  │                  │ port 5000    │
    └──────────────┘                  └──────────────┘
```

Use cases:
- **Metadata service**: deliver instance identity, configuration, secrets to the guest
- **Agent communication**: the forge communicates with an in-VM agent for provisioning
- **Metrics/logs**: the guest pushes metrics and logs to the host without network traffic

Vsock avoids the complexity of setting up a metadata HTTP endpoint on a link-local address (like AWS's 169.254.169.254). It's a direct, low-latency channel.

## Limitations

| Limitation | Impact on Syfrah | Mitigation |
|---|---|---|
| **No GPU** | Cannot offer GPU instances | Out of scope for initial product |
| **No live migration** | Cannot move running VMs between nodes | Stop → move volume → start on new node (see storage.md) |
| **No Windows guests** | Linux-only VMs | Target audience runs Linux workloads |
| **Max 32 vCPUs** | Largest VM is 32 vCPU | Sufficient for dedicated server sizes |
| **No memory/CPU hotplug** | Cannot resize a running VM | Stop → reconfigure → start |
| **No nested virtualization** | Cannot run VMs inside VMs | Not needed for target use cases |

The most impactful limitation is **no live migration**. Syfrah mitigates this through the storage design: since volumes are backed by S3 (via ZeroFS), moving a VM between nodes only requires stopping the VM, detaching the volume, reattaching on the new node, and starting. The data does not need to be copied — just the cache needs to warm up.

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
    │   libkrun microVMs on each node                      │
    │                                                      │
    ├──────────────────────────────────────────────────────┤
    │                                                      │
    │   Storage         ◄── storage.md       │
    │   ZeroFS NBD volumes attached to VMs                 │
    │                                                      │
    ├──────────────────────────────────────────────────────┤
    │                                                      │
    │   Forge           ◄── forge.md         │
    │   Manages libkrun VM lifecycle on each node           │
    │                                                      │
    ├──────────────────────────────────────────────────────┤
    │                                                      │
    │   Fabric          ◄── fabric.md        │
    │   WireGuard mesh carrying overlay traffic            │
    │                                                      │
    └──────────────────────────────────────────────────────┘
```
