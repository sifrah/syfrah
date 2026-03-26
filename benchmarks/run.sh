#!/usr/bin/env bash
set -uo pipefail

# ─── Config ─────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [ -f "$REPO_ROOT/.env" ]; then
    set -a; source "$REPO_ROOT/.env"; set +a
fi
[ -z "${HCLOUD_TOKEN:-}" ] && echo "ERROR: HCLOUD_TOKEN not set" && exit 1
export HCLOUD_TOKEN

PREFIX="bench"
SSH_KEY="bench-key"
IMAGE="ubuntu-24.04"
TYPE_EU="cx23"
TYPE_US="cpx11"
MESH="prod-bench"
PIN="8421"
REPORT="$SCRIPT_DIR/report_$(date +%Y%m%d_%H%M%S).md"

BOLD='\033[1m' GREEN='\033[0;32m' RED='\033[0;31m' YELLOW='\033[0;33m' NC='\033[0m'
PASS_COUNT=0 FAIL_COUNT=0 SKIP_COUNT=0 TOTAL=0
REPORT_BODY=""

declare -a IPS NAMES LOCATIONS V6S
NODE_COUNT=0

# ─── Helpers ────────────────────────────────────────────────
log()    { echo -e "${BOLD}[$(date '+%H:%M:%S')]${NC} $*"; }
ok()     { echo -e "  ${GREEN}✓${NC} $*"; }
report() { REPORT_BODY+="$*"$'\n'; }

rcmd() { ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=15 -o LogLevel=ERROR root@"${IPS[$1]}" "$2" 2>/dev/null; }

pass_test() {
    TOTAL=$((TOTAL + 1)); PASS_COUNT=$((PASS_COUNT + 1))
    printf "  %-7s %-55s ${GREEN}PASS${NC}\n" "$1" "$2"
    report "| $1 | $2 | **PASS** | |"
}
fail_test() {
    TOTAL=$((TOTAL + 1)); FAIL_COUNT=$((FAIL_COUNT + 1))
    printf "  %-7s %-55s ${RED}FAIL${NC} %s\n" "$1" "$2" "${3:-}"
    report "| $1 | $2 | **FAIL** | ${3:-} |"
}

# ─── Cleanup ────────────────────────────────────────────────
cleanup() {
    log "${YELLOW}Destroying infrastructure...${NC}"
    for name in $(hcloud server list -o columns=name -o noheader 2>/dev/null | grep "^$PREFIX"); do
        hcloud server delete "$name" > /dev/null 2>&1 && echo "  Deleted $name" || true
    done
    hcloud ssh-key delete "$SSH_KEY" > /dev/null 2>&1 || true
    log "Done."
}
trap cleanup EXIT

# ─── Infrastructure ─────────────────────────────────────────
log "${BOLD}══════════════════════════════════════════════════${NC}"
log "${BOLD}  Syfrah Production Readiness Test Suite${NC}"
log "${BOLD}══════════════════════════════════════════════════${NC}"
echo ""

report "# Syfrah Production Readiness Report"
report "Date: $(date '+%Y-%m-%d %H:%M:%S UTC')"
report ""

hcloud ssh-key create --name "$SSH_KEY" --public-key-from-file /root/.ssh/id_ed25519.pub > /dev/null 2>&1 || true

log "Creating servers..."
idx=0
for spec in "fsn1-a:fsn1:$TYPE_EU" "fsn1-b:fsn1:$TYPE_EU" "nbg1-a:nbg1:$TYPE_EU" "nbg1-b:nbg1:$TYPE_EU" "hel1-a:hel1:$TYPE_EU" "hel1-b:hel1:$TYPE_EU" "ash-a:ash:$TYPE_US" "hil-a:hil:$TYPE_US"; do
    IFS=: read -r name loc type <<< "$spec"
    sname="$PREFIX-$name"
    if hcloud server create --name "$sname" --type "$type" --image "$IMAGE" --location "$loc" --ssh-key "$SSH_KEY" > /dev/null 2>&1; then
        ip=$(hcloud server ip "$sname" 2>/dev/null)
        IPS[$idx]="$ip"; NAMES[$idx]="$sname"; LOCATIONS[$idx]="$loc"
        ok "$sname → $ip ($loc)"
        idx=$((idx + 1))
    fi
