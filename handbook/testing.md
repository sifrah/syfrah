# Testing

## Principle: tests follow the architecture

Tests are organized by layer, just like the code. Each layer owns its unit and integration tests. End-to-end tests live in `tests/e2e/` and test cross-layer scenarios using Docker containers and shell scripts.

```
    layers/core/src/         <- unit tests (inline)
    layers/core/tests/       <- integration tests
    layers/fabric/src/       <- unit tests (inline)
    layers/fabric/tests/     <- integration tests
    tests/e2e/               <- E2E tests (shell scripts + Docker)
```

## Three levels

### Unit tests

**What:** Test individual functions and types in isolation.

**Where:** Inline in `src/` files, inside `#[cfg(test)]` blocks.

**Requirements:** None. No root, no network, no WireGuard. Pure logic.

**Run:** `cargo test -p syfrah-core` (per-layer) or `cargo test --workspace` (all).

**CI:** Automatic, per-layer, on every push and PR.

```rust
// layers/core/src/addressing.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_address_preserves_prefix() {
        let prefix = generate_mesh_prefix();
        let addr = derive_node_address(&prefix, b"test-key");
        assert_eq!(prefix.segments()[0..3], addr.segments()[0..3]);
    }
}
```

**What belongs here:**
- Type construction and validation
- Serialization roundtrips
- Pure functions (addressing, IPAM math, key derivation)
- Error handling paths
- CLI argument parsing

**What does NOT belong here:**
- Anything requiring network, filesystem, or root
- Anything that spawns processes or creates interfaces

### Integration tests

**What:** Test a layer's modules working together. May require system resources.

**Where:** `layers/{layer}/tests/` directory. Each file is a separate test binary.

**Requirements:** Some tests need root (WireGuard, network interfaces). These are marked `#[ignore]`.

**Run:**
- Normal: `cargo test -p syfrah-fabric` (skips `#[ignore]`)
- With root: `sudo cargo test -p syfrah-fabric -- --ignored`

**CI:** The non-ignored tests run automatically per-layer. Ignored tests are covered by E2E.

**What belongs here:**
- WireGuard interface creation/destruction (root, `#[ignore]`)
- TCP peering protocol (loopback, no root needed)
- State file persistence (tempdir)
- Control socket communication (Unix socket)

