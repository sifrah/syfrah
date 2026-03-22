#!/usr/bin/env bash
set -euo pipefail

# E2E test runner: builds the Docker image, discovers and runs all scenarios.
#
# Usage:
#   ./tests/e2e/run.sh                  # run all scenarios
#   ./tests/e2e/run.sh 01_mesh          # run scenarios matching "01_mesh"
#
# Each scenario in scenarios/ is an independent test that creates its own
# containers, runs assertions, and cleans up.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FILTER="${1:-}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BOLD='\033[1m'
NC='\033[0m'

# ── Build Docker image ────────────────────────────────────────

echo -e "${BOLD}═══════════════════════════════════════${NC}"
echo -e "${BOLD}  Syfrah E2E Tests${NC}"
echo -e "${BOLD}═══════════════════════════════════════${NC}"
echo ""

echo -e "${YELLOW}→ Building Docker image...${NC}"
cd "$REPO_ROOT"
docker build -t syfrah-e2e-test -f tests/e2e/Dockerfile . --quiet
echo ""

# ── Create shared network ─────────────────────────────────────

docker network create syfrah-e2e --subnet 172.20.0.0/24 --driver bridge >/dev/null 2>&1 || true

# ── Discover and run scenarios ────────────────────────────────

SCENARIOS=()
for f in "$SCRIPT_DIR/scenarios/"*.sh; do
    [ -f "$f" ] || continue
    if [ -n "$FILTER" ]; then
        if ! echo "$f" | grep -q "$FILTER"; then
            continue
        fi
    fi
    SCENARIOS+=("$f")
done

if [ ${#SCENARIOS[@]} -eq 0 ]; then
    echo "No scenarios found${FILTER:+ matching '$FILTER'}."
    exit 1
fi

echo -e "Found ${#SCENARIOS[@]} scenario(s):"
for s in "${SCENARIOS[@]}"; do
    echo "  $(basename "$s")"
done
echo ""

TOTAL_PASS=0
TOTAL_FAIL=0
RESULTS=()

for scenario in "${SCENARIOS[@]}"; do
    name="$(basename "$scenario" .sh)"

    echo -e "${BOLD}── $name ────────────────────────────${NC}"

    if bash "$scenario"; then
        RESULTS+=("${GREEN}✓ $name${NC}")
        TOTAL_PASS=$((TOTAL_PASS + 1))
    else
        RESULTS+=("${RED}✗ $name${NC}")
        TOTAL_FAIL=$((TOTAL_FAIL + 1))
    fi

    echo ""
done

# ── Cleanup shared network ────────────────────────────────────

docker network rm syfrah-e2e >/dev/null 2>&1 || true

# ── Summary ───────────────────────────────────────────────────

TOTAL=$((TOTAL_PASS + TOTAL_FAIL))

echo -e "${BOLD}═══════════════════════════════════════${NC}"
echo -e "${BOLD}  Results${NC}"
echo -e "${BOLD}═══════════════════════════════════════${NC}"
echo ""

for r in "${RESULTS[@]}"; do
    echo -e "  $r"
done

echo ""

if [ "$TOTAL_FAIL" -eq 0 ]; then
    echo -e "  ${GREEN}${BOLD}$TOTAL/$TOTAL scenarios passed${NC}"
    echo ""
    exit 0
else
    echo -e "  ${RED}${BOLD}$TOTAL_FAIL/$TOTAL scenarios failed${NC}"
    echo ""
    exit 1
fi
