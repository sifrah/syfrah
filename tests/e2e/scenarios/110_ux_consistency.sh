#!/usr/bin/env bash
# Scenario: UX — cross-command data consistency
# Verifies data shown by different commands is consistent.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX: Consistency ──"
trap cleanup EXIT
create_network

start_node "e2e-consist-1" "172.20.0.10"
start_node "e2e-consist-2" "172.20.0.11"
start_node "e2e-consist-3" "172.20.0.12"

# Set up 3-node mesh
info "Setting up 3-node mesh..."
output_init=$(docker exec "e2e-consist-1" syfrah fabric init \
    --name consist-mesh --node-name consist-srv-1 --endpoint 172.20.0.10:51820 2>&1)
wait_daemon "e2e-consist-1" 30
start_peering "e2e-consist-1"
join_mesh "e2e-consist-2" "172.20.0.10" "172.20.0.11" "consist-srv-2"
sleep 3
join_mesh "e2e-consist-3" "172.20.0.10" "172.20.0.12" "consist-srv-3"

sleep 10

# === Version consistency ===
info "Testing: version consistency..."
version=$(docker exec "e2e-consist-1" syfrah --version 2>&1)
if echo "$version" | grep -qE "[0-9]+\.[0-9]+\.[0-9]+"; then
    pass "version: valid semver"
else
    fail "version: not semver: $version"
fi

# === Secret consistency ===
info "Testing: secret consistency..."
init_secret=$(echo "$output_init" | grep -oE "syf_sk_[a-zA-Z0-9]+" | head -1)
token_secret=$(docker exec "e2e-consist-1" syfrah fabric token 2>&1 | grep -oE "syf_sk_[a-zA-Z0-9]+" | head -1)
if [ -n "$init_secret" ] && [ "$init_secret" = "$token_secret" ]; then
    pass "secret: init matches token"
else
    fail "secret: mismatch (init=${init_secret:-empty}, token=${token_secret:-empty})"
fi

# === Region/zone consistency ===
info "Testing: region/zone consistency..."
for node in "e2e-consist-1" "e2e-consist-2" "e2e-consist-3"; do
    assert_consistent_region "$node"
done

# === Peer count consistency ===
info "Testing: peer count consistency..."
for node in "e2e-consist-1" "e2e-consist-2" "e2e-consist-3"; do
    assert_peer_count "$node" 2
done

# === Bidirectional peer visibility ===
info "Testing: bidirectional peer visibility..."
for pair in "e2e-consist-1:consist-srv-2" "e2e-consist-1:consist-srv-3" \
            "e2e-consist-2:consist-srv-1" "e2e-consist-2:consist-srv-3" \
            "e2e-consist-3:consist-srv-1" "e2e-consist-3:consist-srv-2"; do
    from=$(echo "$pair" | cut -d: -f1)
    to=$(echo "$pair" | cut -d: -f2)
    if docker exec "$from" syfrah fabric peers 2>&1 | grep -q "$to"; then
        pass "$from sees $to"
    else
        fail "$from does NOT see $to"
    fi
done

# === No duplicates, no epoch dates ===
info "Testing: data quality..."
for node in "e2e-consist-1" "e2e-consist-2" "e2e-consist-3"; do
    assert_no_duplicate_peers "$node"
    assert_no_epoch_dates "$node"
done

# === Post leave+rejoin: no stale data ===
info "Testing: no stale data after leave+rejoin..."
docker exec "e2e-consist-3" syfrah fabric leave 2>&1 || true
sleep 3
start_peering "e2e-consist-1"
join_mesh "e2e-consist-3" "172.20.0.10" "172.20.0.12" "consist-srv-3"
sleep 10

# Verify node 3 is visible after rejoin
if docker exec "e2e-consist-1" syfrah fabric peers 2>&1 | grep -q "consist-srv-3"; then
    pass "node-1 sees node-3 after rejoin"
else
    fail "node-1 does NOT see node-3 after rejoin"
fi
if docker exec "e2e-consist-3" syfrah fabric peers 2>&1 | grep -q "consist-srv-1"; then
    pass "node-3 sees node-1 after rejoin"
else
    fail "node-3 does NOT see node-1 after rejoin"
fi
assert_no_epoch_dates "e2e-consist-1"
assert_no_epoch_dates "e2e-consist-3"

# === Post stop+start: no data loss ===
info "Testing: no data loss after stop+start..."
peers_before=$(docker exec "e2e-consist-1" syfrah fabric peers 2>&1 | tail -n +3 | wc -l)
stop_daemon "e2e-consist-1"
sleep 2
docker exec -d "e2e-consist-1" syfrah fabric start
wait_daemon "e2e-consist-1" 30
peers_after=$(docker exec "e2e-consist-1" syfrah fabric peers 2>&1 | tail -n +3 | wc -l)

if [ "$peers_before" = "$peers_after" ]; then
    pass "stop+start: peer count preserved ($peers_before)"
else
    fail "stop+start: peer count changed ($peers_before -> $peers_after)"
fi

cleanup
summary
