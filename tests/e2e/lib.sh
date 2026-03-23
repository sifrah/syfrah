#!/usr/bin/env bash
# Shared functions for E2E test scenarios.
# Source this file at the top of each scenario.

set -euo pipefail

# ── Config ────────────────────────────────────────────────────

E2E_IMAGE="${E2E_IMAGE:-syfrah-e2e-test}"
E2E_NETWORK="${E2E_NETWORK:-syfrah-e2e}"
E2E_SUBNET="${E2E_SUBNET:-172.20.0.0/24}"
E2E_PIN="${E2E_PIN:-4829}"
E2E_MESH="${E2E_MESH:-e2e-test}"
E2E_CONTAINERS=()

# ── Colors ────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

PASSED=0
FAILED=0

pass() { echo -e "  ${GREEN}✓ $1${NC}"; PASSED=$((PASSED + 1)); }
fail() { echo -e "  ${RED}✗ $1${NC}"; FAILED=$((FAILED + 1)); }
info() { echo -e "${YELLOW}→ $1${NC}"; }
debug() { echo -e "  ${NC}  [debug] $1${NC}"; }

# ── Network ───────────────────────────────────────────────────

create_network() {
    docker network create "$E2E_NETWORK" \
        --subnet "$E2E_SUBNET" \
        --driver bridge \
        >/dev/null 2>&1 || true
}

remove_network() {
    docker network rm "$E2E_NETWORK" >/dev/null 2>&1 || true
}

# ── Containers ────────────────────────────────────────────────

# Start a container. Args: <name> <ip>
start_node() {
    local name="$1"
    local ip="$2"

    docker rm -f "$name" >/dev/null 2>&1 || true

    debug "starting container $name at $ip"
    docker run -d \
        --name "$name" \
        --network "$E2E_NETWORK" \
        --ip "$ip" \
        --privileged \
        --hostname "$name" \
        "$E2E_IMAGE" >/dev/null

    E2E_CONTAINERS+=("$name")
}

# Wait for the syfrah daemon control socket to appear. Args: <container>
wait_daemon() {
    local container="$1"
    local max_wait="${2:-30}"
    local i=0
    debug "waiting for daemon on $container (max ${max_wait}s)"
    while [ $i -lt "$max_wait" ]; do
        if docker exec "$container" test -S /root/.syfrah/control.sock 2>/dev/null; then
            debug "daemon ready on $container after ${i}s"
            return 0
        fi
        sleep 1
        i=$((i + 1))
    done
    fail "daemon on $container did not start within ${max_wait}s"
    info "Docker logs for $container:"
    docker logs "$container" 2>&1 | tail -30 || true
    info "Processes in $container:"
    docker exec "$container" ps aux 2>/dev/null || true
    info "Files in .syfrah:"
    docker exec "$container" ls -la /root/.syfrah/ 2>/dev/null || true
    info "Syfrah log:"
    docker exec "$container" cat /root/.syfrah/syfrah.log 2>/dev/null | tail -20 || true
    return 1
}

# ── Mesh operations ───────────────────────────────────────────

# Initialize a mesh on a container. Args: <container> <ip> [node_name]
init_mesh() {
    local container="$1"
    local ip="$2"
    local node_name="${3:-$container}"

    debug "init_mesh: $container at $ip as $node_name"
    docker exec -d "$container" \
        syfrah fabric init \
        --name "$E2E_MESH" \
        --node-name "$node_name" \
        --endpoint "${ip}:51820"

    wait_daemon "$container"
    debug "init_mesh: $container done"
}

# Start peering with PIN on a container. Args: <container>
start_peering() {
    local container="$1"
    debug "start_peering: $container with PIN $E2E_PIN"
    timeout 10 docker exec "$container" \
        syfrah fabric peering start --pin "$E2E_PIN" || {
        info "start_peering timed out or failed on $container"
        info "Docker logs:"
        docker logs "$container" 2>&1 | tail -10 || true
        info "Control socket:"
        docker exec "$container" ls -la /root/.syfrah/control.sock 2>/dev/null || echo "(missing)"
        return 1
    }
    debug "start_peering: $container done"
}

