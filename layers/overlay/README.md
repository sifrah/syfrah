# Overlay Network

## What is the overlay?

The **overlay** is the tenant-facing virtual network layer. It provides VPCs, subnets, and network isolation for VMs — the equivalent of AWS VPC or GCP VPC.

The overlay sits on top of the [fabric](fabric.md) (WireGuard mesh between nodes). Tenants interact with the overlay. They never see the fabric.

```
    ┌─────────────────────────────────────────────────────┐
    │  Tenant VMs                                          │
    │  10.0.1.5 ◄──── same subnet, same VPC ────► 10.0.1.9│
    ├─────────────────────────────────────────────────────┤
    │  Overlay                                             │
    │  VPC isolation (VXLAN VNI per VPC)                   │
    │  Subnet routing, security groups, NAT                │
    ├─────────────────────────────────────────────────────┤
    │  Fabric                                              │
    │  WireGuard mesh (encrypted, node-to-node)            │
    ├─────────────────────────────────────────────────────┤
    │  Internet                                            │
    │  Physical connectivity between dedicated servers     │
    └─────────────────────────────────────────────────────┘
```

## How it works

Each VPC gets a unique **VXLAN Network Identifier** (VNI). VXLAN encapsulates tenant Ethernet frames inside UDP packets that travel over the WireGuard fabric. Different VNIs are completely isolated — a VM in VPC-A cannot see or reach traffic in VPC-B.

```
    Node 1                                    Node 2
    ┌────────────────────┐                    ┌────────────────────┐
    │  VM-A (10.0.1.5)   │                    │  VM-B (10.0.1.9)   │
    │  └── tap-vmA       │                    │  └── tap-vmB       │
    │       │            │                    │       │            │
    │  ┌────┴─────┐      │                    │  ┌────┴─────┐      │
    │  │ br-100   │      │                    │  │ br-100   │      │
    │  │ (bridge) │      │                    │  │ (bridge) │      │
    │  └────┬─────┘      │                    │  └────┬─────┘      │
    │       │            │                    │       │            │
    │  ┌────┴─────┐      │                    │  ┌────┴─────┐      │
    │  │vxlan-100 │      │                    │  │vxlan-100 │      │
    │  │(VNI 100) │      │                    │  │(VNI 100) │      │
    │  └────┬─────┘      │                    │  └────┬─────┘      │
    │       │            │                    │       │            │
    │  syfrah0 (WG)      │                    │  syfrah0 (WG)      │
    └───────┼────────────┘                    └───────┼────────────┘
            │           encrypted tunnel              │
            └─────────────────────────────────────────┘
```

On each node, per VPC:
- A **VXLAN interface** handles encapsulation/decapsulation
- A **Linux bridge** connects local VMs to the VXLAN interface
- A **TAP device** per VM plugs into the bridge

## Packet flow

When VM-A (10.0.1.5 on Node 1) sends a packet to VM-B (10.0.1.9 on Node 2):

```
    1. VM-A sends Ethernet frame
       src MAC: 02:00:0a:00:01:05
       dst MAC: 02:00:0a:00:01:09
       src IP:  10.0.1.5
       dst IP:  10.0.1.9
            │
    2. Frame arrives on tap-vmA → forwarded to br-100
            │
    3. Bridge looks up dst MAC in FDB
       → "02:00:0a:00:01:09 is at VTEP fd12::node2"
       → sends frame out vxlan-100
            │
    4. VXLAN encapsulation:
       [UDP dst=4789][VXLAN VNI=100][original Ethernet frame]
       outer src: fd12::node1 (fabric IPv6)
       outer dst: fd12::node2 (fabric IPv6)
            │
    5. Outer packet routes to syfrah0 (WireGuard)
       → encrypted with ChaCha20-Poly1305
       → sent over the internet to Node 2
            │
    6. Node 2: WireGuard decrypts → VXLAN decapsulates
       → frame delivered to br-100 → tap-vmB → VM-B
```

Total overhead per packet: ~130 bytes (VXLAN 50 + WireGuard 80 over IPv6). The fabric provides encryption; VXLAN provides isolation.

## No broadcast flooding

Traditional VXLAN uses flood-and-learn for MAC discovery. Syfrah doesn't — the control plane knows where every VM lives.

**Static FDB entries**: when a VM is created, the control plane tells every node in that VPC the VM's MAC address and which node it's on. No flooding, no learning, no guessing.

**ARP proxy**: VXLAN interfaces run in proxy mode. When a VM ARPs for another VM's IP, the local VXLAN interface answers immediately from its neighbor table — no ARP broadcast crosses the network.

