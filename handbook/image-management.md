# Image Management

This document describes how Syfrah manages VM boot assets: kernels, base images, instance disks, and cloud-init provisioning.

## Boot contract

Cloud Hypervisor supports multiple boot modes (direct kernel, firmware/UEFI). Syfrah v1 standardizes on **direct kernel boot** for simplicity and speed:

- A shared Linux kernel (`vmlinux`) boots the VM directly — no UEFI, no GRUB, no bootloader
- A raw disk image provides the root filesystem
- A config-drive disk provides cloud-init metadata for provisioning

Firmware boot (OVMF/UEFI) is a future enhancement for Windows guests and images that require it. The architecture supports it but it is not exposed to operators in v1.

### What Cloud Hypervisor receives

```
cloud-hypervisor \
    --kernel /opt/syfrah/kernels/vmlinux \
    --disk path=/opt/syfrah/instances/{uuid}/rootfs.raw \
           path=/opt/syfrah/instances/{uuid}/cloud-init.img \
    --cpus boot_vcpus=2 \
    --memory size=2147483648 \
    --net tap=tap-{uuid} \
    --api-socket /run/syfrah/vms/{uuid}/api.sock
```

## Three subsystems

Image management is organized into three logical subsystems within the compute layer:

### Image service (`compute::image`)

Manages the catalog and local cache of base images:
- Pull images from the catalog (HTTP)
- Import local images
- Delete unused images
- Validate checksums
- Track image metadata

### Disk service (`compute::disk`)

Manages per-VM writable disks:
- Clone base image to instance disk
- Resize disk after clone
- Generate cloud-init config-drive
- Cleanup on VM deletion

### Boot asset service (`compute::boot`)

Manages kernel and firmware resolution:
- Resolve kernel path (bundled or custom)
- Validate kernel at daemon startup
- Future: firmware resolution for UEFI boot

These are modules within the `syfrah-compute` crate, not separate crates.

---

## Kernel

### Bundled kernel (default)

Syfrah ships a Linux kernel in every release, alongside the syfrah binary and the Cloud Hypervisor binary:

```
syfrah-v1.0.0-x86_64-linux-musl.tar.gz
├── syfrah
├── cloud-hypervisor
├── vmlinux               <-- bundled kernel
└── install.sh
```

`install.sh` places it at `/opt/syfrah/kernels/vmlinux`.

The bundled kernel is:
- Compiled for the same architecture as the release target (amd64 or arm64)
- Built with the minimum drivers required for Cloud Hypervisor VMs
- Tested against the specific Cloud Hypervisor version packaged with Syfrah
- Versioned alongside Syfrah — kernel updates ship with syfrah updates

**Required kernel options** (compiled in, not as modules):
- `CONFIG_VIRTIO_NET` — network
- `CONFIG_VIRTIO_BLK` — block devices
- `CONFIG_VIRTIO_VSOCK` — host-guest communication
- `CONFIG_HW_RANDOM_VIRTIO` — entropy
- `CONFIG_EXT4_FS` — root filesystem
- `CONFIG_CGROUPS` — resource limits
- `CONFIG_VFAT_FS` — cloud-init config-drive
- `CONFIG_ISO9660_FS` — cloud-init config-drive (alternative)

### Custom kernel

Operators can provide their own kernel for specialized use cases:

```toml
# ~/.syfrah/config.toml
[compute.kernel]
mode = "custom"                              # "bundled" (default) or "custom"
path = "/opt/syfrah/kernels/custom-vmlinux"  # required if mode = "custom"
```

**Support policy:**
- `bundled` = officially supported, tested, guaranteed to work
- `custom` = best-effort, operator responsibility. If the VM doesn't boot, it's the kernel.

At daemon startup, compute validates:
- Kernel file exists at the configured path
- File is readable
- Logs the kernel mode and path

### Kernel versioning