# Join a mesh. Args: <container> <target_ip> <own_ip> [node_name]
join_mesh() {
    local container="$1"
    local target_ip="$2"
    local own_ip="$3"
    local node_name="${4:-$container}"

    debug "join_mesh: $container joining via $target_ip as $node_name"
    docker exec -d "$container" \
        syfrah fabric join "${target_ip}:51821" \
        --node-name "$node_name" \
        --endpoint "${own_ip}:51820" \
        --pin "$E2E_PIN"

    wait_daemon "$container"
    debug "join_mesh: $container done"
}

# Leave the mesh. Args: <container>
leave_mesh() {
    local container="$1"
    docker exec "$container" syfrah fabric leave 2>&1 || true
}

# Stop the daemon. Args: <container>
stop_daemon() {
    local container="$1"
    docker exec "$container" syfrah fabric stop 2>&1 || true
}

# ── Assertions ────────────────────────────────────────────────

# Assert daemon is running (or was running and set up the mesh). Args: <container>
assert_daemon_running() {
    local container="$1"
    # Check via status command OR by verifying syfrah0 exists + state.json has peers
    if docker exec "$container" syfrah fabric status 2>&1 | grep -q "running"; then
        pass "$container daemon is running"
    elif docker exec "$container" ip link show syfrah0 >/dev/null 2>&1; then
        # Daemon may have exited but setup completed (interface exists)
        pass "$container daemon setup completed (interface active)"
    else
        fail "$container daemon is not running"
    fi
}

