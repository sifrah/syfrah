# Releasing

## Versioning

Syfrah follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). All crates in the workspace share a single version defined in the root `Cargo.toml` under `[workspace.package]`. Individual crates inherit the version with `version.workspace = true`.

## Version bumps

To bump the version, update the single source of truth:

```toml
# Cargo.toml (root)
[workspace.package]
version = "0.2.0"
```

All crates pick up the new version automatically.

## Release checklist

1. **Update CHANGELOG.md** — move items from `Unreleased` to a new version section with today's date.
2. **Bump version** — edit `version` in `[workspace.package]` in the root `Cargo.toml`.
3. **Run CI locally** — `just ci` (fmt + clippy + test).
4. **Commit** — `git commit -m "Release v0.x.y"`.
5. **Tag** — `git tag -a v0.x.y -m "v0.x.y"`.
6. **Push** — `git push origin main --follow-tags`.
7. **Verify CI** — wait for all checks to pass on the tagged commit.
8. **Create GitHub release** — `gh release create v0.x.y --notes-from-tag` or write release notes from the CHANGELOG section.

## Changelog format

The project uses [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Every user-facing change should be recorded under one of: `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`.

## Pre-release versions

For pre-release builds, use SemVer pre-release identifiers: `0.2.0-alpha.1`, `0.2.0-rc.1`.

## crates.io (future)

All crates include the required crates.io metadata (`description`, `license`, `repository`, `keywords`, `categories`). When the project is ready for publishing, run:

```bash
cargo publish -p syfrah-core
cargo publish -p syfrah-state
cargo publish -p syfrah-fabric
cargo publish -p syfrah-bin
```

Publish in dependency order: core first, then state, fabric, and finally the binary.
