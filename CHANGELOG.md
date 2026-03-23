# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - Unreleased

Initial release of Syfrah: an open-source control plane to transform dedicated servers into a programmable cloud.

### Added

#### Core layer (`syfrah-core`)
- Deterministic IPv6 ULA addressing derived from mesh identity
- Mesh identity with cryptographic node IDs (SHA-256, base58-encoded)
- AES-GCM encryption primitives for mesh secrets
- Pure types for peers, meshes, zones, and regions (no I/O, no async)
- serde Serialize/Deserialize on all public types

#### State layer (`syfrah-state`)
- Embedded state persistence using redb (one file per layer)
- Atomic read/write operations with ACID transactions
- CLI commands for inspecting and managing state (`syfrah state list`, `syfrah state get`, `syfrah state delete`)
- Stress, corruption recovery, and fuzzing tests

#### Fabric layer (`syfrah-fabric`)
- WireGuard mesh networking with automatic tunnel configuration
- TCP peering protocol with PIN-based manual approval (no auto-discovery)
- Daemon with init/join/start/leave lifecycle flows
- Unix domain socket control protocol for CLI-daemon communication
- Peer announcements with retry and exponential backoff
- Zones and regions for topology-aware mesh organization
- Configurable daemon intervals via `config.toml`
- Structured logging with tracing fields and JSON output
- `syfrah fabric diagnose` command for mesh troubleshooting
- Enhanced `syfrah fabric status` with WireGuard health and peer breakdown
- Persistent event log for mesh activity
- Actionable CLI error messages with context

#### Binary (`syfrah`)
- Unified CLI binary composing all layers via namespace-per-layer architecture (`syfrah fabric ...`, `syfrah state ...`)

#### Infrastructure
- Dynamic per-layer CI with automatic crate discovery (GitHub Actions)
- Docker-based E2E tests for multi-node mesh formation (50+ scenarios)
- Documentation site with Next.js, auto-synced from layer READMEs
- Security audit workflow (`cargo audit`, weekly + on dependency changes)
- Comprehensive test suite: unit, integration, store atomicity, and E2E tests

[0.1.0]: https://github.com/sifrah/syfrah/releases/tag/v0.1.0
