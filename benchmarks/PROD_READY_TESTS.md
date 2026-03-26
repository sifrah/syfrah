# Production Readiness Test Suite — 100 Tests

## How to use

Each test has:
- **ID**: Unique identifier (T-001 to T-100)
- **Category**: What aspect it validates
- **Test**: What to do
- **Pass criteria**: What success looks like
- **Infra**: Minimum nodes required

---

## 1. Mesh Formation (T-001 to T-010)

| ID | Test | Pass Criteria | Infra |
|----|------|--------------|-------|
| T-001 | Init mesh on fresh Ubuntu 24.04 via `curl \| sh` install | Install < 30s, `syfrah --version` matches release | 1 node |
| T-002 | Init mesh with `--region` and `--zone` flags | Status shows correct region/zone | 1 node |
| T-003 | Join 2nd node with PIN auto-accept | 2 nodes see each other within 30s | 2 nodes |
| T-004 | Join 8 nodes sequentially (1 per second) | All 8 see 7 peers within 60s | 8 nodes |
| T-005 | Join 8 nodes concurrently (all at once) | All 8 converge within 90s | 8 nodes |
| T-006 | Join node with explicit `--endpoint IP:PORT` | Endpoint correctly displayed in peers | 2 nodes |
| T-007 | Join node without `--endpoint` (auto-detect) | Endpoint auto-detected from TCP peer address | 2 nodes |
| T-008 | Join with wrong PIN | Rejected with actionable error message | 2 nodes |
| T-009 | Join with invalid region name (uppercase) | Rejected with suggestion to fix | 2 nodes |
| T-010 | Join when leader has peering disabled | Connection refused with helpful message | 2 nodes |

## 2. Connectivity (T-011 to T-020)

| ID | Test | Pass Criteria | Infra |
|----|------|--------------|-------|
| T-011 | Ping6 between 2 nodes in same datacenter | < 5ms latency, 0% loss over 60s | 2 nodes same DC |
| T-012 | Ping6 between 2 nodes cross-region (EU→US) | < 200ms latency, 0% loss over 60s | 2 nodes cross-region |
| T-013 | Sustained ping6 for 10 minutes (same DC) | 0% packet loss | 2 nodes |
| T-014 | Sustained ping6 for 10 minutes (cross-region) | < 2% packet loss | 2 nodes cross-region |
| T-015 | TCP connection via mesh (netcat) | Data sent/received correctly | 2 nodes |
| T-016 | UDP stream via mesh (iperf3 -u) | < 1% jitter same DC | 2 nodes |
| T-017 | Full mesh ping: every node pings every other | 0% loss on all N×(N-1) pairs | 8 nodes |
| T-018 | MTU test: send 1400-byte packets via mesh | No fragmentation, 0% loss | 2 nodes |
| T-019 | Concurrent TCP connections (100 parallel) | All complete without error | 2 nodes |
| T-020 | DNS resolution via mesh (`syfrah fabric hosts --apply` + ping by name) | Hostname resolves to mesh IPv6 | 3 nodes |

## 3. Throughput (T-021 to T-030)

| ID | Test | Pass Criteria | Infra |
|----|------|--------------|-------|
| T-021 | iperf3 single stream (same DC) | > 1 Gbps | 2 nodes same DC |
| T-022 | iperf3 4 parallel streams (same DC) | > 1.5 Gbps | 2 nodes same DC |
| T-023 | iperf3 single stream (cross-region) | > 500 Mbps | 2 nodes cross-region |
| T-024 | iperf3 full matrix: all pairs (8 nodes) | Average > 1 Gbps | 8 nodes |
| T-025 | iperf3 all-to-all simultaneous (8 nodes, each sending to neighbor) | No node drops below 500 Mbps | 8 nodes |
| T-026 | iperf3 sustained 10 minutes (single pair) | Stable bandwidth, no degradation over time | 2 nodes |
| T-027 | iperf3 during reconcile cycle (every 30s) | No bandwidth drops > 10% during reconcile | 2 nodes |
| T-028 | iperf3 UDP max throughput | Measure ceiling, < 5% loss | 2 nodes |
| T-029 | iperf3 reverse mode (server sends) | Symmetric bandwidth | 2 nodes |
| T-030 | iperf3 with 1MB window size | > 2 Gbps on dedicated servers | 2 nodes dedicated |

## 4. Chaos Monkey — Node Failures (T-031 to T-045)

