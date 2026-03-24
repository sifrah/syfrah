# Releasing

## Versioning

Syfrah follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). All crates in the workspace share a single version defined in the root `Cargo.toml` under `[workspace.package]`. Individual crates inherit the version with `version.workspace = true`.

## Automated releases

Every merge to `main` automatically produces a new GitHub Release. There is no manual version bump required.

### How it works

1. **Trigger** -- the `Release` workflow runs on every push to `main` and on `workflow_dispatch`.
2. **Loop prevention** -- if the HEAD commit message starts with `release:`, the workflow exits immediately. This prevents the version-bump commit from triggering another release.
3. **Version calculation** -- the workflow inspects commit messages since the last git tag and determines the next version:

   | Commit message contains | Bump | Example |
   |--------------------------|------|---------|
   | `BREAKING` or `breaking:` | Major (X.0.0) | `0.2.0 -> 1.0.0` |
   | `[Feature]` or `feat:` | Minor (0.X.0) | `0.2.0 -> 0.3.0` |
   | Anything else | Patch (0.0.X) | `0.2.0 -> 0.2.1` |

4. **CI gate** -- the workflow waits for CI checks to pass before proceeding.
5. **Cargo.toml bump** -- the workflow updates `version` in `[workspace.package]` in the root `Cargo.toml`, commits with `release: vX.Y.Z [skip ci]`, and pushes directly to `main`.
6. **Build** -- all four targets are built in parallel (same matrix as CI).
7. **Release** -- binaries are packaged into `.tar.gz` archives, `SHA256SUMS.txt` is generated, a git tag `vX.Y.Z` is created, and a GitHub Release is published with all artifacts.

### Influencing the version bump

To trigger a minor version bump, include `feat:` or `[Feature]` in at least one commit message in your PR:

```
feat: add mesh event log
```

To trigger a major version bump, include `BREAKING` or `breaking:` in a commit message:

```
breaking: remove legacy peering protocol
```

If no commit matches these patterns, the patch version is incremented.

### Manual release

You can also trigger a release manually via the GitHub Actions UI using `workflow_dispatch`. This uses the same auto-increment logic.

## Targets

| Target | OS | Arch | Build method |
|---|---|---|---|
| `x86_64-unknown-linux-musl` | Linux | amd64 | `cargo build` with musl-tools |
| `aarch64-unknown-linux-musl` | Linux | arm64 | `cross build` |
| `x86_64-apple-darwin` | macOS | amd64 | `cargo build` (native) |
| `aarch64-apple-darwin` | macOS | arm64 | `cargo build` (native) |

Linux binaries are statically linked via musl so they run on any Linux distribution with no runtime dependencies.

## Artifacts

Each release contains:

```
syfrah-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz
syfrah-vX.Y.Z-aarch64-unknown-linux-musl.tar.gz
syfrah-vX.Y.Z-x86_64-apple-darwin.tar.gz
syfrah-vX.Y.Z-aarch64-apple-darwin.tar.gz
SHA256SUMS.txt
install.sh
```

## Verifying a download

```bash
sha256sum -c SHA256SUMS.txt
```

## crates.io (future)

All crates include the required crates.io metadata (`description`, `license`, `repository`, `keywords`, `categories`). When the project is ready for publishing, run:

```bash
cargo publish -p syfrah-core
cargo publish -p syfrah-state
cargo publish -p syfrah-fabric
cargo publish -p syfrah-bin
```

Publish in dependency order: core first, then state, fabric, and finally the binary.
