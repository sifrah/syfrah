# Repository Structure

## Principle: the repo IS the architecture

A layer is an **architectural boundary** first, and a Rust crate second. The crate exists to enforce the boundary in code.

The repository is organized by architectural layer. Each layer is a self-contained folder with its own code, documentation, and CLI commands. The top-level binary just composes the layers together.

```
    The repo                              The architecture
    ────────                              ────────────────

    layers/core/                          Foundation types (no I/O)
    layers/fabric/                        Fabric layer
    layers/forge/                         Forge layer
    layers/compute/                       Compute layer
    layers/storage/                       Storage layer
    layers/overlay/                       Overlay layer
    layers/controlplane/                  Control plane layer
    layers/org/                           Organization model
    layers/iam/                           IAM layer
    layers/products/                      Products layer

    One folder = one layer.
    Adding a layer = adding a folder.
```

## Foundation: `layers/core/`

The `syfrah-core` crate is the foundation that every other layer depends on. It contains **shared types, validation, and pure logic** — nothing else.

**Strict rules:**
- No I/O
- No network
- No async
- No filesystem access
- Only: types, validation, small pure functions

**What lives in core:**

| Category | Examples |
|---|---|
| IDs | `OrgId`, `ProjectId`, `EnvId`, `VmId`, `VpcId`, `SubnetId`, `VolumeId`, `NodeId` |
| Errors | `SyfrahError` base type, error conversion traits |
| Timestamps | `Timestamp` wrapper, duration helpers |
| Resource names | Name validation (DNS-compatible, length limits) |
| Labels | `Labels` type (`BTreeMap<String, String>`), label validation |
| Phases | `VmPhase`, `VolumePhase`, `VpcPhase`, `NodePhase` enums |
| Crypto | `MeshSecret`, key derivation, encryption helpers |
| Addressing | IPv6 ULA prefix derivation, node address derivation |
| IPAM | IP pool bitmap, MAC derivation — pure math, no allocation |
| Serde helpers | Common serialization patterns |
| API envelopes | `ApiResponse<T>`, `ApiError`, pagination types |

**Why this matters:** without `core`, shared types end up duplicated across layers or create false dependencies. `core` is the single import that every layer can rely on without pulling in I/O or network code.

**Discipline:** `core` must not become a catch-all. The litmus test: if a type or function is only used by one layer, it belongs in that layer, not in `core`. Only types referenced by 2+ layers belong here. Review `core`'s surface area regularly — if it grows beyond ~15 modules, something is wrong.

```
    layers/core/
    ├── Cargo.toml               crate: syfrah-core
    ├── README.md                Foundation types and conventions
    └── src/
        ├── lib.rs
        ├── ids.rs               All ID newtypes (Uuid-based)
        ├── errors.rs            SyfrahError, Result type alias
        ├── names.rs             Resource name validation
        ├── labels.rs            Label type and validation
        ├── phases.rs            Phase enums for all resource types
        ├── secret.rs            MeshSecret, key derivation
        ├── addressing.rs        IPv6 ULA addressing
        ├── ipam.rs              IP pool bitmap (pure math)
        ├── crypto.rs            AES-256-GCM helpers
        └── api.rs               ApiResponse<T>, ApiError, pagination
```

## Layer structure

Every layer (except `core`) follows the same internal structure:

```
    layers/{layer}/
    ├── Cargo.toml              Rust crate for this layer
    ├── README.md               Concept documentation (fixed template)
    ├── src/
    │   ├── lib.rs              Library code (types, logic, I/O)
    │   ├── cli/
    │   │   ├── mod.rs          CLI commands for this layer
    │   │   ├── {command}.rs    One file per command
    │   │   └── ...
    │   └── ...                 Layer-specific modules
    └── tests/
        └── ...                 Tests for this layer
```

The three outputs of each layer:

