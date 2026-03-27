# SDK Generation

Syfrah exposes gRPC APIs defined as Protobuf files in `api/proto/`. Language-specific
SDKs are generated with [Buf](https://buf.build/) and committed to the repository
under `sdk/`.

## Output directories

| Language  | Directory    | Plugin(s)                                      |
|-----------|--------------|-------------------------------------------------|
| Go        | `sdk/go/`    | `protocolbuffers/go`, `grpc/go`                |
| Rust      | `sdk/rust/`  | `protocolbuffers/rust`                         |
| Python    | `sdk/python/`| `protocolbuffers/python`, `grpc/python`        |
| JS/TS     | `sdk/js/`    | `protocolbuffers/es`, `connectrpc/es`          |

## Prerequisites

Install the Buf CLI:

```bash
# macOS / Linux
brew install bufbuild/buf/buf

# or via npm
npm install -g @bufbuild/buf
```

## Generating SDKs locally

From the repository root:

```bash
buf generate
```

This reads `buf.gen.yaml`, finds the proto files referenced by `buf.yaml`, and
writes generated code into the `sdk/` sub-directories listed above.

**Always commit the regenerated files.** CI will fail if the committed SDK code
is stale compared to the proto definitions.

## CI pipeline

The workflow `.github/workflows/sdk.yml` runs on every push or PR that touches
`api/proto/**`, `buf.gen.yaml`, or `buf.yaml`.

1. **generate** job — runs `buf generate` and verifies no diff in `sdk/`.
   If stale, the job fails with a clear error message.
2. **publish** job (main branch only) — publishes packages to their registries.
   Publishing is gated behind secrets (`PYPI_TOKEN`, `NPM_TOKEN`) and disabled
   by default until those secrets are configured.

## Adding a new language

1. Add the appropriate Buf plugin to `buf.gen.yaml` with `out: sdk/<lang>`.
2. Create `sdk/<lang>/README.md`.
3. Run `buf generate` and commit the output.
4. Update this document.