The kernel version is pinned in a `SYFRAH_KERNEL_VERSION` file at the repo root (same pattern as `CLOUD_HYPERVISOR_VERSION`). The release workflow downloads or builds the kernel for each target architecture.

---

## Base images

A base image is an immutable, read-only raw disk image containing a bootable Linux OS.

### Storage layout

```
/opt/syfrah/images/
├── ubuntu-24.04.raw       # 500 MB
├── ubuntu-24.10.raw       # 520 MB
├── alpine-3.20.raw        # 50 MB
├── debian-12.raw          # 300 MB
└── images.json            # local metadata cache
```

Images are stored as raw `.raw` files. Cloud Hypervisor does not support qcow2 — raw is the only supported format for virtio-block.

### Image metadata

Each image has associated metadata tracked in `/opt/syfrah/images/images.json`:

```json
{
  "images": [
    {
      "name": "ubuntu-24.04",
      "arch": "amd64",
      "os_family": "linux",
      "variant": "minimal",
      "format": "raw",
      "compression": "gzip",
      "boot_mode": "direct-kernel",
      "sha256": "abc123...",
      "size_mb": 500,
      "min_disk_mb": 2048,
      "cloud_init": true,
      "default_username": "ubuntu",
      "rootfs_fs": "ext4",
      "source_kind": "official"
    }
  ]
}
```

| Field | Type | Description |
|---|---|---|
| name | string | Image identifier used in CLI (`--image ubuntu-24.04`) |
| arch | string | CPU architecture: `amd64` or `arm64` |
| os_family | string | `linux` (v1 only) |
| variant | string | `minimal`, `standard`, `server` |
| format | string | `raw` (v1 only) |
| compression | string | `gzip`, `zstd`, `none` |
| boot_mode | string | `direct-kernel`, `firmware`, `either` |
| sha256 | string | SHA-256 of the uncompressed raw image |
| size_mb | u32 | Uncompressed image size in MB |
| min_disk_mb | u32 | Minimum disk size for a VM using this image |
| cloud_init | bool | Whether the image supports cloud-init |
| default_username | string | Default user (e.g., `ubuntu`, `root`, `alpine`) |
| rootfs_fs | string | Filesystem type (`ext4`) |
| source_kind | string | `official`, `community`, `custom` |

### Image pull policy

```toml
# ~/.syfrah/config.toml
[compute.images]
pull_policy = "IfNotPresent"    # IfNotPresent | Always | Never
catalog_url = "https://images.syfrah.dev/catalog.json"
image_dir = "/opt/syfrah/images"
```

| Policy | Behavior | Use case |
|---|---|---|
| `IfNotPresent` | Pull on first use if not cached locally | Default, production |
| `Always` | Check catalog on every `vm create`, re-pull if hash changed | Development, testing |
| `Never` | Never pull, all images must be pre-imported | Air-gapped, controlled environments |

### Catalog

The image catalog is published as a static JSON file over HTTP:

```
https://images.syfrah.dev/catalog.json
```

The catalog is not served from the Git repository at runtime. The repo contains the source data; the release pipeline publishes it to the HTTP endpoint.

The catalog lists all officially supported images with their metadata, download URLs, and checksums. Syfrah fetches and caches it locally.

### Architecture

All images, kernels, and catalog entries are tagged with `arch` (`amd64` or `arm64`). v1 supports `amd64` only.

Validation at runtime:
- `syfrah compute image pull` only downloads images matching the node's architecture
- `syfrah compute vm create` refuses if the image arch doesn't match the node
- Import validates arch compatibility

---

## Instance disks

When a VM is created, compute clones the base image into a per-instance writable disk.

### Storage layout

```
/opt/syfrah/instances/
└── 550e8400-e29b-41d4-a716-446655440000/
    ├── rootfs.raw          # writable clone of base image
    ├── cloud-init.img      # config-drive disk (NoCloud)
    └── serial.log          # VM console output
```

Instance directories are named by **UUID**, not by VM name. The name is mutable metadata; the UUID is the permanent identity. This prevents path collisions and simplifies rename operations.

