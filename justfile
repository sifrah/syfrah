# Syfrah development commands
# Install just: cargo install just

# Run all CI checks (same as GitHub Actions)
ci: fmt-check clippy test

# Format check
fmt-check:
    cargo fmt --check

# Format fix
fmt:
    cargo fmt

# Lint
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Run tests
test:
    cargo test --workspace

# Build
build:
    cargo build --workspace

# Build release
release:
    cargo build --workspace --release

# Run security audit
audit:
    cargo audit

# Run the CLI
run *ARGS:
    cargo run --bin syfrah -- {{ARGS}}

# Run all E2E tests (requires Docker)
e2e:
    ./tests/e2e/run.sh

# Run E2E tests for a specific layer
e2e-layer LAYER:
    ./tests/e2e/run.sh {{LAYER}}

# Sync READMEs into documentation site pages
docs-sync:
    ./scripts/sync-docs.sh

# Build documentation site (sync + build)
docs: docs-sync
    cd documentation && npm run build

# Serve documentation locally (dev server)
docs-serve:
    cd documentation && npm run dev
