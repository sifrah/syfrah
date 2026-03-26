# Regions & Zones — Feature Design

> Consolidated design from 5 expert perspectives: cloud infrastructure, product management, UX design, software architecture, and testing/reliability.

## Overview

Currently, `region` and `zone` are decorative `Option<String>` fields on `PeerRecord`. They are displayed in CLI output but have **zero behavioral impact** on health checks, announce propagation, failure detection, or any fabric operation.

This document elevates regions and zones to **first-class topology concepts** that drive fabric behavior, provide visibility to operators, and expose an API to upper layers (compute, storage, overlay).

**Core principle:** Fabric remains a flat full-mesh. Regions and zones are **metadata** that inform timeouts, announce prioritization, and failure domain detection. No special routing or peering behavior based on topology.

---

## 1. What Are Regions and Zones?

**Region** = a geographic or administrative boundary (datacenter campus, cloud provider region, country). Nodes within a region share fast, reliable network (< 50ms latency).

**Zone** = a failure domain within a region (rack, availability zone, building). If a zone fails, other zones in the same region continue operating.

**Hierarchy:** `Mesh > Region > Zone > Node`

```
Mesh: prod-cloud
  Region: eu-west (OVH Paris + Scaleway Paris)
    Zone: par-ovh (OVH DC3)
      node-1, node-2
    Zone: par-scw (Scaleway DC5)
      node-3
  Region: eu-central (Hetzner)
    Zone: fsn-1 (Falkenstein DC14)
      node-4, node-5
    Zone: nbg-1 (Nuremberg DC3)
      node-6
  Region: us-east (AWS)
    Zone: use-1a (us-east-1a)
      node-7
```

**Every node belongs to exactly one zone. A zone belongs to exactly one region.**

---

## 2. Data Model

### Rust Types (in `layers/core/src/mesh.rs`)

```rust
/// Validated region identifier. Lowercase alphanumeric + hyphen, 1-64 chars.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct Region(String);

impl Region {
    pub fn new(s: &str) -> Option<Region> {
        // Validate: non-empty, <= 64 chars, [a-z0-9-], no leading/trailing dash
    }
    pub fn as_str(&self) -> &str { &self.0 }
}

/// Validated zone identifier. Same constraints as Region.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct Zone(String);

/// Topology metadata for a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Topology {
    pub region: Region,
    pub zone: Zone,
}
```

### Updated PeerRecord

```rust
pub struct PeerRecord {
    pub name: String,
    pub wg_public_key: String,
    pub endpoint: SocketAddr,
    pub mesh_ipv6: Ipv6Addr,
    pub last_seen: u64,
    pub status: PeerStatus,
    #[serde(default)]
    pub topology: Option<Topology>,  // backward-compatible
}
```

### Backward Compatibility

- Old peers have `topology: None`. On first announce/join, the daemon auto-fills with `Region("default")` + auto-generated zone.
- Old nodes ignore the new `topology` field (serde skips unknown fields).
- No forced migration. Lazy conversion as peers are updated.

### TopologyView (in `layers/fabric/src/topology.rs`)

```rust
/// Point-in-time snapshot of mesh topology for querying.
pub struct TopologyView {
    pub by_region: HashMap<Region, Vec<PeerRecord>>,
    pub by_zone: HashMap<Zone, Vec<PeerRecord>>,
    pub zone_to_region: HashMap<Zone, Region>,
}

impl TopologyView {
    pub fn snapshot() -> Result<Self, StoreError>;
    pub fn peers_in_region(&self, region: &Region) -> &[PeerRecord];
    pub fn peers_in_zone(&self, zone: &Zone) -> &[PeerRecord];
    pub fn regions(&self) -> Vec<&Region>;
    pub fn zones_in_region(&self, region: &Region) -> Vec<&Zone>;
}
```

This is the API that upper layers (compute, storage, overlay) will use for topology-aware decisions.

---

## 3. Topology-Aware Health Checks

### Problem

Current health check uses a **single global timeout** (300s) for all peers. This is:
- Too slow for same-zone peers (local network, < 1ms latency — 5 minutes to detect a dead neighbor is unacceptable)
- Too aggressive for cross-region peers (intercontinental links flap, 5 minutes might be reasonable)

### Solution: Per-Topology Timeout Tiers