done
NODE_COUNT=$idx
log "$NODE_COUNT nodes created."

# Wait SSH + install
log "Waiting SSH + installing..."
for i in $(seq 0 $((NODE_COUNT - 1))); do
    for _ in $(seq 1 30); do rcmd "$i" "true" && break || sleep 2; done
done

for i in $(seq 0 $((NODE_COUNT - 1))); do
    rcmd "$i" "apt-get update -qq > /dev/null 2>&1; apt-get install -y -qq iperf3 > /dev/null 2>&1; curl -fsSL https://github.com/sifrah/syfrah/releases/latest/download/install.sh | sh > /dev/null 2>&1" &
done
wait
ok "All nodes ready"

# Form mesh
log "Forming mesh..."
LEADER=0
rcmd $LEADER "syfrah fabric init --name $MESH --region eu-central --zone fsn-1 --endpoint ${IPS[$LEADER]}:51820" > /dev/null 2>&1
rcmd $LEADER "syfrah fabric peering start --pin $PIN" > /dev/null 2>&1

ZONES=("fsn-1" "fsn-2" "nbg-1" "nbg-2" "hel-1" "hel-2" "ash-1" "hil-1")
REGIONS=("eu-central" "eu-central" "eu-central" "eu-central" "eu-north" "eu-north" "us-east" "us-west")
for i in $(seq 1 $((NODE_COUNT - 1))); do
    rcmd "$i" "syfrah fabric join ${IPS[$LEADER]} --pin $PIN --region ${REGIONS[$i]} --zone ${ZONES[$i]} --endpoint ${IPS[$i]}:51820" > /dev/null 2>&1
done

log "Convergence (45s)..."
sleep 45

# Collect IPv6
for i in $(seq 0 $((NODE_COUNT - 1))); do
    V6S[$i]=$(rcmd "$i" "syfrah fabric status 2>/dev/null | grep -i 'mesh ipv6' | awk '{print \$NF}'" || echo "")
done

# Start iperf3 servers
for i in $(seq 0 $((NODE_COUNT - 1))); do
    rcmd "$i" "pkill iperf3 2>/dev/null; iperf3 -s -B ${V6S[$i]} -D 2>/dev/null" || true
done

report "## Results"
report ""
report "| ID | Test | Result | Details |"
report "|----|------|--------|---------|"

# ═══════════════════════════════════════════════════════════
# 1. MESH FORMATION
# ═══════════════════════════════════════════════════════════
log ""
log "${BOLD}── 1. Mesh Formation ──${NC}"

# T-001
v=$(rcmd 0 "syfrah --version")
[[ "$v" == *"syfrah"* ]] && pass_test "T-001" "Install works" || fail_test "T-001" "Install works" "$v"

# T-002
s=$(rcmd 0 "syfrah fabric status")
[[ "$s" == *"eu-central"* ]] && pass_test "T-002" "Region set correctly" || fail_test "T-002" "Region set correctly"

# T-003
c=$(rcmd 0 "syfrah fabric peers 2>/dev/null | grep -c active" || echo 0)
[ "$c" -ge $((NODE_COUNT - 1)) ] && pass_test "T-003" "All nodes joined ($c peers)" || fail_test "T-003" "All nodes joined" "only $c peers"

# T-004
all_ok=true
for i in $(seq 0 $((NODE_COUNT - 1))); do
    c=$(rcmd "$i" "syfrah fabric peers 2>/dev/null | grep -c active" || echo 0)
    [ "$c" -lt $((NODE_COUNT - 1)) ] && all_ok=false
done
$all_ok && pass_test "T-004" "Every node sees all peers" || fail_test "T-004" "Every node sees all peers"

# T-005
t=$(rcmd 0 "syfrah fabric topology" || echo "")
[[ "$t" == *"Nodes: $NODE_COUNT"* || "$t" == *"nodes"* ]] && pass_test "T-005" "Topology shows correct info" || fail_test "T-005" "Topology shows correct info"

# ═══════════════════════════════════════════════════════════
# 2. CONNECTIVITY
# ═══════════════════════════════════════════════════════════
log ""
log "${BOLD}── 2. Connectivity ──${NC}"