Result: **zero broadcast traffic** in steady state. The control plane populates FDB and neighbor entries; the data plane just forwards.

## Security groups

Security groups are per-VM stateful firewalls, enforced on the host via nftables.

### Model

- **Default deny ingress**: no inbound traffic unless a rule allows it
- **Default allow egress**: all outbound traffic is allowed (configurable)
- **Stateful**: return traffic for allowed connections is automatically permitted (conntrack)
- **Allow-only rules**: no explicit deny — anything not allowed is dropped

### How it works

Each VM's TAP device gets nftables chains on the host. The VM cannot bypass them — they are enforced outside the VM, at the bridge level.

```
    VM sends packet
         │
    ┌────┴──────────────┐
    │  nftables check   │
    │                   │
    │  Anti-spoofing:   │  ← source MAC and IP must match assigned values
    │  Egress rules:    │  ← default allow, or tenant-configured
    │  Conntrack:       │  ← established/related always allowed
    └────┬──────────────┘
         │
    Bridge → VXLAN → Fabric → Remote node
         │
    ┌────┴──────────────┐
    │  nftables check   │
    │                   │
    │  Ingress rules:   │  ← only allowed ports/sources pass
    │  Conntrack:       │  ← return traffic for outbound connections
    │  Anti-spoofing:   │  ← destination must be this VM
    └────┬──────────────┘
         │
    VM receives packet
```

### Anti-spoofing

A VM can only send packets with its assigned source MAC and IP. Any attempt to spoof another VM's address is dropped at the nftables layer before reaching the bridge. This prevents ARP spoofing, IP spoofing, and MAC spoofing.

### Example rules

```
    Security group "web":
      Allow TCP 80, 443 from 0.0.0.0/0         (public web traffic)
      Allow TCP 22 from 10.0.0.0/16             (SSH from within VPC)
      Allow ICMP from 10.0.0.0/16               (ping from within VPC)

    Result: the VM accepts web traffic from anywhere, SSH only
    from the VPC, and all return traffic is auto-allowed.
```

## Routing

### Intra-subnet (same VPC subnet, across nodes)

Handled at L2 by VXLAN. VMs in the same subnet are on the same broadcast domain, even across physical nodes. No routing involved — just bridge forwarding via FDB entries.

### Inter-subnet (different subnets, same VPC)

Each node runs a **distributed router** for its local VPCs. The bridge IP (e.g., 10.0.1.1) serves as the default gateway for VMs on that subnet.

When a VM sends traffic to a different subnet in the same VPC:

```
    VM-A (10.0.1.5) → dst 10.0.2.9 (different subnet, same VPC)
         │
    1. VM-A sends to its default gateway (10.0.1.1 = bridge on local node)
    2. Node 1 routes: 10.0.2.0/24 → via VXLAN to Node 2
    3. Node 2 delivers to VM-B (10.0.2.9) via its local bridge
```

Inter-subnet routing within a VPC is automatic — no tenant configuration needed. Every subnet in a VPC can reach every other subnet by default (filtered by security groups).

### Internet egress (VM → internet)

Each node performs **SNAT (masquerade)** for outbound traffic from its local VMs. Traffic exits through the node's public interface with the node's public IP as the source.

```
    VM (10.0.1.5) → internet
         │
    1. Packet reaches the bridge gateway (10.0.1.1)
    2. Node routes to internet via public interface
    3. nftables SNAT: source 10.0.1.5 → node's public IP
    4. Response comes back, conntrack reverses the NAT
```

### Internet ingress (internet → VM)

**Floating IPs**: a public IP is mapped to a VM's private IP via DNAT on the hosting node.

```
    Internet → 203.0.113.50 (floating IP)
         │
    1. Packet arrives at the node hosting the VM
    2. nftables DNAT: dst 203.0.113.50 → 10.0.1.5
    3. Delivered to VM via bridge
    4. Responses: SNAT back to 203.0.113.50
```

### IPv6 public connectivity

Since Syfrah's fabric is IPv6-native and dedicated servers typically get IPv6 allocations, VMs can receive **public IPv6 addresses directly** — no NAT needed. The node routes the VM's IPv6 address via its public interface.

This is a significant simplification over IPv4: no NAT gateway, no floating IP management, no port mapping. IPv6 ingress and egress just work, filtered only by security groups.

## IP address management (IPAM)

### Allocation

The control plane centrally allocates IPs from each subnet's CIDR. No VM picks its own IP. Reserved addresses per /24 subnet:

| Address | Purpose |
|---|---|
| .0 | Network address |
| .1 | Gateway (bridge IP on each node) |
| .2 | Reserved (DNS, future use) |
| .255 | Broadcast |
| .3–.254 | Available for VMs (252 addresses) |

