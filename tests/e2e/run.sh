#!/usr/bin/env bash
set -euo pipefail

# E2E test runner: discovers and runs scenarios by layer.
#
# Usage:
#   ./tests/e2e/run.sh                  # run all scenarios
#   ./tests/e2e/run.sh fabric           # run only fabric scenarios
#   ./tests/e2e/run.sh compute          # run only compute scenarios
#   ./tests/e2e/run.sh 01_fabric        # run scenarios matching "01_fabric"
#
# The filter matches against the scenario filename.
# "fabric" matches all *_fabric_*.sh files.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FILTER="${1:-}"
SKIP_BUILD="${SKIP_BUILD:-}"

# Network isolation: in DinD each job has its own Docker daemon, so fixed names are safe.
# For parallel local runs, E2E_RUN_ID can be overridden.
E2E_RUN_ID="${E2E_RUN_ID:-$$}"
export E2E_NETWORK="syfrah-e2e-${E2E_RUN_ID}"
export E2E_SUBNET="172.20.0.0/24"
export E2E_IP_PREFIX="172.20.0"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BOLD='\033[1m'
NC='\033[0m'

# ── Build Docker image (unless SKIP_BUILD is set) ─────────────

echo -e "${BOLD}═══════════════════════════════════════${NC}"
echo -e "${BOLD}  Syfrah E2E Tests${NC}"
if [ -n "$FILTER" ]; then
    echo -e "${BOLD}  Filter: ${FILTER}${NC}"
fi
echo -e "${BOLD}═══════════════════════════════════════${NC}"
echo ""

if [ -z "$SKIP_BUILD" ]; then
    echo -e "${YELLOW}→ Building Docker image...${NC}"
    cd "$REPO_ROOT"
    docker build -t syfrah-e2e-test -f tests/e2e/Dockerfile . --quiet
    echo ""
else
    echo -e "${YELLOW}→ Skipping Docker build (SKIP_BUILD set)${NC}"
    echo ""
fi

# ── Clean up stale containers and network from previous runs ──

for cid in $(docker ps -aq --filter network="$E2E_NETWORK" 2>/dev/null); do
    docker rm -f "$cid" >/dev/null 2>&1 || true
done
docker network rm "$E2E_NETWORK" >/dev/null 2>&1 || true

# ── Create shared network ─────────────────────────────────────

docker network create "$E2E_NETWORK" --subnet 172.20.0.0/24 --driver bridge >/dev/null 2>&1 || true

# ── Discover scenarios ────────────────────────────────────────

SCENARIOS=()
for f in "$SCRIPT_DIR/scenarios/"*.sh; do
    [ -f "$f" ] || continue
    if [ -n "$FILTER" ]; then
        if ! basename "$f" | grep -q "$FILTER"; then
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

SCENARIO_INDEX=0
for scenario in "${SCENARIOS[@]}"; do
    SCENARIO_INDEX=$((SCENARIO_INDEX + 1))
    name="$(basename "$scenario" .sh)"

    echo -e "${BOLD}── [$SCENARIO_INDEX/${#SCENARIOS[@]}] $name ────────────────────────────${NC}"
    SCENARIO_START=$(date +%s)

    if bash "$scenario"; then
        SCENARIO_TIME=$(( $(date +%s) - SCENARIO_START ))
        RESULTS+=("${GREEN}✓ $name (${SCENARIO_TIME}s)${NC}")
        TOTAL_PASS=$((TOTAL_PASS + 1))
    else
        SCENARIO_TIME=$(( $(date +%s) - SCENARIO_START ))
        RESULTS+=("${RED}✗ $name (${SCENARIO_TIME}s)${NC}")
        TOTAL_FAIL=$((TOTAL_FAIL + 1))
    fi

    echo ""
done

# ── Cleanup shared network ────────────────────────────────────

docker network rm "$E2E_NETWORK" >/dev/null 2>&1 || true

# ── Summary ───────────────────────────────────────────────────

TOTAL=$((TOTAL_PASS + TOTAL_FAIL))

echo -e "${BOLD}═══════════════════════════════════════${NC}"
echo -e "${BOLD}  Results${FILTER:+ ($FILTER)}${NC}"
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