### Clone strategy

Syfrah uses `cp --reflink=auto` to clone base images:

- **On btrfs/xfs**: instant, zero-copy (copy-on-write at filesystem level)
- **On ext4**: full byte-for-byte copy (ext4 does not support reflinks)

This is intentionally simple. The documentation calls it "clone with reflink when available, fallback to full copy" — not "CoW", because on ext4 it is not copy-on-write.

Future enhancement: device-mapper thin provisioning for true CoW on any filesystem.

### Disk resize

If the requested VM disk size exceeds the base image's `min_disk_mb`, compute resizes after cloning:

1. `truncate -s {size}M rootfs.raw` — extend the file
2. `resize2fs rootfs.raw` — grow the ext4 filesystem to fill

This happens before VM boot, as part of provisioning.

### Preflight disk capacity

Before cloning, compute checks available disk space on the node:
- Required: image size + overhead (1 GB buffer)
- If insufficient: `PreflightError::InsufficientResources { resource: "disk", ... }`

This prevents partial clones and confusing errors mid-provisioning.

---

## Cloud-init (v1)

Syfrah v1 uses cloud-init via a **NoCloud config-drive disk** — a small FAT32 image attached as a second disk to the VM. No metadata service, no vsock, no HTTP endpoint.

### How it works

At VM creation, compute generates a config-drive:

```
cloud-init.img (FAT32, ~1 MB)
├── meta-data          # instance identity
├── user-data          # users, SSH keys, scripts
└── network-config     # (optional) static IP or DHCP
```

This image is passed to Cloud Hypervisor as an additional disk:

```
--disk path=rootfs.raw path=cloud-init.img
```

On first boot, cloud-init inside the guest detects the NoCloud data source, reads the config-drive, and applies the configuration.

### What gets configured

**meta-data:**
```yaml
instance-id: 550e8400-e29b-41d4-a716-446655440000
local-hostname: web-1
```

**user-data:**
```yaml
#cloud-config
users:
  - name: ubuntu
    ssh_authorized_keys:
      - ssh-ed25519 AAAA... operator@laptop
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
```

**network-config** (optional, cloud-init v2 format):
```yaml
version: 2
ethernets:
  eth0:
    dhcp4: true
```

### CLI usage

```bash
syfrah compute vm create --name web-1 --image ubuntu-24.10 \
    --vcpu 2 --memory 2048 \
    --ssh-key ~/.ssh/id_ed25519.pub
```

The `--ssh-key` flag reads the public key file and injects it into the cloud-init user-data. After boot (~5-10 seconds), the operator can SSH in:

```bash
ssh ubuntu@<vm-mesh-ipv6>
```

### Images without cloud-init

Some images (e.g., Alpine minimal) don't include cloud-init. For these:
- The config-drive is still generated but has no effect
- The operator must configure the VM manually (serial console or custom init scripts)
- `syfrah compute image inspect` shows `cloud_init: false` to flag this

---

## Garbage collection

### Base images

- A base image is protected from deletion while at least one VM instance references it (refcount)
- `syfrah compute image delete` refuses with an error if the image is in use
- Unused images can be deleted freely

### Instance disks

- When a VM is deleted, its instance directory is deleted by default (rootfs, cloud-init, serial.log)
- `syfrah compute vm delete --retain-disk` preserves the instance directory for forensics
- Retained disks appear in `syfrah compute disk list` (future) and can be manually cleaned with `rm -rf`

### No independent volumes in v1

v1 does not support detachable volumes or volume-to-VM reassignment. Each VM has exactly one rootfs disk, created from a base image, destroyed with the VM. Independent volumes (ZeroFS/S3-backed) are a future layer (storage).

---

## CLI

### Image commands

