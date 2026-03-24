# Syfrah

Open-source control plane to transform dedicated servers into a programmable cloud.

## Build & Test
- `cargo build` — build all crates
- `cargo test` — run all tests
- `cargo clippy` — lint

## Repository Structure
- `layers/core` — `syfrah-core`: Pure types, crypto, addressing (no I/O, no async)
- `layers/fabric` — `syfrah-fabric`: WireGuard mesh + peering + daemon + CLI commands
- `bin/syfrah` — Binary that composes all layers (zero logic)
- `layers/{forge,compute,storage,overlay,controlplane,org,iam,products}` — Future layers (README only)
- `handbook/` — Project handbook (cross-cutting docs)

## Key Modules (layers/fabric/src/)
- `peering.rs` — TCP peering protocol (join requests, peer announcements, PIN auto-accept)
- `control.rs` — Unix domain socket for CLI-daemon communication
- `daemon.rs` — Daemon loop, init/join/start/leave flows
- `store.rs` — State persistence (~/.syfrah/state.json)
- `wg.rs` — WireGuard interface management
- `cli/` — CLI commands for `syfrah fabric ...`

## Workflow
- Project board: Backlog > Ready > In Progress > In Review > Done
- Pick highest-priority, smallest task from Ready
- Branch: `{issue-number}-{short-slug}` from `main`
- Run `cargo fmt && cargo clippy && cargo test` before pushing
- PR must include `Closes #N`
- CI validates; green → merge + delete branch; red → fix + re-push
- See `handbook/workflow.md` for the full contribution workflow

## Conventions
- serde Serialize/Deserialize on all public types
- thiserror for library errors, anyhow for binaries
- Async runtime: tokio
- IPv6-native (ULA inside mesh)
- Manual peering: no automatic discovery, operator approves join requests
- One layer = one directory in `layers/`, one Rust crate, one README
- CLI commands live inside their layer crate (`src/cli/`)
- Lower layers never depend on higher layers
