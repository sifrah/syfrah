# Init & Join Flows

This document describes the `syfrah init` and `syfrah join` commands — the two entry points to create or join a mesh network.

## CLI Usage

### Create a new mesh

```bash
syfrah init --name production [--node-name node-1] [--port 51820] [--endpoint 203.0.113.1:51820]
```

Output:
```
Mesh 'production' created.
  Secret: syf_sk_5HueCGU8rMjxEXxiPuD5BDku4MkFq...
  Node:   node-1 (fd9a:bc12:7800::a1f3:1)

Share the secret with other nodes to join.
Running daemon... (Ctrl+C to stop)
```

### Join an existing mesh

```bash
syfrah join syf_sk_5HueCGU8rMjxEXxiPuD5BDku4MkFq... [--node-name node-2] [--port 51820]
```

Output:
```
Joining mesh...
Joined mesh 'mesh'.
  Node: node-2 (fd9a:bc12:7800::b2e4:2)
Running daemon... (Ctrl+C to stop)
```

---

## Init Flow

```
syfrah init --name production
  |
  1. Generate MeshSecret (32 bytes random)
  2. Generate iroh SecretKey (ed25519)
  3. Generate WireGuard KeyPair (x25519)
  4. Derive mesh prefix from secret (deterministic fd::/48)
  5. Derive node IPv6 from prefix + WG pubkey
  |
  6. MeshNode::init()
  |    +-- Create iroh Endpoint (auto-publish to PKARR/DHT)
  |    +-- Wait for relay connection
  |    +-- Subscribe to gossip topic (no bootstrap, waits)
  |    +-- Return MeshToken (secret + our iroh PublicKey)
  |
  7. wg::setup_interface()
  |    +-- Create syfrah0 with private key + port
  |    +-- Assign ULA IPv6 address
  |    +-- Bring interface up
  |
  8. store::save() --> ~/.syfrah/state.json
  9. Print token to user
  10. Run daemon loop
```

## Join Flow

```
syfrah join <token>
  |
  1. Parse MeshToken --> MeshSecret + bootstrap NodeId
  2. Generate iroh SecretKey (ed25519)
  3. Generate WireGuard KeyPair (x25519)
  4. Derive mesh prefix from secret (same as init, deterministic)
  5. Derive node IPv6 from prefix + WG pubkey
  |
  6. MeshNode::join()
  |    +-- Create iroh Endpoint (auto-publish to PKARR/DHT)
  |    +-- Wait for relay connection
  |    +-- Resolve bootstrap NodeId via DHT --> find their relay/IP
  |    +-- subscribe_and_join(topic, [bootstrap_pk])
  |    +-- Gossip connects through relay (NAT traversal)
  |
  7. wg::setup_interface()
  8. store::save() --> ~/.syfrah/state.json
  9. Run daemon loop
```

---

## Daemon Loop

After init or join, the daemon runs three concurrent tasks:

```
tokio::select!
  |
  +-- Event loop (gossip)
  |     Receives encrypted PeerRecords from gossip
  |     Decrypts with mesh encryption key
  |     Updates peer list
  |     Reconciles WireGuard (apply_peers)
  |
  +-- Heartbeat (every 60s)
  |     Re-broadcasts our PeerRecord with updated last_seen
  |
  +-- Persist (every 30s)
  |     Saves current peer list to ~/.syfrah/state.json
  |
  +-- Ctrl+C handler
        Tears down WireGuard interface
        Shuts down iroh endpoint
```

### WireGuard Reconciliation

Every time a new PeerRecord is received via gossip:

```
PeerRecord received
  |
  +-- Decrypt with AES-256-GCM
  +-- Upsert in peer list (by WG public key)
  +-- wg::apply_peers()
       +-- DeviceUpdate::new().replace_peers()
       +-- Add each active peer with:
       |     - WG public key
       |     - endpoint (IP:port)
       |     - allowed_ip = mesh_ipv6/128
       |     - keepalive = 25s
       +-- apply("syfrah0")
```

---

## Mesh Prefix Derivation

The mesh prefix is derived deterministically from the secret so all nodes compute the same prefix without coordination:

```
SHA256("mesh-prefix:" || secret) --> first 5 bytes --> fd{5 bytes}::/48
```

This means: same secret = same mesh prefix = same address space.

---

## State Persistence

State is stored in `~/.syfrah/state.json`:

```json
{
  "mesh_name": "production",
  "mesh_token": "syf_sk_...",
  "wg_private_key": "base64...",
  "wg_public_key": "base64...",
  "iroh_secret_key": "hex...",
  "mesh_ipv6": "fd9a:bc12:7800::a1f3:1",
  "mesh_prefix": "fd9a:bc12:7800::",
  "wg_listen_port": 51820,
  "node_name": "node-1",
  "peers": [...]
}
```

- Written atomically (tmp + rename)
- Permissions 0600 on Unix (contains private keys)
- `syfrah leave` deletes the entire `~/.syfrah/` directory

---

## Other Commands

### syfrah status

Reads `~/.syfrah/state.json` and displays mesh info.

### syfrah peers

Reads persisted peer list and displays a table:
```
NAME                 MESH IP                                    ENDPOINT                     STATUS
node-1               fd9a:bc12:7800::a1f3:1                     203.0.113.1:51820              active
node-2               fd9a:bc12:7800::b2e4:2                     198.51.100.5:51820             active
```

### syfrah leave

1. Tears down WireGuard interface `syfrah0`
2. Deletes `~/.syfrah/` directory
3. Does NOT announce departure via gossip (future improvement)