```
syfrah compute image list                        # images available locally
syfrah compute image catalog                     # show remote catalog
syfrah compute image pull ubuntu-24.10           # download from catalog
syfrah compute image import /path --name custom  # import local raw image
syfrah compute image delete alpine-3.20          # delete (if not in use)
syfrah compute image inspect ubuntu-24.10        # show full metadata
```

### VM creation with image

```bash
# Minimal — auto-pulls image if not present
syfrah compute vm create --name web-1 --image ubuntu-24.10 --vcpu 2 --memory 2048

# With SSH key (enables immediate SSH access after boot)
syfrah compute vm create --name web-1 --image ubuntu-24.10 \
    --vcpu 2 --memory 2048 --ssh-key ~/.ssh/id_ed25519.pub

# With custom disk size (default = image min_disk_mb)
syfrah compute vm create --name web-1 --image ubuntu-24.10 \
    --vcpu 2 --memory 2048 --disk-size 20480

# With GPU passthrough
syfrah compute vm create --name gpu-1 --image ubuntu-24.10 \
    --vcpu 8 --memory 32768 --gpu-bdf 0000:01:00.0
```

---

## Full provisioning flow

When `syfrah compute vm create --name web-1 --image ubuntu-24.10 --ssh-key ~/.ssh/id_ed25519.pub` is executed:

```
1. CLI sends CreateVm request via control socket
         |
2. Compute validates spec (vcpus, memory, image name)
         |
3. Image service checks local cache
   |-- image present? → continue
   |-- image absent + pull_policy=IfNotPresent?
   |   → download from catalog URL
   |   → verify SHA-256
   |   → store in /opt/syfrah/images/ubuntu-24.10.raw
   |-- image absent + pull_policy=Never?
       → error: ImageNotFound
         |
4. Disk service clones base image
   |-- cp --reflink=auto .../ubuntu-24.10.raw → .../instances/{uuid}/rootfs.raw
   |-- if disk_size > min_disk_mb: truncate + resize2fs
         |
5. Boot asset service generates cloud-init disk
   |-- create FAT32 image with meta-data + user-data (SSH key)
   |-- write to .../instances/{uuid}/cloud-init.img
         |
6. Preflight validator runs
   |-- CH binary, KVM, kernel, rootfs, cloud-init disk, socket path, capacity
         |
7. Process manager spawns cloud-hypervisor
   |-- --kernel /opt/syfrah/kernels/vmlinux
   |-- --disk rootfs.raw cloud-init.img
   |-- --cpus 2 --memory 2G
   |-- --api-socket /run/syfrah/vms/{uuid}/api.sock
         |
8. VM boots in ~200ms
   |-- cloud-init runs, configures hostname + SSH key
   |-- VM is accessible via SSH within ~5-10s
         |
9. Compute reports: phase=Running, uptime=0s
```

---

## Configuration reference

```toml
# ~/.syfrah/config.toml

[compute.kernel]
mode = "bundled"                             # "bundled" or "custom"
# path = "/opt/syfrah/kernels/custom.vmlinux"  # required if mode = "custom"

[compute.images]
pull_policy = "IfNotPresent"                 # IfNotPresent | Always | Never
catalog_url = "https://images.syfrah.dev/catalog.json"
image_dir = "/opt/syfrah/images"

[compute.instances]
instance_dir = "/opt/syfrah/instances"
retain_disk_on_delete = false                # default: delete disk with VM
```

---

## Limitations (v1)

| Limitation | Impact | Future |
|---|---|---|
| amd64 only | No arm64 VMs | arm64 support planned |
| Direct kernel boot only | No Windows guests, no UEFI | Firmware boot planned |
| No qcow2 | Full copy on ext4, reflink on btrfs/xfs | device-mapper thin provisioning |
| No independent volumes | Disk tied to VM lifecycle | ZeroFS/S3 volumes (storage layer) |
| No image registry API | Catalog is static HTTP JSON | Registry with push/pull API |
| No live resize of rootfs | Must stop VM to resize | Online resize planned |
| cloud-init only | No other provisioning methods | Ignition (CoreOS) planned |