| ID | Test | Pass Criteria | Infra |
|----|------|--------------|-------|
| T-031 | Graceful stop of 1 node in 8-node mesh | 7 remaining see 6 active + 1 unreachable within 5 min | 8 nodes |
| T-032 | `kill -9` daemon on 1 node | Same as T-031 | 8 nodes |
| T-033 | Restart killed node | Recovers to 7 active peers within 60s | 8 nodes |
| T-034 | Full server reboot (hcloud server reboot) | Daemon auto-restarts (systemd), re-joins mesh | 8 nodes |
| T-035 | Kill 2 of 8 nodes simultaneously | 6 remaining converge, detect 2 unreachable | 8 nodes |
| T-036 | Kill 50% of nodes (4 of 8) | 4 remaining maintain mesh between themselves | 8 nodes |
| T-037 | Kill all nodes except 1, restart all | Full mesh recovers within 120s | 8 nodes |
| T-038 | Rolling restart: stop/start each node in sequence (1 per 30s) | Mesh never loses more than 1 node at a time | 8 nodes |
| T-039 | Kill leader (init node) | Mesh continues operating without leader | 8 nodes |
| T-040 | Kill node during iperf3 transfer | Transfer resumes on reconnection | 3 nodes |
| T-041 | Kill daemon, delete PID file, restart | Daemon starts cleanly | 2 nodes |
| T-042 | Kill daemon, leave PID file stale, restart | Detects stale PID, starts cleanly | 2 nodes |
| T-043 | Kill daemon, corrupt state.json, restart | Falls back to redb, starts cleanly | 2 nodes |
| T-044 | Power cycle simulation (kill -9 + immediate restart) | Daemon recovers from crash state | 2 nodes |
| T-045 | Rapid kill/restart cycle (10 times in 60s) | Daemon eventually stabilizes | 2 nodes |

## 5. Chaos Monkey — Network Failures (T-046 to T-060)

