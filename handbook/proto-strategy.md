# Proto Strategy

This document defines the conventions, file layout, and generation pipeline for all
`.proto` files in the Syfrah repository. Proto files are the **source of truth** for
every external API surface — SDKs, REST endpoints, and interactive docs are all
derived from them.

---

## 1. File Locations

Proto files live in two places:

```
layers/{layer}/proto/{layer}.proto   # Per-layer service definitions
api/proto/syfrah/v1/common.proto     # Shared types (pagination, errors, health)
```

**Per-layer protos** — each layer owns its service definition:

| Layer | Proto file |
|-------|-----------|
| fabric | `layers/fabric/proto/fabric.proto` |
| forge | `layers/forge/proto/forge.proto` |
| compute | `layers/compute/proto/compute.proto` |
| overlay | `layers/overlay/proto/overlay.proto` |
| storage | `layers/storage/proto/storage.proto` |
| org | `layers/org/proto/org.proto` |

**Shared types** — `api/proto/syfrah/v1/common.proto` contains cross-layer types:
`PaginationRequest`, `PaginationResponse`, `HealthStatus`, `Error`, and `Empty`.

**Google API protos** — vendored in two locations for import resolution:
- `api/proto/google/api/` (annotations.proto, http.proto)
- `layers/{layer}/proto/google/api/` (per-layer copies)

---

## 2. Package Conventions

Every per-layer proto file declares its package as `syfrah.{layer}.v1`:

```protobuf
package syfrah.fabric.v1;   // layers/fabric/proto/fabric.proto
package syfrah.compute.v1;   // layers/compute/proto/compute.proto
package syfrah.overlay.v1;   // layers/overlay/proto/overlay.proto
```

Shared types in `api/proto/syfrah/v1/common.proto` use `package syfrah.v1`.

The version number in the package name (`v1`) corresponds to the API version.

**Service naming** follows the pattern `{Layer}Service`:

```protobuf
service FabricService { ... }
service ForgeService  { ... }
service ComputeService { ... }
```

**Go package option** mirrors the package path:

```protobuf
option go_package = "github.com/sacha-ops/syfrah/gen/go/syfrah/v1;syfrahv1";
```

**Imports** — layer protos import shared types from their canonical location:

```protobuf
import "syfrah/v1/common.proto";
import "google/protobuf/timestamp.proto";
import "google/api/annotations.proto";
```

---

## 3. HTTP Annotations

Every RPC gets a `google.api.http` annotation that defines its REST mapping.

### Rules

- **GET** for reads (list, get, status, topology, metrics, events, audit).
- **POST** for mutations (remove, update, create, start, stop, accept, reject, rotate, reload).
- **Path prefix:** `/v1/{layer}/...` — the version and layer name appear in the URL path.
- **Body:** mutations use `body: "*"` to accept the full request message as JSON.

### Examples from `fabric.proto`

```protobuf
rpc ListPeers(ListPeersRequest) returns (ListPeersResponse) {
  option (google.api.http) = { get: "/v1/fabric/peers" };
}

rpc RemovePeer(RemovePeerRequest) returns (RemovePeerResponse) {
  option (google.api.http) = { post: "/v1/fabric/peers/remove" body: "*" };
}

rpc UpdatePeerEndpoint(UpdatePeerEndpointRequest) returns (UpdatePeerEndpointResponse) {
  option (google.api.http) = { post: "/v1/fabric/peers/update-endpoint" body: "*" };
}
```

### Path conventions

| Pattern | Example | Use case |
|---------|---------|----------|
| `/v1/{layer}/{resource}` | `/v1/fabric/peers` | List resources |
| `/v1/{layer}/{resource}/{action}` | `/v1/fabric/peers/remove` | Mutate a resource |
| `/v1/{layer}/{category}/{action}` | `/v1/fabric/peering/start` | Lifecycle operations |
| `/v1/{layer}/{noun}` | `/v1/fabric/topology` | Singleton reads |

---

## 4. Message Conventions

### Naming

