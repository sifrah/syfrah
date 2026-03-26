# Control Plane API Architecture

## Design Principles

This document defines how all layers (fabric, forge, compute, storage, overlay, controlplane, org, iam, products) communicate internally and expose their APIs.

These decisions were reached through architectural review by five specialists covering systems engineering, type safety, distributed systems performance, CLI/UX, and security.

### Core Principles

1. **The CLI drives the API design.** Write the `--help` text first, then build the handler.
2. **One binary, one daemon, one process.** All layers run inside a single `syfrah` process.
3. **Unix sockets for local control.** No TCP for CLI-to-daemon. The kernel handles authentication.
4. **JSON everywhere.** One format, one set of debug tools (`curl`, `jq`), one `serde` dependency.
5. **HTTP for observability only.** Read-only endpoints for monitoring. No mutations over HTTP.
6. **Least privilege, smallest surface.** Every endpoint is an attack surface. Fewer is better.

---

## Transport

### CLI → Daemon (local, same machine)

**Transport:** Unix domain socket at `~/.syfrah/control.sock`

**Why Unix socket:**
- Kernel-enforced access control via filesystem permissions (0600, owner-only)
- Peer credential retrieval via `SO_PEERCRED` (get caller UID without tokens)
- Zero network exposure by construction (cannot be reached from the network)
- 5-15 microsecond round-trip (vs 100-300μs for HTTP loopback)

**Protocol:** Length-prefixed JSON frames (already implemented in `control.rs`)
- 4-byte big-endian length prefix
- JSON payload (max 64KB)
- Read timeout: 5 seconds

**Decision: One socket, not one per layer.** The daemon multiplexes all layers through a single socket. Rationale: simpler for the CLI (one connection point), simpler for the operator (one file to check), and the dispatch cost is one enum match (nanoseconds).

### Node → Node (inter-node, over mesh)

**Transport:** HTTP/1.1 + JSON over the WireGuard fabric interface (`syfrah0`)

The WireGuard tunnel provides encryption equivalent to mTLS. No additional TLS layer needed for inter-node HTTP.

**Bind address:** Hardcoded to the mesh IPv6 address or `127.0.0.1`. Never `0.0.0.0`. This is enforced in code, not configuration.

### External monitoring

**Transport:** HTTP on `127.0.0.1:9100` (disabled by default, opt-in via config)

**Endpoints:** Read-only only. `/metrics` (Prometheus), `/v1/{layer}/health`. No mutations.

---

## Message Format

**JSON everywhere.** All control messages, all HTTP responses, all `--json` CLI output.

**Why not protobuf:**
- Every public type already derives `Serialize`/`Deserialize` — zero migration cost
- `serde_json` serialization of a 500-byte message: ~2μs. Protobuf: ~1μs. Difference is noise at our scale (<10,000 msg/s)
- JSON is debuggable with `curl`, `jq`, `socat`. Protobuf requires special tools.
- Adding protobuf means a second serialization system, `.proto` file governance, and `prost-build` in every crate

**Exception:** If profiling proves a specific hot path (Raft log entries, gossip protocol) is serialization-bound, introduce protobuf for that path only. Measure first.

---

## API Naming

### HTTP endpoints

```
/{version}/{layer}/{resource}
```

Examples:
```
GET  /v1/fabric/peers
GET  /v1/fabric/topology
GET  /v1/fabric/health
GET  /v1/forge/vms
GET  /v1/overlay/vpcs
GET  /v1/controlplane/raft/status
```

### Unix socket messages

Namespaced by layer using a dispatch enum:

```rust
enum LayerRequest {
    Fabric(FabricRequest),
    Forge(ForgeRequest),
    Compute(ComputeRequest),
    Overlay(OverlayRequest),
    ControlPlane(ControlPlaneRequest),
    Org(OrgRequest),
    Iam(IamRequest),
}
```

Each layer defines its own request/response enum pair. The daemon dispatches by layer.

### CLI commands

```
syfrah {layer} {resource} {verb} [args] [--flags]
```

Examples:
```bash
syfrah fabric peers list
syfrah fabric peers remove web-3
syfrah compute vm create --name web-1 --vcpu 2
syfrah overlay vpc create --name prod --cidr 10.0.0.0/16
```

The CLI command tree maps 1:1 to the API namespace.

---

## CLI Interaction Model

### The CLI is a thin client

The CLI does three things:
1. Parse arguments (clap)
2. Send a request to the daemon (Unix socket)
3. Format the response (human or `--json`)

No business logic in the CLI. No direct state access. No direct WireGuard operations.

### Global flags

| Flag | Description |
|------|-------------|
| `--json` | Machine-readable JSON output (global, inherited by all subcommands) |
| `--verbose` | Additional fields in output (never changes the schema shape) |
| `--yes` | Skip confirmation prompts on destructive operations |

### Error responses

Every error from the daemon includes:

```json
{
  "error": {
    "code": "PEER_NOT_FOUND",
    "message": "No peer named 'web-3' in the mesh. Did you mean 'web-03'?",
    "trace_id": "req-a7f3"
  }
}
```

- `code`: Machine-readable, stable across versions. Scripts match on this.
- `message`: Human-readable, may change. Includes actionable next steps.
- `trace_id`: 6-character random ID, also logged server-side. For debugging.

The CLI prints: `Error [req-a7f3]: No peer named 'web-3'. Did you mean 'web-03'?`

### Output patterns

Every command follows one of these patterns:

| Pattern | When | Example |
|---------|------|---------|
| **Spinner → result** | Mutating operations | `syfrah fabric peers remove web-3` |
| **Table** | Listing resources | `syfrah fabric peers` |
| **Structured display** | Status/detail views | `syfrah fabric status` |
| **Pass/fail checks** | Diagnostics | `syfrah fabric diagnose` |