# T-011
rcmd 0 "ping6 -c 3 -W 3 ${V6S[1]}" > /dev/null 2>&1 && pass_test "T-011" "Ping6 same DC" || fail_test "T-011" "Ping6 same DC"

# T-012
rcmd 0 "ping6 -c 3 -W 3 ${V6S[2]}" > /dev/null 2>&1 && pass_test "T-012" "Ping6 cross-DC (fsn→nbg)" || fail_test "T-012" "Ping6 cross-DC"

# T-013
rcmd 0 "ping6 -c 3 -W 5 ${V6S[4]}" > /dev/null 2>&1 && pass_test "T-013" "Ping6 cross-region (fsn→hel)" || fail_test "T-013" "Ping6 cross-region"

# T-014
loss=$(rcmd 0 "ping6 -c 30 -i 1 ${V6S[1]} 2>&1 | grep -oP '\d+(?=% packet)'" || echo 100)
[ "$loss" -le 5 ] && pass_test "T-014" "Sustained ping 30s (${loss}% loss)" || fail_test "T-014" "Sustained ping" "${loss}% loss"

# T-015
mesh_ok=true
for s in 0 2 4; do
    for d in 1 3 5; do
        rcmd "$s" "ping6 -c 1 -W 3 ${V6S[$d]}" > /dev/null 2>&1 || mesh_ok=false
    done
done
$mesh_ok && pass_test "T-015" "Cross-pair mesh ping" || fail_test "T-015" "Cross-pair mesh ping"

# ═══════════════════════════════════════════════════════════
# 3. THROUGHPUT
# ═══════════════════════════════════════════════════════════
log ""
log "${BOLD}── 3. Throughput ──${NC}"

# T-021
bw=$(rcmd 0 "iperf3 -c ${V6S[1]} -t 5 --json | python3 -c 'import json,sys; d=json.load(sys.stdin); print(int(d[\"end\"][\"sum_received\"][\"bits_per_second\"]/1e6))'" || echo 0)
[ "$bw" -ge 500 ] && pass_test "T-021" "iperf3 single stream (${bw} Mbps)" || fail_test "T-021" "iperf3 single stream" "${bw} Mbps"

# T-022
bw=$(rcmd 0 "iperf3 -c ${V6S[1]} -t 5 -P 4 --json | python3 -c 'import json,sys; d=json.load(sys.stdin); print(int(d[\"end\"][\"sum_received\"][\"bits_per_second\"]/1e6))'" || echo 0)
[ "$bw" -ge 1000 ] && pass_test "T-022" "iperf3 4-stream (${bw} Mbps)" || fail_test "T-022" "iperf3 4-stream" "${bw} Mbps"

# T-023
bw=$(rcmd 0 "iperf3 -c ${V6S[2]} -t 5 -P 2 --json | python3 -c 'import json,sys; d=json.load(sys.stdin); print(int(d[\"end\"][\"sum_received\"][\"bits_per_second\"]/1e6))'" || echo 0)
[ "$bw" -ge 500 ] && pass_test "T-023" "iperf3 cross-DC (${bw} Mbps)" || fail_test "T-023" "iperf3 cross-DC" "${bw} Mbps"

# ═══════════════════════════════════════════════════════════
# 4. CHAOS: NODE FAILURES
# ═══════════════════════════════════════════════════════════
log ""
log "${BOLD}── 4. Chaos: Node Failures ──${NC}"

# T-031: Graceful stop + recover
rcmd 2 "syfrah fabric stop" > /dev/null 2>&1; sleep 5
rcmd 2 "syfrah fabric start" > /dev/null 2>&1; sleep 20
c=$(rcmd 2 "syfrah fabric peers 2>/dev/null | grep -c active" || echo 0)
[ "$c" -ge $((NODE_COUNT - 2)) ] && pass_test "T-031" "Graceful stop + recover ($c peers)" || fail_test "T-031" "Graceful stop + recover" "$c peers"

# T-032: Kill -9 + recover
rcmd 3 "kill -9 \$(cat ~/.syfrah/syfrah.pid 2>/dev/null) 2>/dev/null; rm -f ~/.syfrah/syfrah.pid" > /dev/null 2>&1; sleep 2
rcmd 3 "syfrah fabric start" > /dev/null 2>&1; sleep 20
c=$(rcmd 3 "syfrah fabric peers 2>/dev/null | grep -c active" || echo 0)
[ "$c" -ge $((NODE_COUNT - 2)) ] && pass_test "T-032" "Kill -9 + recover ($c peers)" || fail_test "T-032" "Kill -9 + recover" "$c peers"

