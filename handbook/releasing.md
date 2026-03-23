# Releasing

## Overview

Syfrah publishes pre-compiled binaries for every tagged release. Pushing a `v*` tag triggers the release workflow, which builds static binaries for four targets and creates a GitHub Release with the archives and SHA256 checksums.

## Targets

| Target | OS | Arch | Build method |
|---|---|---|---|
| `x86_64-unknown-linux-musl` | Linux | amd64 | `cargo build` with musl-tools |
| `aarch64-unknown-linux-musl` | Linux | arm64 | `cross build` |
| `x86_64-apple-darwin` | macOS | amd64 | `cargo build` (native) |
| `aarch64-apple-darwin` | macOS | arm64 | `cargo build` (native) |

Linux binaries are statically linked via musl so they run on any Linux distribution with no runtime dependencies.

## How to cut a release

1. Make sure `main` is green (all CI checks pass).
2. Choose a version following [semver](https://semver.org/). Current: `0.1.0`.
3. Tag and push:

```bash
git tag v0.1.0
git push origin v0.1.0
```

4. The `Release` workflow (`.github/workflows/release.yml`) runs automatically:
   - Builds all four targets in parallel
   - Packages each binary into a `.tar.gz` archive
   - Generates `SHA256SUMS.txt`
   - Creates a GitHub Release with auto-generated release notes and all artifacts attached

5. Verify the release at `https://github.com/sifrah/syfrah/releases`.

## Artifacts

Each release contains:

```
syfrah-v0.1.0-x86_64-unknown-linux-musl.tar.gz
syfrah-v0.1.0-aarch64-unknown-linux-musl.tar.gz
syfrah-v0.1.0-x86_64-apple-darwin.tar.gz
syfrah-v0.1.0-aarch64-apple-darwin.tar.gz
SHA256SUMS.txt
```

## Verifying a download

```bash
sha256sum -c SHA256SUMS.txt
```