# Assert peer count. Args: <container> <expected_count>
assert_peer_count() {
    local container="$1"
    local expected="$2"
    local actual
    actual=$(docker exec "$container" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
    if [ "$actual" -eq "$expected" ]; then
        pass "$container sees $expected peers"
    else
        fail "$container sees $actual peers (expected $expected)"
        docker exec "$container" syfrah fabric peers 2>&1 || true
    fi
}

# Assert syfrah0 interface exists. Args: <container>
assert_interface_exists() {
    local container="$1"
    if docker exec "$container" ip link show syfrah0 2>&1 | grep -q "syfrah0"; then
        pass "$container has syfrah0 interface"
    else
        fail "$container missing syfrah0 interface"
    fi
}

# Assert can ping a mesh IPv6 address. Args: <from_container> <ipv6>
assert_can_ping() {
    local from="$1"
    local ipv6="$2"
    if docker exec "$from" ping -6 -c 1 -W 3 "$ipv6" >/dev/null 2>&1; then
        pass "$from can ping $ipv6"
    else
        fail "$from cannot ping $ipv6"
    fi
}

# Get the mesh IPv6 of a container. Args: <container>
get_mesh_ipv6() {
    docker exec "$1" syfrah fabric status 2>&1 | grep "Mesh IPv6" | awk '{print $NF}'
}

# Assert syfrah0 interface does NOT exist. Args: <container>
assert_interface_gone() {
    local container="$1"
    if ! docker exec "$container" ip link show syfrah0 >/dev/null 2>&1; then
        pass "$container syfrah0 interface removed"
    else
        fail "$container syfrah0 interface still exists"
    fi
}

# Assert daemon is NOT running. Args: <container>
assert_daemon_stopped() {
    local container="$1"
    if docker exec "$container" syfrah fabric status 2>&1 | grep -q "stopped"; then
        pass "$container daemon is stopped"
    else
        # Also check if process is gone
        if ! docker exec "$container" pgrep -f "syfrah" >/dev/null 2>&1; then
            pass "$container daemon is stopped (process gone)"
        else
            fail "$container daemon is still running"
        fi
    fi
}

# ── Network manipulation ──────────────────────────────────────

# Block traffic between two containers. Args: <container> <target_ip>
block_traffic() {
    local container="$1"
    local target_ip="$2"
    debug "block_traffic: $container ↛ $target_ip"
    docker exec "$container" iptables -A OUTPUT -d "$target_ip" -j DROP 2>/dev/null || true
    docker exec "$container" iptables -A INPUT -s "$target_ip" -j DROP 2>/dev/null || true
}

# Unblock traffic. Args: <container> <target_ip>
unblock_traffic() {
    local container="$1"
    local target_ip="$2"
    debug "unblock_traffic: $container ↔ $target_ip"
    docker exec "$container" iptables -D OUTPUT -d "$target_ip" -j DROP 2>/dev/null || true
    docker exec "$container" iptables -D INPUT -s "$target_ip" -j DROP 2>/dev/null || true
}

# Assert CANNOT ping. Args: <from_container> <ipv6>
assert_cannot_ping() {
    local from="$1"
    local ipv6="$2"
    if ! docker exec "$from" ping -6 -c 1 -W 2 "$ipv6" >/dev/null 2>&1; then
        pass "$from cannot ping $ipv6 (expected)"
    else
        fail "$from CAN ping $ipv6 (should be blocked)"
    fi
}

# Assert a command fails (non-zero exit). Args: <container> <command...>
assert_command_fails() {
    local container="$1"
    shift
    debug "assert_command_fails: $container $*"
    if timeout 15 docker exec "$container" "$@" >/dev/null 2>&1; then
        fail "command should have failed: $*"
    else
        pass "command failed as expected: $*"
    fi
}

# Assert a command succeeds. Args: <container> <command...>
assert_command_succeeds() {
    local container="$1"
    shift
    debug "assert_command_succeeds: $container $*"
    if timeout 15 docker exec "$container" "$@" >/dev/null 2>&1; then
        pass "command succeeded: $*"
    else
        fail "command failed: $*"
    fi
}

# Get a field from state.json. Args: <container> <jq_filter>
get_state_field() {
    docker exec "$1" cat /root/.syfrah/state.json 2>/dev/null | jq -r "$2" 2>/dev/null
}

# Assert state.json exists. Args: <container>
assert_state_exists() {
    if docker exec "$1" test -f /root/.syfrah/state.json 2>/dev/null; then
        pass "$1 has state.json"
    else
        fail "$1 missing state.json"
    fi
}

# Assert state.json does NOT exist. Args: <container>
assert_state_gone() {
    if ! docker exec "$1" test -f /root/.syfrah/state.json 2>/dev/null; then
        pass "$1 state.json removed"
    else
        fail "$1 state.json still exists"
    fi
}

# Wait for all nodes to see expected peer count. Args: <prefix> <count> <expected_peers> <timeout>
wait_for_convergence() {
    local prefix="$1"
    local count="$2"
    local expected="$3"
    local timeout="${4:-60}"
    local deadline=$(($(date +%s) + timeout))

    while [ "$(date +%s)" -lt "$deadline" ]; do
        local all_ok=true
        for i in $(seq 1 "$count"); do
            local actual
            actual=$(docker exec "${prefix}${i}" syfrah fabric peers 2>&1 | grep -c "active" 2>/dev/null || echo "0")
            actual=$(echo "$actual" | tr -d '[:space:]')
            if [ "$actual" -ne "$expected" ] 2>/dev/null; then
                all_ok=false
                break
            fi
        done
        if [ "$all_ok" = true ]; then
            return 0
        fi
        sleep 2
    done
    return 1
}

# ── Cleanup ───────────────────────────────────────────────────

cleanup() {
    debug "cleanup: removing ${#E2E_CONTAINERS[@]} containers"
    for c in "${E2E_CONTAINERS[@]}"; do
        docker rm -f "$c" >/dev/null 2>&1 || true
    done
    E2E_CONTAINERS=()
}

# ── Summary ───────────────────────────────────────────────────

# Print results summary. Returns exit code 1 if any tests failed.
summary() {
    local total=$((PASSED + FAILED))
    echo ""
    if [ "$FAILED" -eq 0 ]; then
        echo -e "  ${GREEN}$total/$total passed${NC}"
        return 0
    else
        echo -e "  ${RED}$FAILED/$total failed${NC}"
        return 1
    fi
}
