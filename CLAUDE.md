# Syfrah

Open-source platform to transform dedicated servers into a cloud provider.

## Build & Test
- `cargo build` — build all crates
- `cargo test` — run all tests
- `cargo clippy` — lint

## Architecture
- `crates/syfrah-core` — Pure types, crypto, addressing (no I/O, no async)
- `crates/syfrah-net` — WireGuard management + peering + daemon + control channel
- `crates/syfrah-cli` — CLI binary `syfrah`

## Key Modules
- `peering.rs` — TCP peering protocol (join requests, peer announcements, PIN auto-accept)
- `control.rs` — Unix domain socket for CLI-daemon communication
- `daemon.rs` — Daemon loop, init/join/start/leave flows
- `store.rs` — State persistence (~/.syfrah/state.json)
- `wg.rs` — WireGuard interface management

## Conventions
- serde Serialize/Deserialize on all public types
- thiserror for library errors, anyhow for binaries
- Async runtime: tokio
- IPv6-native (ULA inside mesh)
- Manual peering: no automatic discovery, operator approves join requests