| Output | Where | Purpose |
|---|---|---|
| **Library** | `src/lib.rs` | Rust crate that other layers can depend on |
| **Documentation** | `README.md` | Concept doc (fixed template, see below) |
| **CLI commands** | `src/cli/` | Commands registered under `syfrah {layer} ...` |

## README template

Every layer's `README.md` follows the same structure. No drift.

```markdown
# {Layer Name}

## Purpose
What this layer does in one paragraph.

## Responsibilities
Bulleted list of what this layer owns.

## Non-goals
What this layer does NOT do (explicitly).

## Public concepts
Key abstractions exposed to other layers and to the user.

## Main types
The core Rust types this crate exports (with brief descriptions).

## CLI commands
Table of `syfrah {layer} {command}` with one-line descriptions.

## Dependencies
Which other layers this crate depends on, and why.

## Data ownership
What state this layer owns, where it's stored (Raft, gossip, local),
and who the source of truth is.

## Failure modes
What can go wrong, how it's detected, how it's recovered.

## Tests
How to run tests, what's covered, what requires root/integration.
```

This template ensures that every layer is documented consistently. A new contributor can open any layer, read the README, and understand it in 5 minutes.

## CLI scope: operator vs tenant

Commands fall into two categories with different scopes. This is documented explicitly in help text and the README.

### Operator commands (node-scoped, infra)

These run locally on a node or target a specific node. They manage infrastructure, not tenant resources.

```
    syfrah fabric ...          Mesh management (local node + mesh)
    syfrah forge ...           Per-node debug/ops (local or --node remote)
```

**Who uses them:** The platform operator. The person who rents the servers and runs Syfrah.

**How they work:** Direct access to the local daemon (Unix socket) or direct query to a remote node's forge API (via fabric).

### Tenant commands (cluster-scoped, control plane)

These talk to the control plane API (HTTP on the fabric). They manage cloud resources. Any node can accept the request — it's forwarded to the Raft leader internally.

```
    syfrah org ...             Organizations
    syfrah project ...         Projects
    syfrah env ...             Environments
    syfrah vm ...              Virtual machines
    syfrah vpc ...             VPCs
    syfrah subnet ...          Subnets
    syfrah sg ...              Security groups
    syfrah volume ...          Volumes
    syfrah user ...            Users
    syfrah iam ...             Roles
    syfrah apikey ...          API keys
    syfrah login / logout      Authentication
```

**Who uses them:** The platform operator AND tenants (via API keys or login sessions).

**How they work:** HTTP request to the local control plane API → forwarded to Raft leader → committed → reconciled by forges.

### Visual separation in help

```
$ syfrah --help

Syfrah — turn dedicated servers into a programmable cloud

Infrastructure:
  fabric    Manage the WireGuard fabric mesh
  forge     Per-node debug and operations

Resources:
  org       Manage organizations
  project   Manage projects
  env       Manage environments
  vm        Manage virtual machines
  vpc       Manage VPCs
  subnet    Manage subnets
  sg        Manage security groups
  volume    Manage volumes

Identity:
  user      Manage users
  iam       Manage role assignments
  apikey    Manage API keys
  login     Authenticate
  logout    Clear session
```

## The full repo

