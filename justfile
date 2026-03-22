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
    ./site/build.sh

# Serve documentation locally (requires python3)
docs-serve: docs
    cd site/dist && python3 -m http.server 8080