| Relationship | Default Timeout | Rationale |
|---|---|---|
| Same zone | 120s | Same rack/DC, latency < 5ms. 4x keepalive cycles. |
| Same region, different zone | 180s | Same city, different building. Some transient loss expected. |
| Cross-region | 300s | Intercontinental. BGP flaps, routing changes are normal. |

### Configuration

```toml
# ~/.syfrah/config.toml
[health]
same_zone_timeout = 120
same_region_timeout = 180
cross_region_timeout = 300
```

### Implementation (in `daemon.rs`)

```rust
fn timeout_for_peer(my_topo: &Topology, peer_topo: &Topology, policy: &HealthPolicy) -> u64 {
    if my_topo.region == peer_topo.region {
        if my_topo.zone == peer_topo.zone {
            policy.same_zone_timeout
        } else {
            policy.same_region_timeout
        }
    } else {
        policy.cross_region_timeout
    }
}
```

### Zone Failure Detection

After computing per-peer health, aggregate by zone:

| Zone Status | Condition | Action |
|---|---|---|
| Healthy | >= 80% nodes active | None |
| Degraded | 50-79% active | Log warning, emit event |
| Critical | 25-49% active | Log error, emit event |
| Failed | < 25% active | Emit `ZoneFailed` event |

This gives operators early warning before a full zone outage.

---

## 4. Topology-Aware Announce Propagation

### Problem

When a peer joins, the leader announces it to **all** peers simultaneously. At 100 nodes = 100 parallel TCP connections. At 1000 = 1000. This causes announce queue saturation and silent drops.

### Solution: Wave-Based Announce Priority

| Wave | Target | Delay | Concurrency | Rationale |
|---|---|---|---|---|
| 1 | Same zone | 0ms | 50 | Fast local convergence |
| 2 | Same region | 5s | 20 | Regional propagation |
| 3 | Cross-region | 15s | 5 | Global propagation (slower, more retries) |

This ensures local nodes learn about changes first (important for scheduling), while cross-region propagation happens gracefully.

---

## 5. CLI Design

### New Command: `syfrah fabric topology`

```bash
syfrah fabric topology                   # full topology
syfrah fabric topology --region eu-west  # filter by region
syfrah fabric topology --json            # machine-readable
syfrah fabric topology --verbose         # include per-node details
```

**Default output (tree view):**

```
-- Topology ------------------------------------------
  Mesh: prod-cloud  |  Nodes: 7  |  Regions: 3  |  Zones: 5
------------------------------------------------------

eu-west (3 nodes)
  par-ovh (2 nodes)
    node-1            fd27::e101  active
    node-2            fd27::e102  active
  par-scw (1 node)
    node-3            fd27::e103  active

eu-central (2 nodes)
  fsn-1 (1 node)
    node-4            fd27::c201  active
  nbg-1 (1 node)
    node-5            fd27::c202  unreachable

us-east (2 nodes)
  use-1a (2 nodes)
    node-6            fd27::a101  active
    node-7            fd27::a102  active
```

### Updated `syfrah fabric peers`

New flags:
- `--topology` — group by region/zone instead of flat table
- `--region <REGION>` — filter by region
- `--zone <ZONE>` — filter by zone

### Updated `syfrah fabric status`

Show region and zone on separate lines (not combined):

```
-- Mesh ------------------------------------------
  Name:     prod-cloud
  Node:     node-1
  Region:   eu-west
  Zone:     par-ovh
  Prefix:   fd27:6d83:d501::/48
```

Add topology summary in peers section:

```
-- Peers (6) -------------------------------------
  5 active, 1 unreachable
  By region: eu-west (2) | eu-central (2) | us-east (2)
```

### New Command: `syfrah fabric zone`

For zone lifecycle operations:

```bash
syfrah fabric zone drain eu-west/par-ovh     # mark zone as draining
syfrah fabric zone undrain eu-west/par-ovh   # restore zone
syfrah fabric zone status                     # list all zones with health
```

### Enhanced Diagnostics

```bash
syfrah fabric diagnose --zone eu-west/par-ovh
```

Shows:
- Per-node connectivity status
- Possible causes (datacenter outage, network partition, daemon crash)
- Suggested remediation steps

### Input Validation

