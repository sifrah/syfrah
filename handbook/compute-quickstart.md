# Compute Quick Start

## Prerequisites

- syfrah installed and mesh initialized (`syfrah fabric init --name my-mesh`)

## Check compute readiness

```bash
syfrah compute status
```

If you see `Runtime: vm (Cloud Hypervisor)` — you're on bare-metal with KVM.
If you see `Runtime: container (gVisor)` — you're on a VPS or VM without KVM.
Both work identically.

## Create your first VM

```bash
# Pull an image
syfrah compute image pull alpine-3.20

# Create a VM (or container, depending on runtime)
syfrah compute vm create --name my-vm --image alpine-3.20 --vcpus 2 --memory 2048

# Check it's running
syfrah compute vm list

# Get details
syfrah compute vm get my-vm

# Stop it
syfrah compute vm stop my-vm

# Delete it
syfrah compute vm delete my-vm --yes
```

## With SSH access (Ubuntu/Debian images)

```bash
syfrah compute vm create --name web-1 --image ubuntu-24.04 \
    --vcpus 2 --memory 2048 --ssh-key ~/.ssh/id_ed25519.pub

# SSH in after ~10 seconds
ssh ubuntu@<mesh-ipv6>
```
