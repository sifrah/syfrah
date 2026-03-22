# Syfrah

Open-source platform to transform dedicated servers into a cloud provider.

## Build & Test
- `cargo build` — build all crates
- `cargo test` — run all tests
- `cargo clippy` — lint

## Architecture
- `crates/syfrah-core` — Pure types, crypto, addressing (no I/O, no async)
- `crates/syfrah-net` — WireGuard management + iroh discovery + daemon
- `crates/syfrah-cli` — CLI binary `syfrah`

## Conventions
- serde Serialize/Deserialize on all public types
- thiserror for library errors, anyhow for binaries
- Async runtime: tokio
- IPv6-native (ULA inside mesh)
