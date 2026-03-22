# Testing

## Principle: tests follow the architecture

Tests are organized by layer, just like the code. Each layer owns its unit and integration tests. End-to-end tests live at the repo root and test cross-layer scenarios.

```
    layers/core/src/         ← unit tests (inline)
    layers/core/tests/       ← integration tests
    layers/fabric/src/       ← unit tests (inline)
    layers/fabric/tests/     ← integration tests
    layers/compute/tests/    ← integration tests
    ...
    tests/                   ← E2E tests (cross-layer)
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

**CI:** The non-ignored tests run automatically per-layer. Ignored tests run in the E2E workflow.

```rust
// layers/fabric/tests/wireguard.rs

use syfrah_fabric::wg;

#[test]
#[ignore] // requires root — WireGuard interface creation
fn create_and_destroy_interface() {
    let kp = wg::generate_keypair();
    wg::create_interface(&kp.private, 51820).unwrap();
    let device = wg::get_device().unwrap();
    assert_eq!(device.public_key, Some(kp.public.clone()));
    wg::destroy_interface().unwrap();
    assert!(wg::get_device().is_err());
}
```

```rust
// layers/fabric/tests/store.rs

use syfrah_fabric::store;

#[test]
fn save_and_load_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    // ... test state persistence
}
```

**What belongs here:**
- WireGuard interface creation/destruction (root, `#[ignore]`)
- TCP peering protocol (loopback, no root needed)
- State file persistence (tempdir)
- Control socket communication (Unix socket)