| ID | Test | Pass Criteria | Infra |
|----|------|--------------|-------|
| T-046 | Block WireGuard port (iptables DROP on 51820) on 1 node | Peers mark it unreachable within 5 min | 3 nodes |
| T-047 | Unblock port after 5 min | Peer recovers to active within 60s | 3 nodes |
| T-048 | Block peering port (51821) on leader | New joins fail with helpful error, existing mesh unaffected | 3 nodes |
| T-049 | Simulate 50% packet loss (tc netem) for 5 min | Peers may become unreachable, recover when loss stops | 3 nodes |
| T-050 | Simulate 200ms latency (tc netem) on 1 node | Mesh stays connected, iperf3 works at reduced throughput | 3 nodes |
| T-051 | Simulate network partition (2 groups can't talk) | Each group maintains internal connectivity | 4 nodes |
| T-052 | Heal network partition | Full mesh converges within 120s | 4 nodes |
| T-053 | Simulate bandwidth limit (10 Mbps) on 1 node | Node stays in mesh, iperf3 matches limit | 3 nodes |
| T-054 | Block IPv6 on syfrah0 interface | Peer becomes unreachable, mesh routing adapts | 3 nodes |
| T-055 | DNS resolution failure (no /etc/resolv.conf) | Mesh unaffected (uses IPs, not DNS) | 2 nodes |
| T-056 | Flapping network (up 10s, down 5s, repeat 10x) | Peer eventually stabilizes as active or unreachable | 3 nodes |
| T-057 | Block only outbound traffic (allow inbound) | Asymmetric connectivity detected and reported | 3 nodes |
| T-058 | Saturate network (iperf3 flood) while joining new node | Join succeeds despite congestion | 4 nodes |
| T-059 | Rate-limit peering port to 1 connection/sec | Joins slow but succeed, no crash | 3 nodes |
| T-060 | Simultaneous partition + node kill | Survivors maintain mesh, recover when healed | 6 nodes |

## 6. State Persistence (T-061 to T-070)

| ID | Test | Pass Criteria | Infra |
|----|------|--------------|-------|
| T-061 | Stop daemon, verify state.json exists and valid | Valid JSON, peers present | 1 node |
| T-062 | Stop daemon, verify fabric.redb exists | redb opens without error | 1 node |
| T-063 | Delete state.json, restart daemon | Recovers from redb, regenerates JSON | 1 node |
| T-064 | Delete fabric.redb, restart daemon | Fails gracefully with clear error | 1 node |
| T-065 | Delete both state.json and fabric.redb | Fails, requires rejoin | 1 node |
| T-066 | Corrupt state.json (truncate to 0 bytes), restart | Falls back to redb | 1 node |
| T-067 | Disk full simulation (fill /root with dd) | Daemon warns but doesn't crash | 1 node |
| T-068 | Read-only filesystem on ~/.syfrah/ | Daemon fails to write, clear error | 1 node |
| T-069 | `syfrah state list fabric` after 1000 peer operations | Lists all 3 tables without error | 2 nodes |
| T-070 | `syfrah state drop fabric --force` then rejoin | Clean rejoin from scratch | 2 nodes |

## 7. Security (T-071 to T-080)

| ID | Test | Pass Criteria | Infra |
|----|------|--------------|-------|
| T-071 | Attempt join with invalid mesh secret | Rejected, no info leak | 2 nodes (different meshes) |
| T-072 | Attempt announce with forged WG key | Rejected with signature verification failure | 2 nodes |
| T-073 | Brute-force PIN (100 attempts) | Rate-limited after 5 attempts, locked 10 min | 2 nodes |
| T-074 | Connect to peering port without TLS | Connection refused | 2 nodes |
| T-075 | Send oversized message to control socket (>64KB) | Rejected, daemon doesn't crash | 1 node |
| T-076 | Check state.json permissions are 0o600 | Not world-readable | 1 node |
| T-077 | Check audit.log permissions are 0o600 | Not world-readable | 1 node |
| T-078 | Check ~/.syfrah/ directory permissions are 0o700 | Not world-accessible | 1 node |
| T-079 | `syfrah fabric status` does NOT show mesh secret | Shows masked `****` | 1 node |
| T-080 | `syfrah fabric token` shows security warning | Warning on stderr before secret | 1 node |

## 8. CLI UX (T-081 to T-090)

| ID | Test | Pass Criteria | Infra |
|----|------|--------------|-------|
| T-081 | `syfrah fabric status --json` | Valid JSON output | 1 node |
| T-082 | `syfrah fabric peers --json` | Valid JSON with all peer fields | 2 nodes |
| T-083 | `syfrah fabric diagnose --json` | Valid JSON with checks array | 1 node |
| T-084 | `syfrah fabric topology --json` | Valid JSON with regions/zones/nodes | 4 nodes |
| T-085 | `syfrah fabric events --limit 5` | Shows exactly 5 events | 1 node |
| T-086 | `syfrah fabric hosts` | Outputs valid /etc/hosts format | 2 nodes |
| T-087 | `syfrah fabric status \| cat` (non-TTY) | Plain text, no ANSI codes | 1 node |
| T-088 | `syfrah fabric peers --topology` | Grouped by region/zone | 4 nodes |
| T-089 | `syfrah completions bash` | Valid bash completion script | 1 node |
| T-090 | `syfrah --help` and all subcommand `--help` | No missing commands, no errors | 1 node |

## 9. Operations (T-091 to T-095)

| ID | Test | Pass Criteria | Infra |
|----|------|--------------|-------|
| T-091 | `syfrah fabric service install` + systemctl start | Daemon runs via systemd | 1 node |
| T-092 | Reboot server with systemd service | Daemon auto-starts, rejoins mesh | 2 nodes |
| T-093 | `syfrah update --check` | Reports current vs latest version | 1 node |
| T-094 | `syfrah update` (full update with daemon restart) | Binary updated, daemon restarted, peers reconnected | 2 nodes |
| T-095 | `syfrah fabric reload` (hot config change) | Config applied without restart | 1 node |

## 10. Scale & Endurance (T-096 to T-100)

| ID | Test | Pass Criteria | Infra |
|----|------|--------------|-------|
| T-096 | 24-hour soak test (8 nodes, continuous ping6) | < 0.1% packet loss over 24h | 8 nodes |
| T-097 | Join/leave churn: 1 node joins and leaves every 60s for 1 hour | Mesh stable, no orphan peers | 4 nodes |
| T-098 | Secret rotation during active iperf3 traffic | Traffic interrupted < 10s, mesh recovers | 4 nodes |
| T-099 | Chaos monkey: random kills every 30s for 30 minutes | Mesh self-heals after each event | 8 nodes |
| T-100 | Full lifecycle: init → scale to 8 → chaos → scale down → rotate → scale up → leave all | Every step completes without error | 8 nodes |

---

## Summary

| Category | Tests | Min Nodes |
|----------|-------|-----------|
| Mesh Formation | T-001 to T-010 | 1-8 |
| Connectivity | T-011 to T-020 | 2-8 |
| Throughput | T-021 to T-030 | 2-8 |
| Chaos: Node Failures | T-031 to T-045 | 2-8 |
| Chaos: Network Failures | T-046 to T-060 | 2-6 |
| State Persistence | T-061 to T-070 | 1-2 |
| Security | T-071 to T-080 | 1-2 |
| CLI UX | T-081 to T-090 | 1-4 |
| Operations | T-091 to T-095 | 1-2 |
| Scale & Endurance | T-096 to T-100 | 4-8 |

**Total: 100 tests**
**Infrastructure needed: 8 servers (dedicated vCPU recommended)**
**Estimated runtime: ~4-6 hours (excluding 24h soak test)**
