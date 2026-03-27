# Compute

## What is the compute layer?

The compute layer is the **Cloud Hypervisor driver**. It owns the full lifecycle of a VM: create, start, stop, resize, delete, reconnect. Forge tells it what to do, compute knows how.

Compute is not an orchestrator, not a scheduler, not a reconciliation loop. It is the specialist that interfaces with [Cloud Hypervisor](https://github.com/cloud-hypervisor/cloud-hypervisor) and manages VM processes on a single node.

### Responsibility boundaries

```
    Forge (reconciliation)
      |
      |-- "I need this VM running"  --> Compute.create(spec) + Compute.boot(id)
      |-- "This VM should stop"     --> Compute.shutdown_graceful(id)
      |-- "Delete this VM"          --> Compute.delete(id)
      |-- "What VMs are running?"   --> Compute.list() / Compute.info(id)
      |
    Compute (Cloud Hypervisor driver)
      |
      |-- spawns cloud-hypervisor processes
      |-- talks to per-VM REST API (Unix socket)
      |-- monitors process health
      |-- reconnects after daemon restart
      |-- manages cgroups and isolation
```

### What compute does NOT do

- **Scheduling** — control plane decides which node runs which VM
- **Reconciliation** — forge compares desired state (Raft) with local reality
- **Networking** — overlay creates TAP devices, bridges, VXLAN
- **Storage** — storage layer connects ZeroFS/NBD volumes
- **External API** — forge exposes the HTTP endpoints
- **Distributed state** — Raft owns the desired state

---

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
| **GPU passthrough (VFIO)** | Yes | Yes | No | No |
| **Live migration** | Yes | Yes | No | No |
| **VMs survive host restart** | Yes (separate process) | Yes | No | No (embedded) |
| **License** | Apache 2.0 | GPL 2.0 | Apache 2.0 | Apache 2.0 |

The deciding factors for Cloud Hypervisor:

1. **Separate process per VM** — VMs survive forge/syfrah restarts. Critical for zero-downtime updates. libkrun embeds in the host process: if forge restarts, all VMs die.
2. **VFIO GPU passthrough** — NVIDIA CUDA, multi-GPU, GPUDirect P2P for ML workloads. Firecracker and libkrun do not support this.
3. **REST API per VM** — clean lifecycle management without embedding VMM code. Each VM is independently manageable.
4. **Rust** — same language as the rest of Syfrah. Smaller attack surface than QEMU.

The trade-off is ~13 MB overhead per VM (vs ~5 MB for Firecracker). Acceptable for the operational benefits.

---

## Types

### Shared types (exposed to forge and control plane)

**VmId** — unique identifier for a VM.

**VmSpec** — desired state of a VM:
- vcpus (u32)
- memory_mb (u32)
- image (String — e.g., "ubuntu-24.04")
- kernel (optional, defaults to shared vmlinux)
- network (TAP device config, provided by overlay)
- volumes (list of block device paths, provided by storage)
- gpu (GpuMode)

**VmPhase** — current phase in the VM lifecycle (see state machine below).

**VmStatus** — external view of a VM, exposed to forge and other layers:
- vm_id
- phase
- vcpus, memory_mb
- created_at
- uptime (if running)

**VmEvent** — observable events emitted to forge:
- Created, Booted, Stopped, Crashed, Deleted
- ReconnectSucceeded, ReconnectFailed
- Resized, DeviceAttached, DeviceDetached

**GpuMode**:
- `None` — no GPU
- `Passthrough { bdf: String }` — VFIO passthrough of a PCI device (e.g., "0000:01:00.0")

No `Shared` mode. virtio-gpu (shared rendering) is a future consideration. Until there is a concrete technical contract, it does not exist in the API.

### Internal type (not exposed)

**VmRuntimeState** — compute-internal state, never leaked to forge or control plane:
- vm_id
- pid (OS process ID of the cloud-hypervisor process)
- socket_path (`/run/syfrah/vms/{id}/api.sock`)
- cgroup_path
- ch_binary_path
- ch_binary_version
- launched_at
- last_ping_at
- last_error
- current_phase
- reconnect_source (FreshSpawn | Recovered)

The separation is strict:
- **VmSpec** = what the user/forge asked for (desired)
- **VmStatus** = what compute tells the outside world (observable)
- **VmRuntimeState** = what compute uses internally (implementation detail)

---

## State machine

```
Pending --> Provisioning --> Starting --> Running --> Stopping --> Stopped --> Deleting --> Deleted
                |               |           |           |                        |
                v               v           v           v                        v
              Failed          Failed      Failed      Failed                   Failed

Stopped --> Starting  (restart)
Stopped --> Deleting
Failed  --> Deleting
```

### Allowed transitions

| From | To | Trigger |
|---|---|---|
| Pending | Provisioning | Forge requests create |
| Provisioning | Starting | Process spawned, cgroup configured, runtime dir created |
| Provisioning | Failed | Preflight check fails, spawn fails |
| Starting | Running | `ping()` on socket succeeds — VMM is operational |
| Starting | Failed | Ping timeout (10s), process crashed before API ready |
| Running | Stopping | Forge requests shutdown |
| Running | Failed | Process crashed, ping health check fails |
| Stopping | Stopped | Process terminated cleanly |
| Stopping | Failed | Kill chain exhausted (all 4 levels), process still alive |
| Stopped | Starting | Forge requests restart |
| Stopped | Deleting | Forge requests delete |
| Failed | Deleting | Forge requests delete |
| Deleting | Deleted | Cleanup complete (socket, pid, cgroup, runtime dir) |

**Any transition not listed is an error.** The state machine is enforced in code. Invalid transitions return a `TransitionError`.

**Important:** `Running` means the VMM process is responding to API calls. It does **not** mean the guest OS has finished booting. Guest-level readiness (e.g., via a vsock agent) is a future enhancement and would be a separate state (`GuestReady`), not a replacement for `Running`.

---

## Cloud Hypervisor client

HTTP client that talks to Cloud Hypervisor's REST API via Unix socket. One socket per VM at `/run/syfrah/vms/{id}/api.sock`.

### Endpoints by stability level

**GA (stable, production-ready from day one):**

| Compute method | CH endpoint | Notes |
|---|---|---|
| `create(config)` | `PUT /vm.create` | Create VM definition |
| `boot(id)` | `PUT /vm.boot` | Boot the VM |
| `info(id)` | `GET /vm.info` | VM details and status |
| `shutdown_graceful(id)` | `PUT /vm.shutdown` | ACPI shutdown signal to guest |
| `shutdown_force(id)` | `PUT /vm.power-button` | Power button (guest-level force) |
| `delete(id)` | `PUT /vm.delete` | Delete VM definition from CH |
| `ping(id)` | `GET /vmm.ping` | Health check |

**Beta (functional, may change):**

| Compute method | CH endpoint | Notes |
|---|---|---|
| `reboot(id)` | `PUT /vm.reboot` | Reboot guest |
| `pause(id)` | `PUT /vm.pause` | Pause VM execution |
| `resume(id)` | `PUT /vm.resume` | Resume paused VM |
| `resize(id, vcpu, mem)` | `PUT /vm.resize` | Hot-resize CPU/memory |
| `counters(id)` | `GET /vm.counters` | Performance counters |

**Experimental (may not work in all configurations):**

| Compute method | CH endpoint | Notes |
|---|---|---|
| `attach_disk(id, config)` | `PUT /vm.add-disk` | Hot-add block device |
| `attach_net(id, config)` | `PUT /vm.add-net` | Hot-add network device |
| `attach_device(id, path)` | `PUT /vm.add-device` | VFIO device (GPU passthrough) |
| `detach_device(id, dev)` | `PUT /vm.remove-device` | Remove device |

### Shutdown levels

Four distinct levels, from gentle to brutal:

1. **`shutdown_graceful(id)`** — ACPI shutdown signal via CH API. The guest OS handles it (systemd shutdown, etc.). Timeout: 30s.
2. **`shutdown_force(id)`** — Power button via CH API. Guest-level force power off. Timeout: 10s.
3. **`terminate_vmm(id)`** — SIGTERM on the cloud-hypervisor PID. VMM-level termination. Timeout: 5s.
4. **`kill_vmm(id)`** — SIGKILL on the PID. Unconditional process death. No timeout.

No ambiguity between guest, VMM, and OS-level operations.

### Idempotence

Lifecycle operations are idempotent where possible:

| Operation | On already-done state | Behavior |
|---|---|---|
| `shutdown_graceful` | VM already stopped | Success (no-op) |
| `delete` | VM already deleted / absent | Success (already deleted) |
| `boot` | VM already running | No-op |
| `create` | VM already exists | Error (`VmAlreadyExists`) |
| `attach_disk` | Disk already attached | Error (`DiskAlreadyAttached`) |

Forge does not need defensive logic around idempotent operations. Compute handles it.

---

## Process manager

The core of the compute layer. Manages `cloud-hypervisor` child processes.

### Runtime directory

Each VM gets an isolated runtime directory:

```
/run/syfrah/vms/{id}/
    api.sock         Cloud Hypervisor REST API socket
    pid              PID of the cloud-hypervisor process
    meta.json        Metadata for reconnect (see below)
    ch-version       Version of the CH binary used to spawn this VM
    stdout.log       CH process stdout/stderr (or journal pointer)
```

**meta.json** contains everything needed to reconnect:

```json
{
  "vm_id": "vm-web-1",
  "created_at": "2026-03-27T14:00:00Z",
  "socket_path": "/run/syfrah/vms/vm-web-1/api.sock",
  "pid": 12345,
  "ch_binary": "/usr/local/lib/syfrah/cloud-hypervisor",
  "ch_version": "v43.0",
  "spec_hash": "sha256:abc123..."
}
```

### Spawn

When forge requests a VM creation:

1. Run preflight validation (see below)
2. Create `/run/syfrah/vms/{id}/`
3. Configure cgroup v2 (`cpu.max`, `memory.max`)
4. Exec `cloud-hypervisor --api-socket /run/syfrah/vms/{id}/api.sock`
5. Write `pid`, `meta.json`, `ch-version`
6. Poll `ping()` until the API is ready (timeout 10s)
7. Call `create(config)` then `boot()`
8. Transition: `Pending → Provisioning → Starting → Running`

### Monitor

Continuous health checking of active VMs:

- Periodic `kill(pid, 0)` to verify the process is alive
- Periodic `ping()` on the socket to verify the API responds
- If PID is dead → transition to `Failed`, emit `Crashed` event
- If ping times out → transition to `Failed`, emit `Crashed` event

### Reconnect (after daemon restart)

When syfrah/forge restarts, compute reconstructs its state from the runtime directories:

1. Scan `/run/syfrah/vms/*/meta.json`
2. For each: read `meta.json`, verify PID is alive (`kill(pid, 0)`), ping socket
3. All three match → reconstruct `VmRuntimeState` with `reconnect_source: Recovered`
4. PID dead but runtime dir exists → cleanup, mark `Failed`, emit `ReconnectFailed`
5. meta.json missing or corrupt → cleanup, mark orphaned, emit `VmOrphanCleaned`
6. Log: "Reconnected to 5 VMs, 1 failed, 1 orphaned and cleaned"

**meta.json is the source of truth for runtime intention.** The actual truth at reconnect time is meta.json + PID alive + socket responding. All three must agree.

**Orphaned runtime dirs** (runtime dir present, PID dead, socket dead, no reconcilable state) are cleaned up immediately. Logs provide forensic information; there is no "keep for debug" mode.

### Kill chain

When forge requests VM deletion or the kill chain is triggered:

1. `shutdown_graceful(id)` — ACPI via API (30s timeout)
2. `shutdown_force(id)` — power button via API (10s timeout)
3. `terminate_vmm(id)` — SIGTERM on PID (5s timeout)
4. `kill_vmm(id)` — SIGKILL on PID
5. Cleanup: remove runtime dir, destroy cgroup

### Delete contract

`delete` on the compute side means:
- Stop the cloud-hypervisor process (via kill chain if needed)
- Remove all runtime artifacts: socket, pid file, meta.json, cgroup, stdout.log
- Remove the runtime directory `/run/syfrah/vms/{id}/`

`delete` does **not** touch external assets:
- TAP devices (managed by overlay)
- Volumes/NBD (managed by storage)
- Images/kernels (shared, managed by the platform)

Forge orchestrates the full cleanup across layers. Compute only cleans up its own artifacts.

### Concurrency

- One `tokio::sync::Mutex` per VM, stored in a `HashMap<VmId, Arc<Mutex<VmRuntimeState>>>`
- Every lifecycle operation acquires the VM's lock before proceeding
- Operations on the same VM are serialized
- Operations on different VMs run in parallel

This is a conscious MVP choice. Some operations are long (shutdown_graceful: up to 30s). During that time, concurrent operations on the same VM will block. If this becomes a problem, a future iteration can move to a command-in-progress model with observable state. For now, the simplicity of a mutex per VM is worth the trade-off.

---

## Preflight validator

Before every spawn, compute validates all preconditions in a single pass. It does not fail-fast — it collects all errors and returns them together.

| Check | How | Error |
|---|---|---|
| CH binary exists and is executable | `fs::metadata` + permission check | `ChBinaryNotFound` |
| KVM available | `/dev/kvm` exists and is accessible | `KvmNotAvailable` |
| Kernel exists | Path to vmlinux | `KernelNotFound` |
| Disk image exists | Path to rootfs | `ImageNotFound` |
| TAP device exists (if requested) | Check network interface | `TapDeviceNotFound` |
| VFIO device bound (if GPU) | `/sys/bus/pci/devices/{bdf}/driver` = `vfio-pci` | `VfioNotBound` |
| cgroup v2 available | `/sys/fs/cgroup/cgroup.controllers` exists | `CgroupV2NotAvailable` |
| Socket path free | No existing file at socket path | `SocketPathOccupied` |
| Sufficient capacity | Free RAM, available CPUs | `InsufficientResources` |

The preflight returns `Result<ValidatedSpec, Vec<PreflightError>>` — all failures at once, not one at a time. This is what operators expect.

---

## Config pipeline

Translating a Syfrah `VmSpec` into a Cloud Hypervisor `VmConfig` (JSON) is a three-step pipeline:

```
VmSpec --> validate(spec) --> ValidatedSpec
       --> resolve(validated) --> ResolvedSpec
       --> map(resolved) --> VmConfig (CH JSON)
```

**validate** — logical coherence checks:
- vcpus > 0
- memory_mb >= 128
- image name is known
- GPU mode is valid (if Passthrough, bdf is well-formed)
- No contradictory settings

**resolve** — names to paths:
- `"ubuntu-24.04"` → `/opt/syfrah/images/ubuntu-24.04.raw`
- kernel → `/opt/syfrah/vmlinux` (default shared kernel)
- GPU bdf → `/sys/bus/pci/devices/{bdf}/`

**map** — produce the CH JSON:

```
VmSpec (Syfrah)                    VmConfig (Cloud Hypervisor)
───────────────                    ─────────────────────────────
id: "vm-web-1"                     api-socket: /run/syfrah/vms/vm-web-1/api.sock
vcpus: 4                           cpus.boot_vcpus: 4
memory_mb: 4096                    memory.size: 4294967296
image: "ubuntu-24.04"              disks[0].path: /opt/syfrah/images/ubuntu-24.04.raw
kernel: (shared)                   payload.kernel: /opt/syfrah/vmlinux
network: {tap: "tap-vm-web-1"}     net[0].tap: "tap-vm-web-1"
volumes: [{path: "/dev/nbd0"}]     disks[1].path: "/dev/nbd0"
gpu: Passthrough("0000:01:00.0")   devices[0].path: /sys/bus/pci/devices/0000:01:00.0/
```

Three steps, three error types, one module. Functions, not structs.

---

## Error taxonomy

Errors are typed so forge can distinguish user errors from infra failures:

| Error type | Meaning | Example |
|---|---|---|
| `PreflightError` | Precondition not met before spawn | KVM unavailable, image missing |
| `ConfigError` | Invalid or unresolvable VM spec | Unknown image name, invalid vcpu count |
| `ClientError` | Cloud Hypervisor API call failed | Socket unreachable, unexpected HTTP status |
| `ProcessError` | OS-level process management failure | Spawn failed, PID not found, cgroup error |
| `TransitionError` | Invalid state machine transition | Boot on a deleted VM |
| `ConcurrencyError` | Operation blocked by another in-flight op | Lock timeout (if implemented) |

All wrapped in a `ComputeError` enum. Forge pattern-matches on the variant to decide:
- User error → return to caller
- Infra error → retry or alert
- Transient error → retry with backoff
- Bug → log and escalate

---

## Events

Two levels of events with different audiences:

### Internal events (process manager, not persisted)

For debug and tracing within compute:
- `Spawned`, `ApiReady`, `PingTimeout`, `ProcessExited`
- `CgroupCreated`, `CgroupDestroyed`
- `SocketCreated`, `SocketRemoved`

### External events (exposed to forge)

Transmitted via a `tokio::sync::broadcast` channel that forge consumes:
- `Created`, `Booted`, `Stopped`, `Crashed`, `Deleted`
- `ReconnectSucceeded`, `ReconnectFailed`, `VmOrphanCleaned`
- `Resized`, `DeviceAttached`, `DeviceDetached`

**Delivery guarantee:** best-effort, real-time. Compute does not persist events and does not guarantee delivery to slow consumers. Forge must treat this stream as informational. The source of truth for VM state is always `info()` / `status()`, never the event stream alone.

---

## Embedded binary and versioning

Cloud Hypervisor is **bundled with Syfrah releases**. Operators do not install it separately.

### Packaging

```
syfrah-v1.0.0-x86_64-linux-musl.tar.gz
    syfrah                                  Syfrah binary
    cloud-hypervisor                        Cloud Hypervisor binary (same target)
    install.sh                              Installation script
```

`install.sh` places both binaries:

```
/usr/local/bin/syfrah
/usr/local/lib/syfrah/cloud-hypervisor
```

### Version pinning

- `CLOUD_HYPERVISOR_VERSION` file at the repo root pins the CH version (e.g., `v43.0`)
- The release workflow downloads the pre-built CH binary from GitHub releases, verifies SHA256
- Compute checks at startup: `cloud-hypervisor --version` must match the pinned version → warning if mismatch, not a blocking error

### Binary resolution

Compute looks for cloud-hypervisor in order:
1. `/usr/local/lib/syfrah/cloud-hypervisor` (installed by syfrah)
2. `$PATH` (operator override)

### Update behavior

When an operator runs `syfrah update`:

1. Downloads the new tarball (syfrah + cloud-hypervisor)
2. Replaces both binaries on disk
3. Restarts the syfrah daemon
4. Compute reconnects to all existing VMs (they kept running)
5. Existing VMs continue using the **old** CH version (loaded in memory)
6. New VMs use the **new** CH binary from disk
7. Log: "3 VMs running with CH v42.0, current is v43.0"
8. The operator decides when to rolling-restart VMs (`syfrah compute vm restart` one by one or in batches)

**No automatic rolling restart.** This is an operator decision — they choose the maintenance window. Compute only reports the version mismatch.

---

## Persistence model

- **No database** for compute (no redb, no SQLite)
- **No distributed state** (that's Raft's job)
- **Runtime dir** in `/run/syfrah/vms/{id}/` — survives daemon restarts, lost on machine reboot
- `meta.json` = source of truth for reconnect intention
- All "long memory" lives in Raft (desired state) and forge (local redb state)

Compute is stateless in the database sense. Its state is the running CH processes + the runtime directory.

---

## Testing strategy

### A. State machine and types

- Allowed/refused transitions
- Serde roundtrip for all types
- Idempotence logic of operations
- GpuMode serialization

### B. Cloud Hypervisor client

- Mock Unix socket server simulating CH API responses
- Timeout handling, network errors, invalid responses
- Idempotence of API calls (shutdown on stopped VM, etc.)

### C. Process manager

- Fake CH binary (a shell script that creates a socket and responds to ping)
- Spawn, reconnect, kill chain
- Crash detection (kill the fake binary, verify compute detects it)
- Concurrency: two operations on the same VM in parallel (mutex test)
- Orphan cleanup

### D. Config pipeline

- Validation: invalid specs rejected with correct error types
- Resolution: paths resolved correctly, errors on missing assets
- Mapping: JSON output matches CH expected format

### E. E2E with KVM (not in CI, requires bare-metal host)

- Create + boot + info + shutdown + delete
- Reconnect after daemon kill
- Resize (CPU, memory)
- Disk attach/detach
- GPU passthrough (if hardware available)

Tests A-D run in CI without KVM. Test E runs on dedicated infrastructure.

---

## Security

### Layer 1 — Hardware isolation (KVM)

Each VM runs in its own KVM virtual machine. The guest kernel has no direct access to host memory, devices, or other VMs.

### Layer 2 — Process isolation (cgroups v2 + namespaces)

Each cloud-hypervisor process runs with:
- **cgroups v2**: CPU, memory, and I/O limits per VM
- **namespaces**: mount, PID, network isolation where applicable
- **seccomp**: syscall allowlist — any unauthorized syscall kills the process

### Layer 3 — Minimal attack surface

Cloud Hypervisor emulates only virtio devices. No legacy PCI, no USB, no full ACPI. Every device that doesn't exist is attack surface that doesn't exist.

### GPU passthrough security

VFIO passthrough gives the guest direct hardware access via IOMMU. The host kernel enforces DMA isolation via IOMMU groups. All devices in the same IOMMU group must be passed through together.

---

## GPU support

Cloud Hypervisor supports VFIO passthrough for dedicated GPU access:

```
cloud-hypervisor --device path=/sys/bus/pci/devices/0000:01:00.0/
```

For multi-GPU with GPUDirect P2P (NVIDIA Turing, Ampere, Hopper, Lovelace):

```
cloud-hypervisor --device path=/sys/bus/pci/devices/0000:01:00.0/,x_nv_gpudirect_clique=0
```

Compute exposes this via `GpuMode::Passthrough { bdf }`. The config pipeline translates it to the CH `--device` argument.

**Prerequisites for GPU passthrough on a node:**
- IOMMU enabled in BIOS and kernel (`intel_iommu=on` or `amd_iommu=on`)
- GPU unbound from native driver and bound to `vfio-pci`
- All devices in the same IOMMU group must be passed through

These prerequisites are checked by the preflight validator (`VfioNotBound` error if not met).

---

## Networking

Compute does not manage networking directly. It receives TAP device configuration from the overlay layer (via forge) and passes it to Cloud Hypervisor:

```
    VM (guest)
    eth0            <-- virtio-net device
        |
    tap-vm-{id}     <-- TAP device (created by overlay)
        |
    br-vpc-{vpc}    <-- Linux bridge (created by overlay)
        |
    VXLAN           <-- overlay encapsulation
        |
    syfrah0         <-- fabric (WireGuard)
```

### Rate limiting

- **Network**: bandwidth and packet rate via tc/nftables on the TAP device (managed by overlay)
- **Block I/O**: bandwidth and IOPS via cgroups v2 (managed by compute)
- **CPU**: cgroups v2 cpu.max (managed by compute)

---

## Storage integration

Compute receives block device paths from the storage layer (via forge) and attaches them to Cloud Hypervisor:

- **Root filesystem**: read-only ext4 image, passed as `--disk path={rootfs}`
- **Data volumes**: ZeroFS NBD devices, passed as additional `--disk` entries

Compute does not manage volumes, images, or caches. It receives paths and passes them to CH.

---

## VM migration

Initial implementation uses stop-move-start (no live migration):

```
Node A                               Node B
------                               ------
1. Compute.shutdown_graceful(id)
   (ZeroFS flushes to S3)
                                     2. Compute.create(spec) + boot(id)
                                        (ZeroFS connects to same S3 data)
                                        (cache warms gradually)

Downtime: ~5-30 seconds
Data copied: zero (S3 is the source of truth)
```

Live migration via Cloud Hypervisor's built-in `vm.send-migration` / `vm.receive-migration` is a future enhancement.

---

## Limitations

| Limitation | Impact | Mitigation |
|---|---|---|
| No Windows guests (v1) | Linux-only VMs | UEFI support planned |
| ~13 MB overhead per VM | Higher than Firecracker (~5 MB) | Acceptable for REST API, GPU, VM survival |
| No nested virtualization | Cannot run VMs inside VMs | Not needed for target use cases |
| No virtio-gpu shared mode (v1) | GPU = passthrough only | Shared rendering is a future consideration |
| Mutex per VM blocks concurrent ops | Long shutdown can block delete | Future: command-in-progress model |

---

## Relationship to other layers

```
    Forge           "I need VM X running"
        |
    Compute         spawns CH process, manages lifecycle
        |
    Cloud Hypervisor (one process per VM, REST API on Unix socket)
        |
    KVM             hardware isolation
        |
    Dedicated server hardware
```

Compute sits between forge and Cloud Hypervisor. It translates high-level intentions ("create this VM") into low-level CH API calls and process management. It is a driver, not a controller.
