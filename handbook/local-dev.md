# Local Development Environment

Two Docker containers running syfrah, controlled from the host. Built for iterating on the fabric layer without needing real servers.

## How It Works

- **2 containers** (`syfrah-node1`, `syfrah-node2`) on a shared Docker bridge network (IPv4 + IPv6)
- **WireGuard** runs inside each container via the host's kernel module (interfaces are isolated per network namespace)
- The **syfrah binary** is volume-mounted from `target/debug/syfrah` — no image rebuild needed after code changes
- You control both nodes from the host via `dev.sh` or `docker exec`

## Prerequisites

- Docker with Compose v2
- WireGuard kernel module (`sudo apt install wireguard && sudo modprobe wireguard`)
- Rust toolchain (`cargo build` must work)

## Quick Start

```bash
# 1. Build the binary
./dev/dev.sh build

# 2. Start the two nodes
./dev/dev.sh up

# 3. Init a mesh on node1
./dev/dev.sh n1 syfrah fabric init

# 4. Join from node2
./dev/dev.sh n2 syfrah fabric join 172.28.0.2:9900

# 5. Check peers
./dev/dev.sh n1 syfrah fabric peers
```

## dev.sh Commands

| Command | Description |
|---------|-------------|
| `build` | Compile syfrah (`cargo build`) |
| `up` | Start containers (builds image if needed, loads wireguard module) |
| `down` | Stop and remove containers |
| `restart` | Rebuild binary + restart containers |
| `n1 <cmd...>` | Run a command on node1 |
| `n2 <cmd...>` | Run a command on node2 |
| `exec <node> <cmd...>` | Run a command on any node |
| `status` | Show IP, WireGuard, and syfrah status on both nodes |
| `logs [node]` | Tail container logs |
| `shell <node>` | Open a bash shell on a node |
| `clean` | Stop containers and remove images |

## Typical Workflow

```
edit code -> cargo build -> dev.sh n1/n2 syfrah ... -> repeat
```

The binary is volume-mounted read-only, so each `cargo build` immediately updates what the containers see. If a daemon is running, restart it after rebuilding:

```bash
./dev/dev.sh n1 syfrah fabric stop
./dev/dev.sh n1 syfrah fabric start
```

Or use `./dev/dev.sh restart` to rebuild + recreate containers from scratch.

## Network Layout

```
Host (dev machine)
  |
  +-- docker bridge "mesh"
  |     subnet: 172.28.0.0/16
  |     subnet: fd00:cafe::/64
  |
  +-- syfrah-node1 (172.28.0.2)
  +-- syfrah-node2 (172.28.0.3)
```

Both containers can reach each other on the bridge. WireGuard tunnels are created on top of this network, just like on real servers over the internet.

## Troubleshooting

**"Could not load wireguard module"**
Install wireguard on the host: `sudo apt install wireguard`

**Binary not found**
Run `./dev/dev.sh build` first, or `./dev/dev.sh restart` to build + start.

**Permission denied on WireGuard operations**
Make sure `cap_add: [NET_ADMIN, SYS_MODULE]` is present in `docker-compose.yml`.

**Containers can't reach each other**
Check `docker network inspect dev_mesh` and verify both containers are on the same network.

**IPv6 not working**
Ensure Docker daemon has IPv6 enabled. Check `/etc/docker/daemon.json`:
```json
{
  "ipv6": true,
  "fixed-cidr-v6": "fd00::/80"
}
```

## Running E2E Tests Locally

The CI E2E suite (`tests/e2e/run.sh`) rebuilds a full Docker image on every run (~3 min), which makes local iteration slow. `dev/e2e.sh` solves this by volume-mounting a locally-compiled binary into lightweight containers.

### Quick start

```bash
# 1. Build a static binary (musl, needed for the minimal containers)
cargo build --release --target x86_64-unknown-linux-musl

# 2. Run all E2E scenarios
./dev/e2e.sh

# 3. Run a specific group
./dev/e2e.sh fabric

# 4. Run a single scenario
./dev/e2e.sh 01_fabric
```

Or via just:

```bash
just e2e-local
just e2e-local fabric
just e2e-local 01_fabric
```

### How it works

1. `dev/e2e.sh` builds a lightweight Docker image (debian + wireguard-tools, no Rust compilation)
2. It sets `E2E_BINARY_MOUNT` pointing to the local static binary
3. `tests/e2e/lib.sh` `start_node()` detects the env var and adds a `-v` flag to volume-mount the binary into each container
4. Delegates to the standard `tests/e2e/run.sh` with `SKIP_BUILD=1`

CI is unchanged — when `E2E_BINARY_MOUNT` is not set, `start_node()` uses the baked-in binary as before.

### Iteration workflow

```
edit code -> cargo build --release --target x86_64-unknown-linux-musl -> ./dev/e2e.sh fabric -> repeat
```

The first run builds the base Docker image (~10s). Subsequent runs skip this if the image is cached, so the cycle is: compile + run scenarios.

### Environment variables

| Variable | Default | Description |
|---|---|---|
| `E2E_BINARY` | (auto-detected) | Override path to the syfrah binary |
| `E2E_BINARY_MOUNT` | (set by e2e.sh) | Used by lib.sh to volume-mount the binary |
| `SKIP_BUILD` | (set by e2e.sh) | Tells run.sh to skip the Docker image build |

## Files

```
dev/
  Dockerfile           # Minimal image: debian + wireguard-tools + iproute2
  docker-compose.yml   # 2 nodes, bridge network, volume mount
  dev.sh               # Helper script for the full workflow
  e2e.sh               # Run E2E tests locally with volume-mounted binary
```