```
syfrah/
│
├── layers/
│   │
│   ├── core/                        Foundation types
│   │   ├── Cargo.toml               crate: syfrah-core
│   │   ├── README.md
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── ids.rs
│   │       ├── errors.rs
│   │       ├── names.rs
│   │       ├── labels.rs
│   │       ├── phases.rs
│   │       ├── secret.rs
│   │       ├── addressing.rs
│   │       ├── ipam.rs
│   │       ├── crypto.rs
│   │       └── api.rs
│   │
│   ├── fabric/                      WireGuard mesh
│   │   ├── Cargo.toml               crate: syfrah-fabric
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── cli/
│   │   │   │   ├── mod.rs           FabricCommand
│   │   │   │   ├── init.rs          syfrah fabric init
│   │   │   │   ├── join.rs
│   │   │   │   ├── start.rs
│   │   │   │   ├── stop.rs
│   │   │   │   ├── leave.rs
│   │   │   │   ├── status.rs
│   │   │   │   ├── peers.rs
│   │   │   │   ├── token.rs
│   │   │   │   ├── rotate.rs
│   │   │   │   └── peering.rs
│   │   │   ├── peering.rs
│   │   │   ├── daemon.rs
│   │   │   ├── wg.rs
│   │   │   ├── store.rs
│   │   │   └── control.rs
│   │   └── tests/
│   │
│   ├── forge/                       Per-node control + debug
│   │   ├── Cargo.toml               crate: syfrah-forge
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── cli/
│   │   │   │   ├── mod.rs           ForgeCommand
│   │   │   │   ├── status.rs
│   │   │   │   ├── vms.rs
│   │   │   │   ├── bridges.rs
│   │   │   │   ├── volumes.rs
│   │   │   │   ├── nftables.rs
│   │   │   │   ├── logs.rs
│   │   │   │   └── drain.rs
│   │   │   ├── server.rs
│   │   │   └── reconciler.rs
│   │   └── tests/
│   │
│   ├── compute/                     Firecracker microVMs
│   │   ├── Cargo.toml               crate: syfrah-compute
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── cli/
│   │   │   │   ├── mod.rs           VmCommand
│   │   │   │   ├── create.rs
│   │   │   │   ├── list.rs
│   │   │   │   ├── start.rs
│   │   │   │   ├── stop.rs
│   │   │   │   ├── reboot.rs
│   │   │   │   ├── delete.rs
│   │   │   │   └── ssh.rs
│   │   │   ├── firecracker.rs
│   │   │   ├── jailer.rs
│   │   │   └── images.rs
│   │   └── tests/
│   │
│   ├── storage/                     ZeroFS + S3
│   │   ├── Cargo.toml               crate: syfrah-storage
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── cli/
│   │   │   │   ├── mod.rs           VolumeCommand
│   │   │   │   ├── create.rs
│   │   │   │   ├── list.rs
│   │   │   │   ├── attach.rs
│   │   │   │   ├── detach.rs
│   │   │   │   ├── delete.rs
│   │   │   │   └── snapshot.rs
│   │   │   ├── zerofs.rs
│   │   │   └── s3.rs
│   │   └── tests/
│   │
│   ├── overlay/                     VXLAN, VPC, SG, DNS
│   │   ├── Cargo.toml               crate: syfrah-overlay
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── cli/
│   │   │   │   ├── mod.rs           VpcCommand, SubnetCommand, SgCommand
│   │   │   │   ├── vpc_create.rs
│   │   │   │   ├── vpc_list.rs
│   │   │   │   ├── vpc_delete.rs
│   │   │   │   ├── vpc_peer.rs
│   │   │   │   ├── subnet_create.rs
│   │   │   │   ├── subnet_list.rs
│   │   │   │   ├── sg_create.rs
│   │   │   │   ├── sg_list.rs
│   │   │   │   ├── sg_add_rule.rs
│   │   │   │   └── sg_remove_rule.rs
│   │   │   ├── vxlan.rs
│   │   │   ├── bridge.rs
│   │   │   ├── fdb.rs
│   │   │   ├── firewall.rs
│   │   │   ├── ipam.rs
│   │   │   ├── routing.rs
│   │   │   └── dns.rs
│   │   └── tests/
│   │
│   ├── controlplane/                Raft + gossip + scheduler + API
│   │   ├── Cargo.toml               crate: syfrah-controlplane
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── raft/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── store.rs
│   │   │   │   ├── network.rs
│   │   │   │   └── state_machine.rs
│   │   │   ├── gossip/
│   │   │   │   ├── mod.rs
│   │   │   │   └── node_health.rs
│   │   │   ├── scheduler.rs
│   │   │   ├── reconciler.rs
│   │   │   └── api/
│   │   │       ├── mod.rs
│   │   │       ├── server.rs
│   │   │       └── handlers.rs
│   │   └── tests/
│   │
│   ├── org/                         Org, projects, environments
│   │   ├── Cargo.toml               crate: syfrah-org
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── cli/
│   │   │   │   ├── mod.rs           OrgCommand, ProjectCommand, EnvCommand
│   │   │   │   ├── org_create.rs
│   │   │   │   ├── org_list.rs
│   │   │   │   ├── org_delete.rs
│   │   │   │   ├── project_create.rs
│   │   │   │   ├── project_list.rs
│   │   │   │   ├── project_delete.rs
│   │   │   │   ├── env_create.rs
│   │   │   │   ├── env_list.rs
│   │   │   │   ├── env_update.rs
│   │   │   │   └── env_destroy.rs
│   │   │   └── types.rs
│   │   └── tests/
│   │
│   ├── iam/                         Users, roles, API keys
│   │   ├── Cargo.toml               crate: syfrah-iam
│   │   ├── README.md
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── cli/
│   │   │   │   ├── mod.rs           UserCommand, IamCommand, ApikeyCommand
│   │   │   │   ├── user_create.rs
│   │   │   │   ├── user_list.rs
│   │   │   │   ├── user_disable.rs
│   │   │   │   ├── iam_assign.rs
│   │   │   │   ├── iam_list.rs
│   │   │   │   ├── iam_revoke.rs
│   │   │   │   ├── apikey_create.rs
│   │   │   │   ├── apikey_list.rs
│   │   │   │   ├── apikey_rotate.rs
│   │   │   │   ├── apikey_delete.rs
│   │   │   │   ├── login.rs
│   │   │   │   └── logout.rs
│   │   │   ├── roles.rs
│   │   │   ├── tokens.rs
│   │   │   └── auth.rs
│   │   └── tests/
│   │
│   └── products/                    Product orchestration
│       ├── Cargo.toml               crate: syfrah-products
│       ├── README.md
│       └── src/
│           ├── lib.rs
│           └── ...
│
├── bin/
│   └── syfrah/
│       ├── Cargo.toml               Binary — composes all layers
│       └── src/
│           └── main.rs              Imports all CLIs, zero logic
│
├── docs/
│   ├── state-and-reconciliation.md  Cross-cutting: reconciliation, phases
│   └── zones-and-regions.md         Cross-cutting: topology metadata
│
├── ARCHITECTURE.md                  Global overview
├── CLAUDE.md                        Build instructions
└── IDEA.md                          Project vision
```

