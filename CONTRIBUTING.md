# Contributing to Syfrah

Thank you for your interest in contributing.

## Getting started

1. Fork the repository
2. Create a branch from `main`
3. Make your changes
4. Run the checks: `just ci` (or manually: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test`)
5. Open a pull request

## Development setup

```bash
# Clone your fork
git clone https://github.com/YOUR_USER/syfrah.git
cd syfrah

# Install the Rust toolchain (version is pinned in rust-toolchain.toml)
rustup show

# Build and test
cargo build
cargo test
```

## Code conventions

- **Formatting**: `cargo fmt` — enforced by CI
- **Linting**: `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings allowed
- **Tests**: `cargo test` — all tests must pass
- **Error handling**: `thiserror` for library crates, `anyhow` for the binary
- **Serialization**: `serde` Serialize/Deserialize on all public types
- **Async**: tokio runtime

## Repository structure

The repo is organized by architectural layer. See [handbook/repository.md](handbook/repository.md) for conventions.

- `layers/core/` — foundation types (no I/O, no async)
- `layers/fabric/` — WireGuard mesh (implemented)
- `layers/{other}/` — future layers (README only)
- `bin/syfrah/` — CLI binary (composes all layers)

When adding code, put it in the right layer. When adding a CLI command, put it in `layers/{layer}/src/cli/`.

## Pull request checklist

- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test` passes
- [ ] New code has tests where applicable
- [ ] Documentation updated if behavior changed

## Commit messages

- Use imperative mood: "Add feature" not "Added feature"
- Keep the first line under 72 characters
- Reference issues when relevant: "Fix #123"

## Reporting bugs

Use the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md) or open a plain issue with:
- What you expected
- What happened
- Steps to reproduce
- `syfrah fabric status` output

## Feature requests

Open an issue with the [feature request template](.github/ISSUE_TEMPLATE/feature_request.md). Describe the use case, not just the solution.
