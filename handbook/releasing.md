# Releasing

## Versioning

Syfrah follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). All crates in the workspace share a single version defined in the root `Cargo.toml` under `[workspace.package]`. Individual crates inherit the version with `version.workspace = true`.

The **release workflow is the source of truth** for version numbers. It computes the next version from git tags and commit messages, then stamps it into the build via the `SYFRAH_VERSION` env var. `Cargo.toml` is not required to match — the workflow overrides the compiled-in version at build time. The version in `Cargo.toml` is only used for local `cargo build` during development.

## Release channels

Syfrah uses two release channels:

| Channel | Branch | Tag format | GitHub Release | Purpose |
|---|---|---|---|---|
| **Beta** | `main` | `vX.Y.Z-beta.N` | Pre-release | Continuous delivery of latest changes for testing |
| **Stable** | `release/vX.Y` | `vX.Y.Z` | Latest release | Production-ready, explicitly cut by a maintainer |

### Beta (main)

Every merge to `main` automatically produces a **pre-release** on GitHub. These are tagged `vX.Y.Z-beta.N` where N auto-increments. Beta releases are not installed by default — users must opt in.

**How N is computed:** the workflow lists all existing `vX.Y.Z-beta.*` tags for the current minor version and picks `max(N) + 1`. If no beta tag exists yet for this minor, N starts at 1. Example: if `v0.3.0-beta.4` is the latest tag and the next push to main is a patch, the new tag is `v0.3.1-beta.1`. If it's still the same computed version, it becomes `v0.3.0-beta.5`.

### Stable (release branch)

When a set of changes is ready for production, a maintainer creates or updates a `release/vX.Y` branch from `main`. Pushing to this branch triggers a **stable release** tagged `vX.Y.Z`. Stable releases are what `install.sh` downloads by default.

### Release flow

```
feature branch → PR → merge to main → beta vX.Y.Z-beta.N (pre-release)
                                         ↓
                              when ready for prod:
                              merge main → release/vX.Y → stable vX.Y.Z
```

## Automated releases

### How it works

1. **Trigger** -- the `Release` workflow runs on push to `main` (beta) and push to `release/v*` (stable), plus `workflow_dispatch`.
2. **Loop prevention** -- if the HEAD commit message starts with `release:`, the workflow exits immediately.
3. **Version calculation** -- the workflow inspects commit messages since the last git tag and determines the base version:

   | Commit message contains | Bump | Example |
   |--------------------------|------|---------|
   | `BREAKING` or `breaking:` | Major (X.0.0) | `0.2.0 -> 1.0.0` |
   | `[Feature]` or `feat:` | Minor (0.X.0) | `0.2.0 -> 0.3.0` |
   | Anything else | Patch (0.0.X) | `0.2.0 -> 0.2.1` |

   This convention relies on contributors following [Conventional Commits](https://www.conventionalcommits.org/). If nobody writes `feat:` or `breaking:`, every release is a patch bump. This is intentional — patch is the safe default. For important version bumps, the maintainer cutting the release should verify the computed version makes sense before pushing.

4. **Channel detection**:
   - Push to `main` → append `-beta.N` suffix (auto-incremented), mark GitHub Release as `prerelease: true`
   - Push to `release/v*` → stable version (no suffix), mark as `latest`
5. **Build** -- all four targets are built in parallel. The version is injected via `SYFRAH_VERSION` env var at compile time.
6. **Release** -- binaries are packaged into `.tar.gz` archives, `SHA256SUMS.txt` is generated, a git tag is created, and a GitHub Release is published with all artifacts.

### Manual release

You can also trigger a release manually via the GitHub Actions UI using `workflow_dispatch`. This uses the same auto-increment logic.

## Cutting a stable release

Only the project maintainer cuts stable releases. The decision is based on:
- All planned features for the minor version are merged and tested
- No known P0/P1 bugs open
- E2E tests passing on main

```bash
# Create the release branch from main (first time for vX.Y)
git checkout main && git pull
git checkout -b release/v0.3
git push -u origin release/v0.3
# → triggers stable release v0.3.0
```

### Hotfixes

For critical fixes on an already-released stable version:

```bash
# Cherry-pick the fix onto the release branch
git checkout release/v0.3
git cherry-pick <fix-commit-sha>
git push
# → triggers stable release v0.3.1
```

Do NOT merge all of main into a release branch for a hotfix. Cherry-pick only the specific fix to avoid shipping untested changes.

### Patch releases

For planned patch releases (batching several fixes):

```bash
git checkout release/v0.3
git cherry-pick <sha1> <sha2> <sha3>
git push
# → triggers stable release v0.3.1 (or .2, .3, etc.)
```

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
syfrah-vX.Y.Z-{target}.tar.gz        # stable
syfrah-vX.Y.Z-beta.N-{target}.tar.gz # beta
SHA256SUMS.txt
install.sh
```

## Installing

```bash
# Stable (default)
curl -fsSL https://get.syfrah.dev | bash

# Beta
curl -fsSL https://get.syfrah.dev | bash -s -- --beta
```

## Verifying a download

```bash
sha256sum -c SHA256SUMS.txt
```

## SDK and API versioning (planned)

Not yet implemented. When SDKs are published, they will follow the same beta/stable channel model:

| Component | Beta (main) | Stable (release/*) | Status |
|---|---|---|---|
| Go SDK | `sdk/go/vX.Y.Z-beta.N` tag | `sdk/go/vX.Y.Z` tag | Not published yet |
| Python SDK | PyPI `syfrah==X.Y.ZbN` | PyPI `syfrah==X.Y.Z` | Not published yet |
| JS/TS SDK | npm `@syfrah/sdk@X.Y.Z-beta.N` | npm `@syfrah/sdk@X.Y.Z` | Not published yet |
| Docker image | `ghcr.io/sacha-ops/syfrah:beta` | `ghcr.io/sacha-ops/syfrah:latest` + `:vX.Y.Z` | Not published yet |
| OpenAPI spec | Bundled in beta release | Bundled in stable release | Generated, not versioned separately |

## crates.io (future)

All crates include the required crates.io metadata (`description`, `license`, `repository`, `keywords`, `categories`). When the project is ready for publishing, run:

```bash
cargo publish -p syfrah-core
cargo publish -p syfrah-state
cargo publish -p syfrah-fabric
cargo publish -p syfrah-bin
```

Publish in dependency order: core first, then state, fabric, and finally the binary.