# T-033: Kill 2, recover both
rcmd 4 "syfrah fabric stop" > /dev/null 2>&1
rcmd 5 "syfrah fabric stop" > /dev/null 2>&1; sleep 3
rcmd 4 "syfrah fabric start" > /dev/null 2>&1
rcmd 5 "syfrah fabric start" > /dev/null 2>&1; sleep 25
c=$(rcmd 0 "syfrah fabric peers 2>/dev/null | grep -c active" || echo 0)
[ "$c" -ge $((NODE_COUNT - 2)) ] && pass_test "T-033" "Kill 2 nodes, recover ($c peers)" || fail_test "T-033" "Kill 2 nodes" "$c peers"

# T-034: Rolling restart
for i in $(seq 0 $((NODE_COUNT - 1))); do
    rcmd "$i" "syfrah fabric stop" > /dev/null 2>&1; sleep 2
    rcmd "$i" "syfrah fabric start" > /dev/null 2>&1; sleep 8
done
sleep 20
c=$(rcmd 0 "syfrah fabric peers 2>/dev/null | grep -c active" || echo 0)
[ "$c" -ge $((NODE_COUNT - 2)) ] && pass_test "T-034" "Rolling restart ($c peers)" || fail_test "T-034" "Rolling restart" "$c peers"

# ═══════════════════════════════════════════════════════════
# 5. CHAOS: NETWORK FAILURES
# ═══════════════════════════════════════════════════════════
log ""
log "${BOLD}── 5. Chaos: Network ──${NC}"

# T-046: Block WG port
rcmd 2 "iptables -A INPUT -p udp --dport 51820 -j DROP" > /dev/null 2>&1; sleep 10
rcmd 2 "iptables -D INPUT -p udp --dport 51820 -j DROP" > /dev/null 2>&1; sleep 20
c=$(rcmd 2 "syfrah fabric peers 2>/dev/null | grep -c active" || echo 0)
[ "$c" -ge $((NODE_COUNT - 3)) ] && pass_test "T-046" "Block WG port + recover ($c peers)" || fail_test "T-046" "Block WG port" "$c peers"

# T-050: Latency
rcmd 3 "tc qdisc add dev eth0 root netem delay 200ms 2>/dev/null" > /dev/null 2>&1; sleep 15
c=$(rcmd 3 "syfrah fabric peers 2>/dev/null | grep -c active" || echo 0)
rcmd 3 "tc qdisc del dev eth0 root 2>/dev/null" > /dev/null 2>&1; sleep 10
[ "$c" -ge $((NODE_COUNT - 3)) ] && pass_test "T-050" "200ms latency, mesh survives ($c)" || fail_test "T-050" "200ms latency" "$c peers"

# T-049: Packet loss
rcmd 4 "tc qdisc add dev eth0 root netem loss 50% 2>/dev/null" > /dev/null 2>&1; sleep 15
rcmd 4 "tc qdisc del dev eth0 root 2>/dev/null" > /dev/null 2>&1; sleep 20
c=$(rcmd 4 "syfrah fabric peers 2>/dev/null | grep -c active" || echo 0)
[ "$c" -ge $((NODE_COUNT - 3)) ] && pass_test "T-049" "50% packet loss, recover ($c)" || fail_test "T-049" "50% packet loss" "$c peers"

# ═══════════════════════════════════════════════════════════
# 6. STATE PERSISTENCE
# ═══════════════════════════════════════════════════════════
log ""
log "${BOLD}── 6. State Persistence ──${NC}"

rcmd 0 "python3 -c 'import json; json.load(open(\"/root/.syfrah/state.json\"))'" && pass_test "T-061" "state.json valid JSON" || fail_test "T-061" "state.json valid"

rcmd 0 "test -f /root/.syfrah/fabric.redb" && pass_test "T-062" "fabric.redb exists" || fail_test "T-062" "fabric.redb exists"