**What does NOT belong here:**
- Multi-node scenarios (that's E2E)
- Cross-layer interactions (that's E2E)

### E2E tests

**What:** Full scenarios that test the real `syfrah` binary across multiple Docker containers simulating a multi-node cluster.

**Where:** `tests/e2e/scenarios/`. Each file is a self-contained shell script.

**Infrastructure:** Docker containers with WireGuard kernel support, running the statically-linked `syfrah` binary.

**Run:**
```bash
./tests/e2e/run.sh                  # run all scenarios
./tests/e2e/run.sh fabric           # run only fabric scenarios
./tests/e2e/run.sh 01_fabric        # run scenarios matching "01_fabric"
just e2e                            # shorthand
```

**CI:** Dedicated workflow (`e2e.yml`), on push to main + PRs with code changes + weekly + manual trigger.

```
tests/e2e/
├── Dockerfile          Docker image (debian + wireguard-tools + syfrah binary)
├── run.sh              Orchestrator: builds image, discovers and runs scenarios
├── lib.sh              Shared functions for all scenarios
└── scenarios/
    ├── 01_fabric_mesh_formation.sh
    ├── 02_fabric_mesh_connectivity.sh
    ├── 03_fabric_node_leave.sh
    ├── 04_fabric_daemon_restart.sh
    ├── 05_fabric_large_mesh.sh
    ├── ...                              (65+ scenarios)
    ├── 21_state_cli_list.sh
    ├── 22_state_cli_get.sh
    ├── 23_state_cli_drop.sh
    └── ...
```

**`run.sh`** builds the Docker image (compiles syfrah with musl for static linking), discovers all `scenarios/*.sh` files, and runs them sequentially. Each scenario is independent.

**`lib.sh`** provides shared functions that every scenario sources:

| Function | What it does |
|---|---|
| **Setup & lifecycle** | |
| `create_network` | Create the shared Docker bridge network |
| `remove_network` | Remove the shared Docker bridge network |
| `start_node <name> <ip>` | Start a container on the test network |
| `init_mesh <container> <ip>` | Run `syfrah fabric init` |
| `start_peering <container>` | Run `syfrah fabric peering start --pin` |
| `join_mesh <container> <target_ip> <own_ip>` | Run `syfrah fabric join` |
| `wait_daemon <container>` | Wait for the control socket (up to 30s) |
| `stop_daemon <container>` | Run `syfrah fabric stop` |
| `leave_mesh <container>` | Run `syfrah fabric leave` |
| `get_mesh_ipv6 <container>` | Extract the mesh IPv6 from status |
| `cleanup` | Remove all containers created by this scenario |
| `summary` | Print pass/fail count, return exit code |
| **Assertions — daemon & interface** | |
| `assert_daemon_running <container>` | Verify daemon is running |
| `assert_daemon_stopped <container>` | Verify daemon is not running |
| `assert_peer_count <container> <n>` | Verify peer count |
| `assert_interface_exists <container>` | Verify syfrah0 exists |
| `assert_interface_gone <container>` | Verify syfrah0 is removed |
| **Assertions — connectivity** | |
| `assert_can_ping <from> <ipv6>` | Verify IPv6 mesh connectivity |
| `assert_cannot_ping <from> <ipv6>` | Verify IPv6 mesh is unreachable |
| `block_traffic <container> <target_ip>` | Simulate network partition with iptables |
| `unblock_traffic <container> <target_ip>` | Restore connectivity after partition |
| **Assertions — commands & output** | |
| `assert_command_fails <container> <cmd>` | Verify a command exits non-zero |
| `assert_command_succeeds <container> <cmd>` | Verify a command exits zero |
| `assert_output_contains <container> <cmd> <str>` | Verify command output contains string |
| `assert_output_not_contains <container> <cmd> <str>` | Verify command output does not contain string |
| `assert_output_matches <container> <cmd> <regex>` | Verify command output matches regex |
| `assert_command_suggests <container> <cmd> <suggestion>` | Verify error suggests a corrective action |
| `assert_all_commands_valid <container>` | Verify all CLI commands parse correctly |
| **Assertions — state & consistency** | |
| `get_state_field <container> <field>` | Extract a field from daemon state |
| `assert_state_exists <container>` | Verify state file is present |
| `assert_state_gone <container>` | Verify state file is removed |
| `assert_clean_state <container>` | Verify state file has no corruption |
| `assert_no_duplicate_peers <container>` | Verify no duplicate peers in mesh |
| `assert_consistent_peer_count <containers...>` | Verify all nodes agree on peer count |
| `assert_consistent_region <containers...>` | Verify all nodes agree on region |
| `assert_regions_displayed <container>` | Verify region info appears in output |
| `assert_no_epoch_dates <container>` | Verify no raw epoch timestamps in output |
| `assert_join_retry_works <container> <target>` | Verify join retries on transient failure |
| `wait_for_convergence <containers...>` | Wait until all nodes have consistent state |
| **Logging** | |
| `pass <msg>` | Log a passing check |
| `fail <msg>` | Log a failing check |
| `info <msg>` | Log an informational message |
| `debug <msg>` | Log a debug message |

**What belongs here:**
- Multi-node mesh formation
- Node join, leave, rejoin flows
- Daemon lifecycle (start, stop, restart)
- Mesh connectivity (IPv6 ping between nodes)
- State CLI operations
- Error message validation
- Stress tests (large meshes, concurrent joins)

**What does NOT belong here:**
- Tests that can run in a single layer (move them to integration tests)
- Performance benchmarks (separate concern)

### Adding an E2E scenario

1. Create `tests/e2e/scenarios/XX_name.sh`
2. Source the shared library: `source "$SCRIPT_DIR/../lib.sh"`
3. Use `start_node`, `init_mesh`, `join_mesh`, `assert_*` functions
4. End with `cleanup` and `summary`
5. Done -- `run.sh` discovers it automatically

Template:

```bash
#!/usr/bin/env bash
SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "-- My New Test --"

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

Container naming: each scenario uses a unique container name prefix (e.g., `e2e-form-*`, `e2e-ping-*`) to avoid conflicts between scenarios. The shared network (`syfrah-e2e`) is reused.

## Test helpers (Rust)

A placeholder `tests/helpers/mod.rs` exists for future Rust-based E2E helpers (`TestNode`), but is not yet implemented. All current E2E testing uses the shell-script approach described above.

## CI integration

### Per-layer tests (ci.yml)

The existing CI workflow runs unit + non-ignored integration tests per layer:

```
    Push / PR
         |
    +----+------+  +------------+  +------------+
    |syfrah-core|  |syfrah-     |  |syfrah-bin  |
    |           |  |fabric      |  |            |
    | clippy    |  | clippy     |  | clippy     |
    | test      |  | test       |  | test       |
    +-----------+  +------------+  +------------+

    Only unit tests and non-#[ignore]d integration tests.
    No root, no WireGuard, no Docker.
    Fast (~1-2 min per layer).
```

### E2E tests (e2e.yml)

A separate workflow runs the full E2E suite. It builds the Docker image once, discovers which layers have scenarios, then runs each layer's tests **in parallel** using a matrix strategy:

```
    Push to main + PRs with code changes + weekly (Monday 4am UTC) + manual
         |
    +----+-----------------------------+
    |  build job                        |
    |  docker build → upload artifact   |
    +----+-----------------------------+
         |
    +----+-----------------------------+
    |  discover job                     |
    |  Extract layer names from         |
    |  scenario filenames (fabric,      |
    |  state, ux, quickstart, ...)      |
    +----+-----------------------------+
         |
         +-------+-------+-------+
         |       |       |       |  (parallel matrix)
    +----+--+ +--+---+ +-+----+ +--+---+
    |fabric | |state | |ux    | |quick |
    |       | |      | |      | |start |
    | load  | | load | | load | | load |
    | image | | img  | | img  | | img  |
    | run.sh| | run  | | run  | | run  |
    | fabric| | state| | ux   | | quick|
    +-------+ +------+ +------+ +------+
```

Containers use `--privileged` for WireGuard kernel access.

## Running tests locally

```bash
# Unit tests (all layers, fast, no root, no Docker)
just test

# Unit tests for one layer
cargo test -p syfrah-fabric

# E2E tests (requires Docker)
just e2e

# Run a specific E2E scenario
./tests/e2e/run.sh 01_mesh

# Everything (unit + E2E)
just ci && just e2e
```

## What we test at each layer

> Layers marked **(planned)** have README-only documentation; their tests do not yet exist.

| Layer | Unit tests | Integration tests | E2E scenarios |
|---|---|---|---|
| **core** | Types, validation, crypto, addressing | Serialization roundtrips | -- |
| **fabric** | Key generation, address derivation | WireGuard interface (root), peering protocol, state persistence | Multi-node mesh, connectivity, rejoin, secret rotation, stress tests |
| **state** | -- | -- | `syfrah state list/get/drop` |
| **compute** (planned) | VM spec validation | Firecracker process lifecycle (root) | VM create/start/stop/delete |
| **storage** (planned) | Volume spec validation | ZeroFS NBD management (root) | Volume attach, detach, migrate |
| **overlay** (planned) | IPAM allocation, MAC derivation | Bridge/VXLAN creation (root), nftables rules (root) | VPC isolation, cross-node connectivity |
| **controlplane** (planned) | Scheduler scoring, state machine | Raft consensus (multi-instance) | Leader election, failover |
| **org** (planned) | Name validation, hierarchy logic | -- | Full org/project/env lifecycle |
| **iam** (planned) | Role permissions, key hashing | -- | Auth flow, API key scoping |

## Conventions

| Convention | Rule |
|---|---|
| Unit test location | Inline `#[cfg(test)]` in source files |
| Integration test location | `layers/{layer}/tests/` |
| E2E test location | `tests/e2e/scenarios/` (shell scripts) |
| E2E infrastructure | Docker containers with WireGuard, orchestrated by `run.sh` |
| Test naming | Descriptive: `01_fabric_mesh_formation.sh`, not `test_1.sh` |
| Cleanup | Each scenario calls `cleanup` to remove its containers |
| Shared helpers | `tests/e2e/lib.sh` for E2E shell functions |
| CI unit/integration | `ci.yml` -- automatic, per-layer, every push |
| CI E2E | `e2e.yml` -- on code changes + weekly + manual |
| Local full suite | `just ci && just e2e` |