### MAC addresses

MAC addresses are derived deterministically from the VM's IP: `02:00:{IP bytes in hex}`. For example, IP `10.0.1.5` → MAC `02:00:0a:00:01:05`. This avoids MAC conflicts and eliminates the need for a MAC allocation service.

### Delivery to the VM

The VM learns its network configuration via one of:

1. **Config drive** (Phase 1) — a small virtual disk attached to the VM with network config as a file. Works with cloud-init on standard cloud images.
2. **DHCP** (later) — dnsmasq per subnet serves the pre-assigned IP. Standard, works with any OS.
3. **Metadata service** (later) — HTTP endpoint at 169.254.169.254, like AWS. VM queries its own configuration.

## Private DNS

Every VPC gets automatic private DNS. VMs can reach each other by name instead of IP — no configuration needed.

### Architecture

Each node runs a **CoreDNS** instance that serves DNS for all VPCs with VMs on that node. CoreDNS listens on each VPC bridge IP (the subnet gateway), so VMs naturally reach it as their DNS resolver.

```
    ┌──────────────────────────────────────────────────┐
    │  Node                                             │
    │                                                   │
    │  ┌─────────────────────────────────────────────┐  │
    │  │  CoreDNS (one per node)                     │  │
    │  │                                             │  │
    │  │  Listens on each bridge IP:                 │  │
    │  │    10.0.1.1 (VPC-A production subnet)       │  │
    │  │    10.0.2.1 (VPC-B staging subnet)          │  │
    │  │                                             │  │
    │  │  Serves zone files per VPC:                 │  │
    │  │    production.syfrah.internal → zone file A  │  │
    │  │    staging.syfrah.internal    → zone file B  │  │
    │  │                                             │  │
    │  │  Forwards external queries:                 │  │
    │  │    *.com, *.org, ... → 8.8.8.8, 1.1.1.1    │  │
    │  └─────────────────────────────────────────────┘  │
    │                                                   │
    │  ┌─────────────┐  ┌─────────────┐                 │
    │  │  VPC-A VMs  │  │  VPC-B VMs  │                 │
    │  │  DNS: 10.0.1.1  │  DNS: 10.0.2.1              │
    │  └─────────────┘  └─────────────┘                 │
    └──────────────────────────────────────────────────┘
```

### Domain scheme

```
    {vm-name}.{vpc-name}.syfrah.internal
```

Examples:
- `web-1.production.syfrah.internal` → 10.0.1.5
- `web-2.production.syfrah.internal` → 10.0.1.6
- `db-primary.production.syfrah.internal` → 10.0.2.10

With the search domain set to `{vpc-name}.syfrah.internal`, VMs can use short names:

```bash
# Inside a VM in the "production" VPC:
ping web-1                     # resolves to web-1.production.syfrah.internal
psql -h db-primary             # resolves to db-primary.production.syfrah.internal
curl http://api-gateway:8080   # resolves to api-gateway.production.syfrah.internal
```

### Auto-registration

DNS records are created and removed automatically by the control plane — no tenant action needed.

```
    VM created (API call)
         │
    1. Control plane allocates IP (10.0.1.5)
    2. Control plane writes DNS record:
       web-1.production.syfrah.internal → 10.0.1.5
    3. CoreDNS reloads zone file
         │
    VM boots
         │
    4. VM gets DNS resolver via DHCP or config drive
    5. VM can immediately resolve other VMs by name

    VM destroyed
         │
    1. Control plane removes DNS record
    2. CoreDNS reloads zone file
    3. Name no longer resolves
```

The tenant never manages DNS records for VMs. They exist automatically for as long as the VM exists.

### VPC isolation

DNS queries are scoped per VPC. A VM in VPC-A **cannot resolve names in VPC-B**. CoreDNS enforces this by serving different zone files depending on which bridge IP received the query.

```
    VM in VPC-A queries: db-primary.staging.syfrah.internal
         │
    Query arrives on 10.0.1.1 (VPC-A bridge)
         │
    CoreDNS: "staging" zone not served on this listener
         │
    → NXDOMAIN (name not found)
```

This prevents information leakage between tenants — you can't enumerate another VPC's VMs via DNS.

### Cross-VPC DNS (after peering)

When two VPCs are peered (see [organization-model.md](organization-model.md) for VPC peering), DNS can optionally be bridged:

```bash
# Enable DNS resolution between peered VPCs
syfrah vpc peer --from proj-a/production --to proj-b/production --enable-dns
```