## Binary composition

The binary crate (`bin/syfrah/`) imports CLI commands from every layer and builds the clap tree. It has **zero business logic**.

```rust
// bin/syfrah/src/main.rs

use clap::{Parser, Subcommand};

use syfrah_fabric::cli::FabricCommand;
use syfrah_forge::cli::ForgeCommand;
use syfrah_compute::cli::VmCommand;
use syfrah_storage::cli::VolumeCommand;
use syfrah_overlay::cli::{VpcCommand, SubnetCommand, SgCommand};
use syfrah_org::cli::{OrgCommand, ProjectCommand, EnvCommand};
use syfrah_iam::cli::{UserCommand, IamCommand, ApikeyCommand, LoginArgs};

#[derive(Parser)]
#[command(name = "syfrah", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    // ── Infrastructure (operator, node-scoped) ──
    /// Manage the WireGuard fabric mesh
    Fabric { #[command(subcommand)] command: FabricCommand },
    /// Per-node debug and operations
    Forge { #[command(subcommand)] command: ForgeCommand },

    // ── Resources (tenant, cluster-scoped) ──
    /// Manage organizations
    Org { #[command(subcommand)] command: OrgCommand },
    /// Manage projects
    Project { #[command(subcommand)] command: ProjectCommand },
    /// Manage environments
    Env { #[command(subcommand)] command: EnvCommand },
    /// Manage virtual machines
    Vm { #[command(subcommand)] command: VmCommand },
    /// Manage VPCs
    Vpc { #[command(subcommand)] command: VpcCommand },
    /// Manage subnets
    Subnet { #[command(subcommand)] command: SubnetCommand },
    /// Manage security groups
    Sg { #[command(subcommand)] command: SgCommand },
    /// Manage volumes
    Volume { #[command(subcommand)] command: VolumeCommand },

    // ── Identity ──
    /// Manage users
    User { #[command(subcommand)] command: UserCommand },
    /// Manage role assignments
    Iam { #[command(subcommand)] command: IamCommand },
    /// Manage API keys
    Apikey { #[command(subcommand)] command: ApikeyCommand },
    /// Log in
    Login(LoginArgs),
    /// Log out
    Logout,
}
```