- **Request:** `{Verb}{Resource}Request` — e.g. `ListPeersRequest`, `RemovePeerRequest`
- **Response:** `{Verb}{Resource}Response` — e.g. `ListPeersResponse`, `RemovePeerResponse`
- **Domain objects:** plain nouns — e.g. `Peer`, `TopologyEdge`, `FabricEvent`, `AuditEntry`

### Shared types from `common.proto`

Use these instead of defining your own:

| Type | Purpose |
|------|---------|
| `PaginationRequest` | Cursor-based pagination input (`page_size`, `page_token`) |
| `PaginationResponse` | Pagination output (`next_page_token`) |
| `HealthStatus` | Enum: `UNSPECIFIED`, `HEALTHY`, `DEGRADED`, `UNHEALTHY` |
| `Error` | Structured error with `code` and `message` |
| `Empty` | Placeholder for RPCs with no payload |

### Field conventions

- Use `proto3` syntax (all fields optional by default on the wire).
- Mark truly optional fields with `optional` keyword for clarity.
- Use `repeated` for lists.
- Prefer `google.protobuf.Timestamp` for time fields.
- Proto field numbers are permanent — never reuse a number. Reserve removed fields.

### Example

```protobuf
message Peer {
  string name = 1;
  string public_key = 2;
  string endpoint = 3;
  string ipv6_address = 4;
  uint32 wg_listen_port = 5;
  HealthStatus status = 6;
  google.protobuf.Timestamp last_handshake = 7;
  optional string region = 8;
  optional string zone = 9;
}

message ListPeersRequest {
  PaginationRequest pagination = 1;
}

message ListPeersResponse {
  repeated Peer peers = 1;
  PaginationResponse pagination = 2;
}
```

---

## 5. Generation Pipeline

Proto files are the single source of truth. The pipeline generates OpenAPI specs
and interactive documentation from them:

```
layers/{layer}/proto/{layer}.proto          ← source of truth
        │
        ▼
scripts/gen-openapi.py                      ← parses proto + HTTP annotations
        │
        ▼
layers/{layer}/openapi.yaml                 ← per-layer OpenAPI 3.0 spec (generated)
        │
        ▼
scripts/gen-openapi.sh                      ← merges all layer specs
        │
        ├──▶ api/openapi.yaml               ← merged spec for all layers
        │
        └──▶ docs/dist/api/index.html       ← Scalar interactive API docs
             docs/dist/api/openapi.yaml        (served on GitHub Pages)
```

### How it works

1. **`scripts/gen-openapi.py`** reads each `layers/{layer}/proto/{layer}.proto`,
   extracts the service name, RPCs, HTTP annotations, request/response messages,
   and enums. It writes an OpenAPI 3.0 YAML file to `layers/{layer}/openapi.yaml`.
   Stub layers (those with only a `GetStatus` RPC) get a minimal 501 spec.

2. **`scripts/gen-openapi.sh`** orchestrates the full pipeline:
   - Calls `gen-openapi.py` to generate per-layer specs.
   - Merges all `layers/*/openapi.yaml` files into `api/openapi.yaml`.
   - Generates a Scalar HTML page at `docs/dist/api/index.html`.
   - Copies the merged spec to `docs/dist/api/openapi.yaml`.

3. **CI** runs `gen-openapi.sh` automatically. The Scalar docs are deployed to
   GitHub Pages and provide an interactive API reference grouped by layer.

### Running locally

```bash
bash scripts/gen-openapi.sh
# Output:
#   layers/*/openapi.yaml        (per-layer specs)
#   api/openapi.yaml             (merged spec)
#   docs/dist/api/index.html     (interactive docs)
```

---

## 6. Versioning Strategy

### Within a version (non-breaking)

Proto field numbers provide wire compatibility within a package version:

- **Adding fields:** assign a new field number. Old clients ignore unknown fields.
- **Removing fields:** mark the number as `reserved`. Old clients get default values.
- **Adding RPCs:** new endpoints appear; existing clients are unaffected.