After peering with DNS enabled:
- VMs in VPC-A can resolve `{name}.vpc-b.syfrah.internal`
- VMs in VPC-B can resolve `{name}.vpc-a.syfrah.internal`
- CoreDNS forwards queries for the peer VPC's zone to the peer's DNS

### Split-horizon

CoreDNS naturally handles split-horizon DNS:

- **Internal queries** (`*.syfrah.internal`) → resolved from local zone files
- **External queries** (`google.com`, `github.com`, etc.) → forwarded to public resolvers (8.8.8.8, 1.1.1.1)

VMs get working internet DNS out of the box. Internal names stay internal.

### Custom DNS records (future)

Tenants will be able to create custom DNS records within their VPC's zone:

```bash
# Create an alias
syfrah dns create --vpc production --name mydb --type A --value 10.0.2.10

# Create a CNAME
syfrah dns create --vpc production --name api --type CNAME --value web-1.production.syfrah.internal
```

This allows tenants to create service names (`mydb`, `cache`, `api`) that abstract away the underlying VM names.

## MTU

Double encapsulation (VXLAN inside WireGuard) adds ~130 bytes of overhead. The MTU at each layer:

| Layer | Interface | MTU |
|---|---|---|
| Physical NIC | eth0 | 1500 |
| WireGuard | syfrah0 | 1400 |
| VXLAN + bridge | vxlan-*, br-* | 1350 |
| VM TAP device | inside guest | 1350 |

The VM's MTU is set at TAP creation time and delivered via DHCP or config drive. With jumbo frames (9000 MTU on physical), VM MTU rises to ~8850 — standard 1500 fits comfortably.

## VPC lifecycle on the data plane

### First VM in a VPC lands on a node

```
    1. Create VXLAN interface (VNI = VPC ID)
    2. Create Linux bridge
    3. Attach VXLAN to bridge
    4. Set bridge IP as subnet gateway
    5. Add FDB entries for remote nodes in this VPC
    6. Create TAP for the VM, attach to bridge
    7. Apply security group rules (nftables)
    8. Add NAT masquerade rule for internet egress
```

### Additional VMs on the same node

```
    1. Create TAP for the VM
    2. Attach to existing bridge
    3. Apply security group rules
    4. Announce MAC/IP to remote nodes (they add FDB entries)
```

### Last VM in a VPC leaves a node

```
    1. Remove TAP device
    2. Remove FDB and neighbor entries from remote nodes
    3. Delete bridge and VXLAN interface (cleanup)
    4. Remove NAT rules
```

Resources are created on-demand and cleaned up when no longer needed.

## Performance

| Metric | Value |
|---|---|
| VM-to-VM throughput (10G NIC, 1500 MTU) | ~7.5–8.2 Gbps |
| VM-to-VM throughput (10G NIC, jumbo 9000) | ~8.5–9.0 Gbps |
| VM-to-VM latency overhead | ~0.1–0.15 ms |
| Max VPCs per node | 500+ (one bridge + VXLAN each) |
| Max VMs per node | 1000+ (one TAP each) |
| FDB entries per VXLAN | 10,000+ |

The bottleneck is WireGuard encryption (ChaCha20-Poly1305), not VXLAN encapsulation. VXLAN adds ~2–5% overhead; WireGuard adds ~10–15%. Combined: ~15–20% throughput reduction vs bare metal at 1500 MTU.

## Relationship to other concepts

```
    ┌──────────────────────────────────────────────────────┐
    │                                                      │
    │   Organization     ◄── org-model.md    │
    │   VPCs belong to projects/environments               │
    │                                                      │
    ├──────────────────────────────────────────────────────┤
    │                                                      │
    │   Overlay          ◄── this document                 │
    │   VXLAN + bridges + security groups + routing + DNS  │
    │                                                      │
    ├──────────────────────────────────────────────────────┤
    │                                                      │
    │   Compute          ◄── compute.md      │
    │   Firecracker VMs connect via TAP → bridge           │
    │                                                      │
    ├──────────────────────────────────────────────────────┤
    │                                                      │
    │   Storage          ◄── storage.md      │
    │   ZeroFS volumes attached to VMs (independent layer) │
    │                                                      │
    ├──────────────────────────────────────────────────────┤
    │                                                      │
    │   Forge            ◄── forge.md        │
    │   Creates bridges, TAPs, VXLAN, nftables per node    │
    │                                                      │
    ├──────────────────────────────────────────────────────┤
    │                                                      │
    │   Fabric           ◄── fabric.md       │
    │   WireGuard mesh carrying VXLAN traffic              │
    │                                                      │
    └──────────────────────────────────────────────────────┘
```
