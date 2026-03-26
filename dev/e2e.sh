#!/usr/bin/env bash
set -euo pipefail

# Run E2E tests locally using a volume-mounted binary instead of baking it
# into the Docker image. This avoids the ~3 min Docker rebuild on each code
# change — just `cargo build` and re-run.
#
# Usage:
#   ./dev/e2e.sh                     # run all scenarios
#   ./dev/e2e.sh fabric              # run only fabric scenarios
#   ./dev/e2e.sh 01_fabric           # run scenarios matching "01_fabric"
#   ./dev/e2e.sh --help              # show this help
#
# Workflow:
#   cargo build --release --target x86_64-unknown-linux-musl && ./dev/e2e.sh fabric

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# ── Colors ────────────────────────────────────────────────────

BOLD='\033[1m'
YELLOW='\033[0;33m'
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

# ── Help ──────────────────────────────────────────────────────

show_help() {
    cat <<'EOF'
dev/e2e.sh — Run E2E integration tests locally (no Docker rebuild)

USAGE
  ./dev/e2e.sh [FILTER]       Run scenarios (optionally filtered by name)
  ./dev/e2e.sh --help         Show this help

FILTER EXAMPLES
  ./dev/e2e.sh                Run all scenarios
  ./dev/e2e.sh fabric         Run all *fabric* scenarios
  ./dev/e2e.sh 01_fabric      Run scenario 01_fabric_mesh_formation
  ./dev/e2e.sh ux             Run all *ux* scenarios
  ./dev/e2e.sh state          Run all *state* scenarios

HOW IT WORKS
  1. Builds a lightweight Docker image (no Rust compilation)
  2. Sets E2E_BINARY_MOUNT so lib.sh volume-mounts the local binary
  3. Delegates to tests/e2e/run.sh with SKIP_BUILD=1

PREREQUISITES
  - Docker
  - WireGuard kernel module (sudo modprobe wireguard)
  - A compiled syfrah binary (static musl build):
      cargo build --release --target x86_64-unknown-linux-musl

    Or for faster debug builds:
      cargo build --target x86_64-unknown-linux-musl

ENVIRONMENT VARIABLES
  E2E_BINARY   Override path to the syfrah binary
                Default: target/x86_64-unknown-linux-musl/release/syfrah
                Fallback: target/x86_64-unknown-linux-musl/debug/syfrah
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
    show_help
    exit 0
fi

# ── Cleanup on Ctrl+C ────────────────────────────────────────

cleanup() {
    echo ""
    echo -e "${YELLOW}Interrupted — cleaning up containers...${NC}"
    # Let run.sh's own cleanup handle containers; just remove the network
    docker network rm syfrah-e2e >/dev/null 2>&1 || true
}
trap cleanup EXIT

# ── Locate binary ────────────────────────────────────────────

if [ -n "${E2E_BINARY:-}" ]; then
    BINARY="$E2E_BINARY"
elif [ -f "$REPO_ROOT/target/x86_64-unknown-linux-musl/release/syfrah" ]; then
    BINARY="$REPO_ROOT/target/x86_64-unknown-linux-musl/release/syfrah"
elif [ -f "$REPO_ROOT/target/x86_64-unknown-linux-musl/debug/syfrah" ]; then
    BINARY="$REPO_ROOT/target/x86_64-unknown-linux-musl/debug/syfrah"
else
    echo -e "${RED}ERROR: No syfrah binary found.${NC}"
    echo ""
    echo "Build it first:"
    echo "  cargo build --release --target x86_64-unknown-linux-musl"
    echo ""
    echo "Or set E2E_BINARY to point to an existing binary."
    exit 1
fi

# Verify it's a static binary (musl) — the E2E containers are minimal
if file "$BINARY" | grep -q "dynamically linked"; then
    echo -e "${YELLOW}WARNING: Binary appears to be dynamically linked.${NC}"
    echo "E2E containers may not have the required shared libraries."
    echo "Consider building with: cargo build --release --target x86_64-unknown-linux-musl"
    echo ""
fi

BINARY="$(cd "$(dirname "$BINARY")" && pwd)/$(basename "$BINARY")"

# ── Build lightweight E2E image (no compilation) ─────────────

echo -e "${BOLD}==========================================${NC}"
echo -e "${BOLD}  Syfrah E2E Tests (LOCAL mode)${NC}"
echo -e "${BOLD}==========================================${NC}"
echo ""
echo -e "  Binary: ${GREEN}${BINARY}${NC}"
echo -e "  Mode:   ${GREEN}LOCAL${NC} (volume-mounted, no Docker rebuild)"
echo ""

echo -e "${YELLOW}-> Building lightweight E2E base image...${NC}"

# Build a minimal image without the syfrah binary baked in.
# The binary will be volume-mounted at runtime by lib.sh.
docker build -t syfrah-e2e-test -f - "$REPO_ROOT" --quiet <<'DOCKERFILE'
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    wireguard-tools \
    iproute2 \
    iputils-ping \
    procps \
    iptables \
    jq \
    ncat \
    tini \
    && rm -rf /var/lib/apt/lists/*
CMD ["sleep", "infinity"]
DOCKERFILE

echo ""

# ── Run E2E via the standard runner ──────────────────────────

export E2E_BINARY_MOUNT="$BINARY"
export SKIP_BUILD=1

exec "$REPO_ROOT/tests/e2e/run.sh" "${1:-}"