# T-063: Delete JSON, recover from redb
rcmd 1 "syfrah fabric stop" > /dev/null 2>&1; sleep 2
rcmd 1 "rm -f /root/.syfrah/state.json" > /dev/null 2>&1
rcmd 1 "syfrah fabric start" > /dev/null 2>&1; sleep 15
c=$(rcmd 1 "syfrah fabric peers 2>/dev/null | grep -c active" || echo 0)
[ "$c" -ge $((NODE_COUNT - 2)) ] && pass_test "T-063" "Recover from redb ($c peers)" || fail_test "T-063" "Recover from redb" "$c peers"

sl=$(rcmd 0 "syfrah state list fabric 2>&1")
[[ "$sl" == *"config"* ]] && pass_test "T-069" "state list shows tables" || fail_test "T-069" "state list" "$sl"

# ═══════════════════════════════════════════════════════════
# 7. SECURITY
# ═══════════════════════════════════════════════════════════
log ""
log "${BOLD}── 7. Security ──${NC}"

p=$(rcmd 0 "stat -c %a /root/.syfrah/state.json" || echo "???")
[ "$p" = "600" ] && pass_test "T-076" "state.json perms 0600" || fail_test "T-076" "state.json perms" "$p"

p=$(rcmd 0 "stat -c %a /root/.syfrah" || echo "???")
[ "$p" = "700" ] && pass_test "T-078" "~/.syfrah/ perms 0700" || fail_test "T-078" "~/.syfrah/ perms" "$p"

s=$(rcmd 0 "syfrah fabric status")
[[ "$s" == *"****"* ]] && pass_test "T-079" "Status masks secret" || fail_test "T-079" "Status masks secret"

w=$(rcmd 0 "syfrah fabric token 2>&1 >/dev/null")
[[ "$w" == *"Warning"* || "$w" == *"warning"* || "$w" == *"sensitive"* ]] && pass_test "T-080" "Token shows warning" || fail_test "T-080" "Token warning" "$w"

# T-075: Oversized message
rcmd 0 "python3 -c \"import socket,os; s=socket.socket(socket.AF_UNIX); s.connect(os.path.expanduser('~/.syfrah/control.sock')); s.send(b'A'*100000); s.close()\"" > /dev/null 2>&1 || true
c=$(rcmd 0 "syfrah fabric peers 2>/dev/null | grep -c active" || echo 0)
[ "$c" -ge 1 ] && pass_test "T-075" "Oversized msg doesn't crash daemon" || fail_test "T-075" "Oversized msg"

# ═══════════════════════════════════════════════════════════
# 8. CLI UX
# ═══════════════════════════════════════════════════════════
log ""
log "${BOLD}── 8. CLI UX ──${NC}"

rcmd 0 "syfrah fabric status --json | python3 -c 'import json,sys; json.load(sys.stdin)'" && pass_test "T-081" "status --json valid" || fail_test "T-081" "status --json"

rcmd 0 "syfrah fabric peers --json | python3 -c 'import json,sys; json.load(sys.stdin)'" && pass_test "T-082" "peers --json valid" || fail_test "T-082" "peers --json"

rcmd 0 "syfrah fabric diagnose --json | python3 -c 'import json,sys; json.load(sys.stdin)'" && pass_test "T-083" "diagnose --json valid" || fail_test "T-083" "diagnose --json"

rcmd 0 "syfrah fabric topology --json | python3 -c 'import json,sys; json.load(sys.stdin)'" && pass_test "T-084" "topology --json valid" || fail_test "T-084" "topology --json"

e=$(rcmd 0 "syfrah fabric events --limit 5 2>/dev/null | grep -c '^20'" || echo 0)
[ "$e" -le 5 ] && pass_test "T-085" "events --limit 5 ($e events)" || fail_test "T-085" "events --limit" "$e events"

s=$(rcmd 0 "syfrah fabric status")
echo "$s" | grep -qP "\x1b\[" && fail_test "T-087" "Non-TTY no ANSI" || pass_test "T-087" "Non-TTY no ANSI codes"

cl=$(rcmd 0 "syfrah completions bash | wc -l" || echo 0)
[ "$cl" -ge 10 ] && pass_test "T-089" "completions bash ($cl lines)" || fail_test "T-089" "completions" "$cl lines"

