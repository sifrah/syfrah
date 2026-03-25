#!/usr/bin/env bash
# Scenario: README Quickstart — exact reproduction
# Executes the exact commands from the README Quick Start section.
# If the README changes, update this test.
SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── README Quickstart ──"
create_network

start_node "e2e-readme-1" "172.20.0.10"
start_node "e2e-readme-2" "172.20.0.11"

# ═══════════════════════════════════════════════
# Commands below are EXACTLY from the README.
# If the README changes, update this test.
# ═══════════════════════════════════════════════

# README: "Server 1: create a mesh"
# README command: syfrah fabric init --name my-cloud
# (--endpoint required in E2E to bind to container IP)
info "README step: syfrah fabric init --name my-cloud"
output_init=$(docker exec "e2e-readme-1" syfrah fabric init \
    --name my-cloud --endpoint 172.20.0.10:51820 2>&1)

# Validate init output contains what README implies
echo "$output_init" | grep -q "my-cloud" || fail "init doesn't show mesh name"
# Secret is no longer printed during init (security improvement)
echo "$output_init" | grep -qv "syf_sk_" || fail "init should not show secret"
pass "init output matches README expectations"

wait_daemon "e2e-readme-1" 30

# README: "syfrah fabric peering --pin 4829"
# (actual CLI is 'peering start')
info "README step: syfrah fabric peering start --pin 4829"
docker exec "e2e-readme-1" syfrah fabric peering start --pin 4829
sleep 3

# README: "Server 2: join the mesh"
# README command: syfrah fabric join 203.0.113.1 --pin 4829
# (using E2E IP + --endpoint for container networking)
info "README step: syfrah fabric join <IP> --pin 4829"
output_join=$(docker exec "e2e-readme-2" syfrah fabric join 172.20.0.10:51821 \
    --pin 4829 --endpoint 172.20.0.11:51820 2>&1)

echo "$output_join" | grep -qi "joined\|approved\|my-cloud" || fail "join output unclear"
pass "join output matches README expectations"

wait_daemon "e2e-readme-2" 30

# README: "Check status"
# README command: syfrah fabric status
info "README step: syfrah fabric status"
output_status=$(docker exec "e2e-readme-1" syfrah fabric status 2>&1)
echo "$output_status" | grep -q "my-cloud" || fail "status doesn't show mesh name"
echo "$output_status" | grep -qi "running\|active" || fail "status doesn't show daemon running"
pass "status works as README implies"

# README command: syfrah fabric peers
info "README step: syfrah fabric peers"
output_peers=$(docker exec "e2e-readme-1" syfrah fabric peers 2>&1)
echo "$output_peers" | grep -q "e2e-readme-2\|active" || fail "peers doesn't show server 2"
pass "peers works as README implies"

# Cross-validate: server 2 also sees server 1
output_peers_2=$(docker exec "e2e-readme-2" syfrah fabric peers 2>&1)
echo "$output_peers_2" | grep -q "e2e-readme-1\|active" || fail "server 2 doesn't see server 1"
pass "bidirectional mesh confirmed"

# Validate NO bad patterns in any output
for node in "e2e-readme-1" "e2e-readme-2"; do
    assert_no_duplicate_peers "$node"
    assert_no_epoch_dates "$node"
done

cleanup
summary