Region/zone names:
- 1-64 characters
- Lowercase alphanumeric + hyphens only (`[a-z0-9-]`)
- No leading/trailing hyphens
- Validated at `init`/`join` time with clear error messages

```
Error: Region name 'EU-WEST' is invalid. Use lowercase: 'eu-west'.
Error: Zone name 'par 1' is invalid. Use alphanumeric + hyphens: 'par-1'.
```

### Progressive Disclosure

| Mesh Size | Default UX | Advanced |
|---|---|---|
| 1-10 nodes | Region/zone optional, auto-generated. `topology` not needed. | Available if wanted |
| 10-50 nodes | Warning if `--region` not set. `topology` useful. | `--topology` flag on peers |
| 50+ nodes | Region required. `topology` is primary overview. | Zone drain, diagnostics |

---

## 6. Naming Conventions

### Recommended Patterns

**Regions:** Cloud-style geographic identifiers.
- `eu-west`, `eu-central`, `us-east`, `ap-south`, `ap-northeast`

**Zones:** Location abbreviation + number or provider suffix.
- `par-1`, `par-2` (city + number)
- `par-ovh`, `par-scw` (city + provider)
- `use-1a`, `use-1b` (AWS AZ style)
- `rack-1`, `rack-2` (physical location)

### Anti-Patterns

- Uppercase: `EU-WEST` (rejected by validation)
- Spaces: `eu west` (rejected)
- Special chars: `eu.west`, `eu@west` (rejected)
- Numeric-only: `1`, `2` (ambiguous)
- Very long: `european-union-western-region-paris-datacenter-3` (truncated in display)

---

## 7. What NOT to Build

These features were considered and explicitly excluded:

| Feature | Why Not |
|---|---|
| Auto-geolocation from IP | Inaccurate (~80%), privacy concerns, operator intent matters more |
| Automatic zone failover | Fabric should report, not remediate. Auto-failover masks root causes and risks cascading failures. |
| Zone capacity / resource tracking | Belongs in compute/scheduler layer, not fabric |
| Latency-based routing | WireGuard is full-mesh, no routing decisions at fabric level |
| Custom zone metadata/tags | Adds schema complexity. Tags belong in control plane |
| Multi-region consensus | Quorum logic belongs in control plane, not fabric |
| Zone rebalancing | Fabric is full-mesh; "rebalancing" is a scheduler concept |

---

## 8. Testing Strategy

### Unit Tests

| Test | Purpose |
|---|---|
| `Region::new()` validation | Accept valid, reject invalid names |
| `Zone::new()` validation | Same |
| `Topology` serde roundtrip | Serialize/deserialize preserves data |
| Backward compat deserialization | Old PeerRecord without topology loads as `None` |
| `TopologyView::snapshot()` | Correct grouping by region/zone |
| `timeout_for_peer()` | Correct timeout tier selection |
| Zone health status computation | Correct healthy/degraded/critical/failed thresholds |
| Property-based: Region roundtrip | Any valid string survives serde |

### E2E Integration Tests

| Scenario | Validates |
|---|---|
| `e2e_topology_basic.sh` | `syfrah fabric topology` shows correct tree after 3-node setup |
| `e2e_topology_multi_region.sh` | Nodes in 2 regions, verify cross-region peering and topology display |
| `e2e_topology_filter.sh` | `--region` and `--zone` filters return correct subset |
| `e2e_topology_json.sh` | `--json` output matches expected schema |
| `e2e_zone_health.sh` | Block traffic to a zone, verify degraded/failed detection |
| `e2e_zone_drain.sh` | Drain zone, verify status, undrain |
| `e2e_topology_backward_compat.sh` | Old state.json (no topology) loads and displays correctly |
| `e2e_validation_names.sh` | Invalid region/zone names rejected with clear errors |

### Dev Environment

Add `dev/test-topology.sh`:
- Spin up 4 containers: 2 in `eu-west/par-1`, 2 in `us-east/use-1`
- Verify `syfrah fabric topology` shows 2 regions, 2 zones
- Verify health checks use different timeouts per tier

### CI Integration

New E2E group `fabric-topology` in CI matrix. Estimated runtime: < 60s (no long sleeps needed for topology display tests).

---

## 9. Implementation Phases

### Phase 1: Data Model + Validation + CLI (2 weeks)

