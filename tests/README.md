# Tests

## Overview

Syfrah tests are organized in three levels:

| Level | Location | What it tests | Needs Docker/root |
|---|---|---|---|
| **Unit** | `layers/*/src/` (inline) | Individual functions | No |
| **Integration** | `layers/*/tests/` | Single layer (protocol, peering) | No |
| **E2E** | `tests/e2e/scenarios/` | Multi-node cluster in Docker containers | Yes |

## Running tests

```bash
# Unit + integration tests (fast, no root, no Docker)
just test

# E2E tests (requires Docker)
just e2e

# Run a specific E2E scenario
./tests/e2e/run.sh 01_mesh

# Run all
just ci && just e2e
```

## E2E tests

E2E tests spawn real Docker containers, each running the `syfrah` binary with WireGuard. They test the full CLI workflow: init, join, peering, connectivity.

### How it works

```
tests/e2e/
├── Dockerfile          Docker image (debian + wireguard-tools + syfrah binary)
├── run.sh              Orchestrator: builds image, discovers and runs scenarios
├── lib.sh              Shared functions for all scenarios
└── scenarios/
    ├── 01_mesh_formation.sh
    ├── 02_mesh_connectivity.sh
    ├── 03_node_leave.sh
    ├── 04_daemon_restart.sh
    └── 05_large_mesh.sh
```

**`run.sh`** builds the Docker image (compiles syfrah with musl for static linking), discovers all `scenarios/*.sh` files, and runs them sequentially. Each scenario is independent.

**`lib.sh`** provides shared functions that every scenario sources:

| Function | What it does |
|---|---|
| `start_node <name> <ip>` | Start a container on the test network |
| `init_mesh <container> <ip>` | Run `syfrah fabric init` |
| `start_peering <container>` | Run `syfrah fabric peering start --pin` |
| `join_mesh <container> <target_ip> <own_ip>` | Run `syfrah fabric join` |
| `wait_daemon <container>` | Wait for the control socket (up to 30s) |
| `stop_daemon <container>` | Run `syfrah fabric stop` |
| `leave_mesh <container>` | Run `syfrah fabric leave` |
| `get_mesh_ipv6 <container>` | Extract the mesh IPv6 from status |
| `assert_daemon_running <container>` | Verify daemon is running |
| `assert_peer_count <container> <n>` | Verify peer count |
| `assert_interface_exists <container>` | Verify syfrah0 exists |
| `assert_interface_gone <container>` | Verify syfrah0 is removed |
| `assert_can_ping <from> <ipv6>` | Verify IPv6 mesh connectivity |
| `cleanup` | Remove all containers created by this scenario |
| `summary` | Print pass/fail count, return exit code |

### Scenarios

| Scenario | What it tests |
|---|---|
| `01_mesh_formation` | 3 nodes init + join, all daemons running, all see 2 peers |
| `02_mesh_connectivity` | Full IPv6 ping matrix between all 3 nodes |
| `03_node_leave` | Node leaves, remaining nodes still connected |
| `04_daemon_restart` | Stop daemon, restart from saved state, peers restored |
| `05_large_mesh` | 5 nodes join, all see 4 peers, end-to-end connectivity |

### Adding a scenario

1. Create `tests/e2e/scenarios/XX_name.sh`
2. Source the shared library: `source "$SCRIPT_DIR/../lib.sh"`
3. Use `start_node`, `init_mesh`, `join_mesh`, `assert_*` functions
4. End with `cleanup` and `summary`
5. Done — `run.sh` discovers it automatically

Template:

```bash
#!/usr/bin/env bash
SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── My New Test ──"

create_network

start_node "e2e-mytest-1" "172.20.0.10"
start_node "e2e-mytest-2" "172.20.0.11"

init_mesh "e2e-mytest-1" "172.20.0.10"
start_peering "e2e-mytest-1"
join_mesh "e2e-mytest-2" "172.20.0.10" "172.20.0.11"

sleep 3

assert_peer_count "e2e-mytest-1" 1
assert_can_ping "e2e-mytest-1" "$(get_mesh_ipv6 e2e-mytest-2)"

cleanup
summary
```

### Container naming

Each scenario uses a unique container name prefix (e.g., `e2e-form-*`, `e2e-ping-*`) to avoid conflicts between scenarios. The shared network (`syfrah-e2e`) is reused.

### CI

E2E tests run in GitHub Actions (`.github/workflows/e2e.yml`):
- On push to main when code changes
- On PRs that modify code
- Weekly (Monday 4am UTC)
- Manual trigger via `workflow_dispatch`

The runner has Docker pre-installed. Containers use `--privileged` for WireGuard kernel access.

## Unit and integration tests

Unit tests are inline (`#[cfg(test)]`) in source files. Integration tests are in `layers/*/tests/`.

See [handbook/testing.md](../handbook/testing.md) for the full testing strategy.