**What does NOT belong here:**
- Multi-node scenarios (that's E2E)
- Cross-layer interactions (that's E2E)

### E2E tests

**What:** Full scenarios that cross multiple layers. Simulate a real cluster on a single machine.

**Where:** `tests/` at the repo root. Each file is a scenario.

**Requirements:** Root (WireGuard), Linux. Always `#[ignore]`.

**Run:** `sudo cargo test --test '*' -- --ignored`

**CI:** Dedicated workflow (`e2e.yml`), weekly + manual + on code changes.

```rust
// tests/fabric_mesh.rs

mod helpers;
use helpers::TestNode;

#[tokio::test]
#[ignore] // requires root
async fn three_nodes_form_mesh() {
    let node_a = TestNode::init("node-a", "test-mesh").await;
    let pin = node_a.start_peering().await;

    let node_b = TestNode::join("node-b", node_a.peering_addr(), &pin).await;
    let node_c = TestNode::join("node-c", node_a.peering_addr(), &pin).await;

    // All nodes see each other
    assert_eq!(node_a.peer_count().await, 2);
    assert_eq!(node_b.peer_count().await, 2);
    assert_eq!(node_c.peer_count().await, 2);

    // Cleanup
    node_c.stop().await;
    node_b.stop().await;
    node_a.stop().await;
}
```

**What belongs here:**
- Multi-node mesh formation
- VM lifecycle across the stack (create, start, stop, delete)
- VPC isolation verification (traffic between VPCs is blocked)
- Volume migration between nodes
- Full scenario: org → project → env → VPC → VM → connectivity

**What does NOT belong here:**
- Tests that can run in a single layer (move them to integration tests)
- Performance benchmarks (separate concern)

## Test helpers

Shared utilities for E2E tests live in `tests/helpers/`:

```rust
// tests/helpers/mod.rs

/// A syfrah node running in a temp directory with unique ports.
/// Used by E2E tests to simulate a multi-node cluster on one machine.
pub struct TestNode {
    pub name: String,
    pub wg_port: u16,
    pub peering_port: u16,
    pub api_port: u16,
    pub data_dir: tempfile::TempDir,
    pub mesh_ipv6: Option<Ipv6Addr>,
}

impl TestNode {
    /// Initialize a new mesh (equivalent to `syfrah fabric init`)
    pub async fn init(name: &str, mesh_name: &str) -> Self { ... }

    /// Join an existing mesh (equivalent to `syfrah fabric join`)
    pub async fn join(name: &str, target: SocketAddr, pin: &str) -> Self { ... }

    /// Start peering and return the auto-accept PIN
    pub async fn start_peering(&self) -> String { ... }

    /// Get the number of connected peers
    pub async fn peer_count(&self) -> usize { ... }

    /// Stop the node and clean up
    pub async fn stop(&self) { ... }
}

impl Drop for TestNode {
    fn drop(&mut self) {
        // Ensure cleanup even if test panics
    }
}
```

Each `TestNode` instance:
- Gets its own temp directory (`~/.syfrah` equivalent)
- Gets unique ports (WireGuard, peering, API) to avoid conflicts
- Creates its own WireGuard interface (requires root)
- Cleans up on drop (interface teardown, temp dir removal)

## CI integration

### Per-layer tests (ci.yml)

The existing CI workflow runs unit + non-ignored integration tests per layer:

```
    Push / PR
         │
    ┌────┴──────┐  ┌────────────┐  ┌────────────┐
    │syfrah-core│  │syfrah-     │  │syfrah-bin  │
    │           │  │fabric      │  │            │
    │ clippy    │  │ clippy     │  │ clippy     │
    │ test      │  │ test       │  │ test       │
    └───────────┘  └────────────┘  └────────────┘

    Only unit tests and non-#[ignore]d integration tests.
    No root, no WireGuard, no Firecracker.
    Fast (~1-2 min per layer).
```

### E2E tests (e2e.yml)

A separate workflow runs the full E2E suite:

```
    Weekly (Monday 4am UTC) + manual + PR with code changes
         │
    ┌────┴────────────────────────────────────────┐
    │  Ubuntu, root, WireGuard installed           │
    │                                              │
    │  sudo cargo test --test '*' -- --ignored     │
    │                                              │
    │  Runs all tests/ scenarios:                  │
    │    fabric_mesh.rs                            │
    │    vm_lifecycle.rs        (when implemented) │
    │    vpc_isolation.rs       (when implemented) │
    │    storage_migration.rs   (when implemented) │
    │    full_stack.rs          (when implemented) │
    └──────────────────────────────────────────────┘
```

The E2E workflow:
- Runs on `ubuntu-latest` with `sudo`
- Installs WireGuard tools
- Only triggers on code changes (not doc-only changes)
- Can be triggered manually via `workflow_dispatch`
- Runs weekly as a regression safety net

## Running tests locally

```bash
# Unit tests (all layers, fast, no root)
just test

# Unit tests for one layer
cargo test -p syfrah-fabric

# Integration tests requiring root (one layer)
sudo cargo test -p syfrah-fabric -- --ignored

# E2E tests (all scenarios, root required)
sudo cargo test --test '*' -- --ignored

# Everything
sudo cargo test --workspace -- --include-ignored
```

## Writing tests

### For a new unit test

Add a `#[test]` function inside a `#[cfg(test)]` block in the source file. No special setup needed.

### For a new integration test

Create a file in `layers/{layer}/tests/`. If it needs root, add `#[ignore]`.

```rust
// layers/{layer}/tests/my_test.rs

#[test]
fn my_test() {
    // ...
}

#[test]
#[ignore] // requires root
fn my_root_test() {
    // ...
}
```

### For a new E2E scenario

Create a file in `tests/`. Always `#[ignore]`. Use the `TestNode` helpers.

```rust
// tests/my_scenario.rs

mod helpers;
use helpers::TestNode;

#[tokio::test]
#[ignore]
async fn my_scenario() {
    let node = TestNode::init("test", "mesh").await;
    // ... test something across layers
    node.stop().await;
}
```

## What we test at each layer

| Layer | Unit tests | Integration tests | E2E scenarios |
|---|---|---|---|
| **core** | Types, validation, crypto, IPAM math | Serialization roundtrips | — |
| **fabric** | Key generation, address derivation | WireGuard interface (root), peering protocol, state persistence | Multi-node mesh formation |
| **compute** | VM spec validation | Firecracker process lifecycle (root) | VM create/start/stop/delete |
| **storage** | Volume spec validation | ZeroFS NBD management (root) | Volume attach, detach, migrate |
| **overlay** | IPAM allocation, MAC derivation | Bridge/VXLAN creation (root), nftables rules (root) | VPC isolation, cross-node connectivity |
| **controlplane** | Scheduler scoring, state machine | Raft consensus (multi-instance) | Leader election, failover |
| **org** | Name validation, hierarchy logic | — | Full org/project/env lifecycle |
| **iam** | Role permissions, key hashing | — | Auth flow, API key scoping |

## Conventions

| Convention | Rule |
|---|---|
| Unit test location | Inline `#[cfg(test)]` in source files |
| Integration test location | `layers/{layer}/tests/` |
| E2E test location | `tests/` (repo root) |
| Root-required tests | Always `#[ignore]` |
| Test naming | Descriptive: `three_nodes_form_mesh`, not `test_1` |
| Cleanup | Use `Drop` or explicit cleanup. Tests must not leak interfaces or processes. |
| Shared helpers | `tests/helpers/mod.rs` for E2E |
| CI unit/integration | `ci.yml` — automatic, per-layer, every push |
| CI E2E | `e2e.yml` — weekly + manual + code PR |
| Local full suite | `sudo cargo test --workspace -- --include-ignored` |