- `Region` and `Zone` typed structs in core
- Validation in `init`/`join` CLI
- `TopologyView` module
- `syfrah fabric topology` command (tree view + JSON)
- Updated `status` (separate region/zone lines)
- Updated `peers` (`--topology` flag)
- Backward-compatible serde
- Unit tests + E2E tests for topology display

**Deliverable:** Operators can see typed topology. No behavioral change.

### Phase 2: Topology-Aware Health Checks (1 week)

- Per-topology timeout tiers in health check loop
- Zone health status computation (healthy/degraded/critical/failed)
- `ZoneFailed` / `ZoneDegraded` events
- Config: `[health]` section with per-tier timeouts
- Unit tests for timeout selection + zone status thresholds
- E2E tests for zone failure detection

**Deliverable:** Faster detection of local failures, tolerant of cross-region flaps.

### Phase 3: Topology-Aware Announces (1 week)

- Wave-based announce prioritization
- Configurable concurrency per tier
- E2E tests for announce ordering

**Deliverable:** Local convergence first, then global. Less announce queue pressure at scale.

### Phase 4: Zone Operations + Diagnostics (1 week)

- `syfrah fabric zone drain/undrain/status`
- `syfrah fabric diagnose --zone`
- Zone drain state persistence
- E2E tests for zone lifecycle

**Deliverable:** Operators can manage zone lifecycle for maintenance.

### Phase 5: Upper Layer API (future)

- HTTP REST endpoints for topology queries
- Control plane integration for placement decisions
- Scheduler zone affinity/anti-affinity

**Deliverable:** Compute and storage layers can make topology-aware placement decisions.

---

## 10. Real-World Deployment Patterns

### Multi-DC (OVH + Hetzner + Scaleway)

```
Region: eu-west    Zone: par-ovh     (OVH Paris)
Region: eu-west    Zone: par-scw     (Scaleway Paris)
Region: eu-central Zone: fsn-hetzner (Hetzner Falkenstein)
Region: eu-central Zone: nbg-hetzner (Hetzner Nuremberg)
```

Same city, different provider = same region, different zone (separate failure domains).

### Hybrid Cloud (On-Prem + AWS)

```
Region: company-lhr  Zone: dc1-onprem  (Company datacenter London)
Region: company-lhr  Zone: colo-london (Colocation London)
Region: aws-lhr      Zone: eu-west-2a  (AWS London AZ)
Region: aws-lhr      Zone: eu-west-2b  (AWS London AZ)
```

### Edge (Multi-POP)

```
Region: us-east   Zone: nyc-vultr   (Vultr NYC)
Region: us-south  Zone: mia-vultr   (Vultr Miami)
Region: eu-west   Zone: ld4-equinix (Equinix London)
Region: ap-south  Zone: sg-linode   (Linode Singapore)
```

Edge pattern: 1-2 nodes per zone, many zones. Use longer cross-region timeouts (600s) due to high latency variance.

---

## 11. Points of Consensus Across Experts

All 5 perspectives agreed on these key decisions:

1. **Fabric stays flat full-mesh.** Regions/zones are metadata, not routing boundaries.
2. **Typed, not strings.** `Region`/`Zone` structs with validation at construction.
3. **Backward compatible.** `Option<Topology>` with lazy migration.
4. **Progressive disclosure.** Simple for small meshes, powerful at scale.
5. **No auto-remediation.** Fabric detects and reports zone failures; it does not auto-failover.
6. **Topology belongs in fabric.** Not a separate layer — health checks and announces must be topology-aware.
7. **Tree view for `topology` command.** ASCII tree is the most natural representation for hierarchical data.

### Points of Tension (Resolved)

| Tension | Cloud Expert | Architect | Resolution |
|---|---|---|---|
| Latency hints in metadata | Yes (per-zone `intra_zone_latency_ms`) | No (over-engineering) | **Deferred to Phase 5.** Use fixed tiers (same-zone/same-region/cross-region) for now. Adaptive latency measurement is future work. |
| Zone drain at fabric level | Yes (fabric should know) | Questionable (control plane concern) | **Include in Phase 4** as lightweight metadata. Fabric stores drain state; control plane acts on it. |
| Separate redb index tables | Testing expert suggested | Architect says unnecessary | **Not now.** `TopologyView::snapshot()` from peer list is O(n) and fast enough for < 1000 nodes. Add index if profiling shows need. |
