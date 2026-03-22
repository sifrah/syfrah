#!/usr/bin/env bash
set -euo pipefail

NODE_COUNT=50
NETWORK="mesh"
IPFS_API="http://172.28.0.2:5001"
BASE_IP="172.28.0"
IMAGE="syfrah-e2e"
COMPOSE_FILE="$(cd "$(dirname "$0")" && pwd)/docker-compose.yml"

cleanup() {
    echo ""
    echo "=== Cleanup ==="
    for i in $(seq 1 $NODE_COUNT); do
        docker rm -f "syfrah-n${i}" 2>/dev/null || true
    done
    docker compose -f "$COMPOSE_FILE" down -v 2>/dev/null || true
}
trap cleanup EXIT

echo "=== 50-node mesh stress test (IPFS discovery) ==="
echo ""

echo "=== Building syfrah image ==="
docker build -f "$(dirname "$0")/Dockerfile" -t "$IMAGE" "$(dirname "$0")/../.." 2>&1 | tail -3

echo ""
echo "=== Starting IPFS node ==="
docker compose -f "$COMPOSE_FILE" up -d ipfs
echo "Waiting for IPFS to be ready..."
for i in $(seq 1 30); do
    if docker compose -f "$COMPOSE_FILE" exec ipfs ipfs id >/dev/null 2>&1; then
        echo "IPFS ready after ${i}s"
        break
    fi
    sleep 1
done

# Verify IPFS API is accessible
docker compose -f "$COMPOSE_FILE" exec ipfs ipfs id 2>&1 | head -1

echo ""
echo "=== Starting node-1 (init) ==="
docker run -d --name syfrah-n1 \
    --cap-add NET_ADMIN \
    --sysctl net.ipv6.conf.all.disable_ipv6=0 \
    --network "${NETWORK}" \
    --ip "${BASE_IP}.10" \
    -e RUST_LOG=info \
    "$IMAGE" init --name stress-test --node-name node-1 --port 51820 \
        --endpoint "${BASE_IP}.10:51820" --ipfs-api "$IPFS_API"

echo "Waiting for node-1..."
sleep 8

SECRET=$(docker exec syfrah-n1 syfrah token 2>&1)
if [[ ! "$SECRET" == syf_sk_* ]]; then
    echo "FAIL: could not get secret"
    docker logs syfrah-n1 2>&1 | tail -20
    exit 1
fi
echo "Secret: ${SECRET:0:30}..."

echo ""
echo "=== Starting nodes 2-${NODE_COUNT} (join, batches of 10) ==="
BATCH_SIZE=10
for batch_start in $(seq 2 $BATCH_SIZE $NODE_COUNT); do
    batch_end=$((batch_start + BATCH_SIZE - 1))
    [ $batch_end -gt $NODE_COUNT ] && batch_end=$NODE_COUNT

    echo -n "  Nodes ${batch_start}-${batch_end}..."
    for i in $(seq $batch_start $batch_end); do
        ip_last=$((9 + i))
        docker run -d --name "syfrah-n${i}" \
            --cap-add NET_ADMIN \
            --sysctl net.ipv6.conf.all.disable_ipv6=0 \
            --network "${NETWORK}" \
            --ip "${BASE_IP}.${ip_last}" \
            -e RUST_LOG=warn \
            "$IMAGE" join "$SECRET" --node-name "node-${i}" --port 51820 \
                --endpoint "${BASE_IP}.${ip_last}:51820" --ipfs-api "$IPFS_API" \
            >/dev/null 2>&1
    done
    echo " launched"
    sleep 3
done

echo ""
echo "=== Checking running nodes ==="
RUNNING=0
EXITED=0
for i in $(seq 1 $NODE_COUNT); do
    STATUS=$(docker inspect --format='{{.State.Status}}' "syfrah-n${i}" 2>/dev/null || echo "missing")
    [ "$STATUS" = "running" ] && RUNNING=$((RUNNING + 1)) || EXITED=$((EXITED + 1))
done
echo "Running: ${RUNNING}/${NODE_COUNT} (${EXITED} failed)"

echo ""
echo "=== Waiting for IPFS discovery convergence ==="
# IPFS: publish every 30s, poll every 15s. Should converge fast.
for t in 30 60 90 120; do
    sleep 30
    N1=$(docker exec syfrah-n1 syfrah peers 2>&1 | grep -c "active" || echo "0")
    echo "  +${t}s: node-1 sees ${N1}/$((RUNNING - 1)) peers"
    [ "$N1" -ge "$((RUNNING - 1))" ] && echo "  Full convergence!" && break
done

echo ""
echo "=== Final Results ==="

docker exec syfrah-n1 syfrah status 2>&1
N1_PEERS=$(docker exec syfrah-n1 syfrah peers 2>&1 | grep -c "active" || echo "0")

echo ""
echo "--- Peer counts ---"
for i in 1 10 25 40 50; do
    STATUS=$(docker inspect --format='{{.State.Status}}' "syfrah-n${i}" 2>/dev/null || echo "missing")
    if [ "$STATUS" = "running" ]; then
        COUNT=$(docker exec "syfrah-n${i}" syfrah peers 2>&1 | grep -c "active" || echo "0")
        echo "  node-${i}: ${COUNT} peers"
    else
        echo "  node-${i}: not running"
    fi
done

echo ""
echo "--- Ping tests ---"
NODE25_IP=$(docker exec syfrah-n25 syfrah status 2>&1 | grep "Mesh IPv6" | awk '{print $3}' 2>/dev/null || echo "")
NODE50_IP=$(docker exec syfrah-n50 syfrah status 2>&1 | grep "Mesh IPv6" | awk '{print $3}' 2>/dev/null || echo "")

[ -n "$NODE25_IP" ] && echo "node-1 -> node-25:" && docker exec syfrah-n1 ping6 -c 3 -W 5 "$NODE25_IP" 2>&1 | tail -2
[ -n "$NODE50_IP" ] && echo "node-1 -> node-50:" && docker exec syfrah-n1 ping6 -c 3 -W 5 "$NODE50_IP" 2>&1 | tail -2

echo ""
WG_PEERS=$(docker exec syfrah-n1 wg show syfrah0 2>&1 | grep -c "^peer:" || echo "0")
echo "WG peers on node-1: ${WG_PEERS}"

echo ""
echo "=== Summary ==="
echo "Nodes launched:  ${NODE_COUNT}"
echo "Nodes running:   ${RUNNING}"
echo "Node-1 peers:    ${N1_PEERS}/$((RUNNING - 1))"
echo "WG peers:        ${WG_PEERS}"

[ "$N1_PEERS" -ge "$((RUNNING - 1))" ] && echo "RESULT: PASS" || echo "RESULT: PARTIAL"
