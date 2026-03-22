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

# Build documentation site
docs:
    cd documentation && npm run build

# Serve documentation locally
docs-serve:
    cd documentation && npm run dev