help_ok=true
for cmd in "" "fabric" "fabric init" "fabric join" "fabric status" "fabric peers"; do
    rcmd 0 "syfrah $cmd --help > /dev/null 2>&1" || help_ok=false
done
$help_ok && pass_test "T-090" "All --help work" || fail_test "T-090" "Some --help fail"

# ═══════════════════════════════════════════════════════════
# 9. OPERATIONS
# ═══════════════════════════════════════════════════════════
log ""
log "${BOLD}── 9. Operations ──${NC}"

u=$(rcmd 0 "syfrah update --check 2>&1")
[[ "$u" == *"version"* || "$u" == *"date"* || "$u" == *"Update"* || "$u" == *"up to date"* || "$u" == *"available"* ]] && pass_test "T-093" "update --check" || fail_test "T-093" "update --check" "$u"

dp=$(rcmd 0 "syfrah fabric diagnose 2>/dev/null | grep -c PASS" || echo 0)
[ "$dp" -ge 8 ] && pass_test "T-095" "Diagnose ($dp checks pass)" || fail_test "T-095" "Diagnose" "$dp pass"

# ═══════════════════════════════════════════════════════════
# 10. STRESS
# ═══════════════════════════════════════════════════════════
log ""
log "${BOLD}── 10. Stress & Endurance ──${NC}"

# T-097: Churn
LAST=$((NODE_COUNT - 1))
churn_ok=true
for _ in $(seq 1 3); do
    rcmd $LAST "syfrah fabric leave --yes" > /dev/null 2>&1; sleep 3
    rcmd $LAST "syfrah fabric join ${IPS[$LEADER]} --pin $PIN --region ${REGIONS[$LAST]} --zone ${ZONES[$LAST]} --endpoint ${IPS[$LAST]}:51820" > /dev/null 2>&1; sleep 10
done
sleep 15
c=$(rcmd 0 "syfrah fabric peers 2>/dev/null | grep -c active" || echo 0)
[ "$c" -ge $((NODE_COUNT - 2)) ] && pass_test "T-097" "Join/leave churn ($c peers)" || fail_test "T-097" "Churn" "$c peers"

# T-099: Rapid chaos
for _ in $(seq 1 6); do
    v=$(( (RANDOM % (NODE_COUNT - 1)) + 1 ))
    rcmd "$v" "syfrah fabric stop" > /dev/null 2>&1; sleep 3
    rcmd "$v" "syfrah fabric start" > /dev/null 2>&1; sleep 12
done
sleep 25
alive=0
for i in $(seq 0 $((NODE_COUNT - 1))); do
    c=$(rcmd "$i" "syfrah fabric peers 2>/dev/null | grep -c active" || echo 0)
    [ "$c" -ge $((NODE_COUNT - 3)) ] && alive=$((alive + 1))
done
[ "$alive" -ge $((NODE_COUNT - 2)) ] && pass_test "T-099" "Chaos monkey ($alive/$NODE_COUNT stable)" || fail_test "T-099" "Chaos monkey" "$alive stable"

# T-100: Full lifecycle
rcmd 0 "syfrah fabric status > /dev/null && syfrah fabric diagnose > /dev/null && syfrah fabric topology > /dev/null && syfrah fabric peers > /dev/null && syfrah fabric events > /dev/null" && pass_test "T-100" "Full lifecycle commands" || fail_test "T-100" "Full lifecycle"

# ═══════════════════════════════════════════════════════════
# REPORT
# ═══════════════════════════════════════════════════════════

report ""
report "## Summary"
report "| Metric | Count |"
report "|--------|-------|"
report "| Total | $TOTAL |"
report "| Passed | $PASS_COUNT |"
report "| Failed | $FAIL_COUNT |"
report "| Pass rate | $(( PASS_COUNT * 100 / TOTAL ))% |"

mkdir -p "$(dirname "$REPORT")"
echo "$REPORT_BODY" > "$REPORT"

log ""
log "${BOLD}══════════════════════════════════════════════════${NC}"
log "  ${GREEN}PASS: $PASS_COUNT${NC}  ${RED}FAIL: $FAIL_COUNT${NC}  Total: $TOTAL  Rate: $(( PASS_COUNT * 100 / TOTAL ))%"
log "  Report: $REPORT"
log "${BOLD}══════════════════════════════════════════════════${NC}"