## Dependency graph

Lower layers never depend on higher layers. `core` is the foundation.

```
    syfrah-core             ← depends on nothing (foundation)
    syfrah-fabric           ← depends on core
    syfrah-org              ← depends on core
    syfrah-iam              ← depends on core, org
    syfrah-compute          ← depends on core, fabric
    syfrah-storage          ← depends on core, fabric
    syfrah-overlay          ← depends on core, fabric
    syfrah-forge            ← depends on core, fabric, compute, storage, overlay
    syfrah-controlplane     ← depends on core, fabric, compute, storage, overlay, org, iam
    syfrah-products         ← depends on core, compute, storage, overlay
    bin/syfrah              ← depends on everything
```

**Watch point: `forge` coupling.** The forge depends on 4 layers (fabric, compute, storage, overlay) because it reconciles all of them locally. This is inherent to its role — it's the local orchestrator. But it must interact with those layers through their public API (`lib.rs` exports), never through internal modules. If forge starts reaching into another layer's internals, that's a boundary violation.

## Products: future decomposition

`layers/products/` starts as a single crate. When products become complex enough, they split:

```
    Phase 1 (now):
    layers/products/                    Single crate, generic model

    Phase 2 (when needed):
    layers/products/
    ├── core/                           Shared product types and lifecycle
    ├── vm/                             VM product (thin wrapper)
    ├── lb/                             Load balancer product
    └── postgres/                       Managed PostgreSQL product
```

The split happens when a product needs its own dependencies, its own tests, or its own release cycle. Not before.

## How to add a new layer

1. Create `layers/{layer}/` with `Cargo.toml`, `README.md` (from template), `src/lib.rs`, `src/cli/mod.rs`
2. Add the crate to the workspace `Cargo.toml`
3. Write the `README.md` following the template
4. Write the CLI commands in `src/cli/`
5. Import the CLI in `bin/syfrah/src/main.rs`

## How to add a command to an existing layer

1. Create `layers/{layer}/src/cli/{command}.rs` with `Args` + `run()`
2. Add `mod {command}` + variant in `layers/{layer}/src/cli/mod.rs`
3. Done. No changes to the binary, no changes to other layers.

## Convention summary

| Convention | Rule |
|---|---|
| Layer location | `layers/{name}/` |
| Crate name | `syfrah-{name}` |
| Foundation crate | `layers/core/` — no I/O, no async, pure types only |
| Concept doc | `layers/{name}/README.md` (fixed template) |
| CLI module | `layers/{name}/src/cli/` |
| Command file | `layers/{name}/src/cli/{command}.rs` |
| Command interface | `pub struct Args` + `pub async fn run(args: Args) -> Result<()>` |
| Namespace interface | `pub enum {Name}Command` + `pub async fn run(cmd) -> Result<()>` |
| Binary | `bin/syfrah/` — imports and dispatches, zero logic |
| Cross-cutting docs | `docs/` |
| CLI grouping | Infrastructure (fabric, forge) vs Resources (vm, vpc, ...) vs Identity (user, iam, ...) |
| Dependency rule | Lower layers never depend on higher layers. All depend on core. |