These changes are all non-breaking and require no version bump.

### Breaking changes

When a breaking change is unavoidable:

1. Create a new package: `syfrah.{layer}.v2` (reflected in the proto file and URL paths).
2. Serve both `v1` and `v2` simultaneously for at least one release cycle.
3. Announce deprecation of the old version.
4. After two minor releases, remove the old version.

### CI enforcement

- `buf lint` runs on every PR to enforce naming and style.
- `buf breaking` compares against the previous release tag and rejects accidental
  breaking changes within a package version.

### Response headers

Every API response includes an `API-Version` header so clients can detect
version incompatibility and produce clear error messages.

---

## 7. Adding a New Endpoint (Step by Step)

Use this checklist when adding an RPC to an existing layer.

### 1. Define the RPC

Open `layers/{layer}/proto/{layer}.proto` and add the RPC to the service block
with its HTTP annotation:

```protobuf
rpc GetWidget(GetWidgetRequest) returns (GetWidgetResponse) {
  option (google.api.http) = { get: "/v1/{layer}/widgets/{name}" };
}
```

### 2. Define request and response messages

In the same proto file, add the message types following the naming conventions:

```protobuf
message GetWidgetRequest {
  string name = 1;
}

message GetWidgetResponse {
  string name = 1;
  string kind = 2;
  google.protobuf.Timestamp created_at = 3;
}
```

Use shared types from `common.proto` where applicable (pagination, health status).

### 3. Verify the OpenAPI spec generates correctly

```bash
bash scripts/gen-openapi.sh
```

Check `layers/{layer}/openapi.yaml` to confirm the new endpoint appears with the
correct HTTP method, path, request body, and response schema.

### 4. Implement the handler

Add the handler in the layer's Rust code. The RPC name maps to a handler function
in the layer's gRPC service implementation.

### 5. Add the CLI command

Add a corresponding CLI subcommand in `layers/{layer}/src/cli/` that maps to the
new RPC.

### 6. Push and let CI verify

CI auto-generates the OpenAPI docs and runs `buf lint` + `buf breaking` checks.
The new endpoint will appear in the Scalar interactive docs after merge.

---

## 8. Adding a New Layer's API

Use this checklist when creating the proto for an entirely new layer.

### 1. Create the proto file

```bash
mkdir -p layers/{name}/proto/google/api
cp api/proto/google/api/annotations.proto layers/{name}/proto/google/api/
cp api/proto/google/api/http.proto layers/{name}/proto/google/api/
```

Create `layers/{name}/proto/{name}.proto`:

```protobuf
syntax = "proto3";

package syfrah.{name}.v1;

option go_package = "github.com/sacha-ops/syfrah/gen/go/syfrah/v1;syfrahv1";

import "syfrah/v1/common.proto";
import "google/protobuf/timestamp.proto";
import "google/api/annotations.proto";

// {Name}Service exposes the syfrah {name} control plane.
service {Name}Service {
  rpc GetStatus(GetStatusRequest) returns (GetStatusResponse) {
    option (google.api.http) = { get: "/v1/{name}/status" };
  }
}

message GetStatusRequest {}

message GetStatusResponse {
  HealthStatus status = 1;
}
```

### 2. Define RPCs with HTTP annotations

Add RPCs following the conventions in sections 3 and 4. Every RPC needs a
`google.api.http` annotation.

### 3. Run the generation pipeline

```bash
bash scripts/gen-openapi.sh
```

The pipeline auto-discovers all `layers/*/proto/*.proto` files. Your new layer
will appear in:
- `layers/{name}/openapi.yaml` (per-layer spec)
- `api/openapi.yaml` (merged spec)
- `docs/dist/api/` (Scalar interactive docs, grouped by layer name)

### 4. Verify in Scalar

Open `docs/dist/api/index.html` in a browser. The new layer should appear as a
tag group in the sidebar with all its endpoints listed.

No manual registration is needed — the pipeline picks up any layer that has a
proto file in the expected location.