Non-TTY (piped) output: no spinners, no colors, no box-drawing. Same data, plain text.

---

## Layer Registration

### Each layer provides

```rust
// In layers/{layer}/src/lib.rs

/// The control API for this layer
pub trait LayerHandler: Send + Sync {
    /// Handle a request from the CLI or another layer
    async fn handle(&self, request: LayerRequest) -> LayerResponse;

    /// HTTP routes for observability (read-only)
    fn routes(&self) -> axum::Router;
}
```

Each layer crate exports:
1. A `{Layer}Request` / `{Layer}Response` enum pair
2. A `{Layer}Handler` implementing `LayerHandler`
3. A clap `{Layer}Command` subcommand enum for the CLI
4. An axum `Router` fragment for HTTP observability

### Daemon composition

```rust
// In bin/syfrah/src/main.rs (simplified)

let daemon = Daemon::new()
    .register(FabricHandler::new(config))
    .register(ForgeHandler::new(config))
    .register(ComputeHandler::new(config))
    // ...
    .build();

// HTTP: merge all layer routers
let app = Router::new()
    .nest("/v1/fabric", fabric.routes())
    .nest("/v1/forge", forge.routes())
    // ...

// Unix socket: dispatch by LayerRequest variant
daemon.serve(control_socket, http_app).await;
```

Registration is compile-time. No dynamic plugin loading. Feature flags in `Cargo.toml` control which layers are included. Disabled layers are not compiled.

---

## Versioning

### Strategy: additive-only within a version

- **New fields:** Add with `#[serde(default)]`. Old clients ignore them. Non-breaking.
- **New endpoints:** Add alongside existing ones. Non-breaking.
- **New request variants:** Add to the enum. Old clients never send them. Non-breaking.
- **Breaking changes:** Bump version (`/v1/` → `/v2/`). Run both for one release cycle, then remove v1.

### CLI and daemon are always the same version

The CLI and daemon ship as a single binary. There is no version mismatch scenario. This eliminates an entire class of compatibility problems.

### No premature versioning

Ship `/v1/`. Stabilize it. Only create `/v2/` when something actually breaks. Do not version speculatively.

---

## Security

### Authentication

**Local (CLI → daemon):** Kernel-enforced via Unix socket filesystem permissions. No tokens. `SO_PEERCRED` provides the caller's UID.

**Remote (node → node):** API keys scoped per-project (`syf_key_{project}_{random}`), transmitted over the WireGuard-encrypted fabric.

### Authorization

Every request is an authorization decision. The handler checks `(caller_uid, layer, command)` against an allow list.

Default: deny all. Initial policy: root and daemon owner UID get full access.

The IAM layer will add role-based access control later. The authorization hook must exist from day one.

### Audit

Every mutating command is logged to `~/.syfrah/audit.log` with:
- Timestamp
- Caller UID (from `SO_PEERCRED`)
- Layer
- Command
- Result (success/failure)
- Trace ID

The audit module is a shared dependency, not layer-specific.

### Network exposure rules

| Resource | Allowed exposure |
|----------|-----------------|
| Control socket | Local filesystem only (never network) |
| Mutating API endpoints | Unix socket only (never HTTP) |
| State database (redb) | Local filesystem only |
| Audit log | Local filesystem only |
| Health/metrics HTTP | `127.0.0.1` or `syfrah0` interface only |
| WireGuard port (UDP 51820) | Public interface |
| Peering port (TCP 51821) | Public interface (temporary, during join) |

Nothing else is exposed. If someone asks for "remote CLI access," the answer is SSH.

---

## Migration Plan

### Phase 1: Extract shared control protocol (1-2 days)

Move the socket creation, length-prefixed JSON framing, read timeout, and `ControlHandler` trait from `layers/fabric/src/control.rs` into a shared location (either `syfrah-core` or a new `layers/api/` crate).

### Phase 2: Namespace fabric requests (1 day)

Rename `ControlRequest`/`ControlResponse` to `FabricRequest`/`FabricResponse`. Wrap in `LayerRequest::Fabric(...)`. The daemon dispatches on the outer enum.

### Phase 3: Add error codes and trace IDs (1 day)

Replace `ControlResponse::Error { message }` with `ControlResponse::Error { code, message, trace_id }`. Update all CLI error handling.

### Phase 4: Add `SO_PEERCRED` and audit UID (1 day)

Read peer credentials on every connection. Add `caller_uid` to audit entries.

### Phase 5: Implement for next layer (forge)

When the forge layer is built, it follows this architecture from day one: defines `ForgeRequest`/`ForgeResponse`, implements `LayerHandler`, exports clap commands, and registers with the daemon.

---

## Anti-Patterns (What We Will Never Do)

1. **Never add gRPC.** Protobuf codegen, `.proto` governance, version matrices. We have serde. Ship.
2. **Never create a separate API gateway process.** The daemon IS the gateway.
3. **Never expose mutations over HTTP.** Unix socket for writes. HTTP for reads.
4. **Never use bearer tokens for local IPC.** `SO_PEERCRED` is unforgeable.
5. **Never create one socket per layer.** One socket, one dispatch enum.
6. **Never let the HTTP bind address be configurable to `0.0.0.0`.** Hardcode to localhost/syfrah0.
7. **Never ship a layer without authorization hooks.** Deny by default.
8. **Never design the RPC first and generate the CLI from it.** CLI drives API.
9. **Never use different serialization formats for different layers.** JSON everywhere.
10. **Never version prematurely.** Ship v1, iterate.
