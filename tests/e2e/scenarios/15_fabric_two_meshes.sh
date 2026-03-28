#!/usr/bin/env bash
# Scenario: Two independent meshes on the same network
# Verifies they don't interfere (different secrets = different encryption keys)

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Two Independent Meshes ──"

create_network

# Mesh Alpha: nodes 1-2
start_node "e2e-alpha-1" "172.20.0.10"
start_node "e2e-alpha-2" "172.20.0.11"

# Mesh Beta: nodes 3-4 (different ports to avoid conflicts)
start_node "e2e-beta-1" "172.20.0.20"
start_node "e2e-beta-2" "172.20.0.21"

# Init mesh Alpha on node-1
E2E_MESH="alpha-mesh"
init_mesh "e2e-alpha-1" "172.20.0.10" "alpha-1"
E2E_PIN="1111"
start_peering "e2e-alpha-1"

# Join Alpha mesh
docker exec -d "e2e-alpha-2" \
    syfrah fabric join 172.20.0.10:51821 \
    --node-name alpha-2 \
    --endpoint 172.20.0.11:51820 \
    --pin "1111"
wait_daemon "e2e-alpha-2"

# Init mesh Beta on node-3 (different ports)
docker exec -d "e2e-beta-1" \
    syfrah fabric init \
    --name "beta-mesh" \
    --node-name beta-1 \
    --endpoint 172.20.0.20:51830 \
    --port 51830 \
    --peering-port 51831
wait_daemon "e2e-beta-1"

docker exec "e2e-beta-1" \
    syfrah fabric peering start --pin "2222"

docker exec -d "e2e-beta-2" \
    syfrah fabric join 172.20.0.20:51831 \
    --node-name beta-2 \
    --endpoint 172.20.0.21:51830 \
    --port 51830 \
    --pin "2222"
wait_daemon "e2e-beta-2"

# Wait for peer convergence instead of fixed sleep
wait_for_peer_active "e2e-alpha-1" 1 30
wait_for_peer_active "e2e-beta-1" 1 30

# Each mesh sees 1 peer (its own partner)
assert_peer_count "e2e-alpha-1" 1
assert_peer_count "e2e-beta-1" 1

# Alpha nodes can reach each other
alpha2_ipv6=$(get_mesh_ipv6 "e2e-alpha-2")
if [ -n "$alpha2_ipv6" ]; then
    assert_can_ping "e2e-alpha-1" "$alpha2_ipv6"
else
    fail "could not get mesh IPv6 for e2e-alpha-2"
fi

# Beta nodes can reach each other
beta2_ipv6=$(get_mesh_ipv6 "e2e-beta-2")
if [ -n "$beta2_ipv6" ]; then
    assert_can_ping "e2e-beta-1" "$beta2_ipv6"
    # Cross-mesh: Alpha cannot reach Beta
    # (different WG keys, different mesh prefixes, no shared peers)
    assert_cannot_ping "e2e-alpha-1" "$beta2_ipv6"
else
    fail "could not get mesh IPv6 for e2e-beta-2"
fi

# Different secrets
alpha_secret=$(get_state_field "e2e-alpha-1" ".mesh_secret")
beta_secret=$(get_state_field "e2e-beta-1" ".mesh_secret")
if [ "$alpha_secret" != "$beta_secret" ]; then
    pass "meshes have different secrets"
else
    fail "meshes have same secret (should be impossible)"
fi

cleanup
summary
