# Discovery Layer

This document describes the iroh-based peer discovery and gossip layer in `syfrah-net/src/discovery.rs`. This is the control plane of the mesh — it handles how nodes find each other and exchange WireGuard configuration.

## Overview

```
syfrah-net/src/discovery.rs
  |
  |-- iroh 0.97       (QUIC endpoint, PKARR/DHT address publishing)
  |-- iroh-gossip     (topic-based encrypted broadcast)
  |
  +-- MeshNode: init/join/broadcast/event loop/shutdown
```

---

## Architecture: Two Planes

```
                        Internet
                           |
    +----------------------|----------------------+
    |            Control Plane (iroh)             |
    |                                             |
    |  PKARR/DHT -----> resolve NodeId to IP      |
    |  Relay servers --> NAT traversal (~100%)     |
    |  Gossip --------> broadcast PeerRecords     |
    |                   (encrypted with AES-256)  |
    +---------------------------------------------+
                           |
              PeerRecords (wg pubkey, endpoint, IPv6)
                           |
    +---------------------------------------------+
    |            Data Plane (WireGuard)            |
    |                                             |
    |  syfrah0 interface                          |
    |  x25519 tunnels between nodes               |
    |  ULA IPv6 addressing                        |
    +---------------------------------------------+
```

The control plane discovers peers and tells the data plane who to connect to.

---

## The Bootstrap Problem

To join gossip, you need at least one peer. To find a peer, you need gossip. This is the bootstrap problem.

### Solution: MeshToken

The `MeshToken` embeds the bootstrap node's iroh `PublicKey` (32 bytes) alongside the mesh secret (32 bytes):

```
MeshToken (64 bytes total):
+-- 32 bytes --+-- 32 bytes --------+
| mesh_secret  | bootstrap_node_id  |
+--------------+--------------------+

Display format: syf_sk_{base58(64 bytes)}
Example:        syf_sk_5HueCGU8rMjxEXxiPuD5BDku4MkFqeZyd4dZ1jvhTVqP...
```

### How bootstrap works

```
1. syfrah init
   |
   +-- Generate mesh_secret (32 bytes random)
   +-- Generate iroh SecretKey --> PublicKey = bootstrap_node_id
   +-- MeshToken = mesh_secret || bootstrap_node_id
   +-- Endpoint auto-publishes to PKARR/DHT
   +-- Subscribe to gossip topic (no bootstrap peers, wait)
   +-- Display: syf_sk_...

2. syfrah join <token>
   |
   +-- Parse token --> mesh_secret + bootstrap_node_id
   +-- Generate own iroh SecretKey
   +-- Create endpoint with DHT address lookup
   +-- DHT resolves bootstrap_node_id --> actual IP/relay
   +-- subscribe_and_join(topic, [bootstrap_node_id])
   +-- Gossip connects through relay (NAT traversal)
   +-- Receive PeerRecords from existing nodes
   +-- Broadcast own PeerRecord
```

### After bootstrap

Once a node has joined the gossip, it discovers all other nodes through gossip messages. The bootstrap node is only needed for the initial connection. If the bootstrap node goes offline, existing members can still gossip with each other. New nodes would need a fresh token from a running node (`syfrah token`).

---

## Gossip Protocol

### Topic derivation

```
gossip_topic = SHA256("gossip:" || mesh_secret)  -->  TopicId (32 bytes)
```

All nodes with the same mesh secret subscribe to the same gossip topic.

### Message format

Messages on the gossip topic are encrypted PeerRecords:

```
Gossip message payload:
+-- 12 bytes --+-- variable ----------+
|    nonce     | AES-256-GCM(          |
|              |   key = encryption_key |
|              |   payload = JSON(     |
|              |     PeerRecord        |
|              |   )                   |
|              | )                     |
+--------------+-----------------------+
```

Without the mesh secret, gossip messages are opaque blobs.

### Event types

```
Event::Received(msg)      -->  Decrypt --> PeerRecord --> update peer list
Event::NeighborUp(id)     -->  A gossip neighbor connected
Event::NeighborDown(id)   -->  A gossip neighbor disconnected
```

---

## MeshNode Lifecycle

### Init (first node)

```rust
let (node, receiver, token) = MeshNode::init(mesh_secret, iroh_key).await?;
// token is the shareable MeshToken
// receiver is the gossip event stream
```

1. Creates iroh Endpoint with PKARR/DHT + relay
2. Waits for relay connection (`.online().await`)
3. Creates Gossip instance + Router
4. Subscribes to gossip topic (empty bootstrap → waits for peers)
5. Returns the `MeshToken` to share

### Join (subsequent nodes)

```rust
let (node, receiver) = MeshNode::join(&token, iroh_key).await?;
```

1. Extracts bootstrap PublicKey from token
2. Creates endpoint with DHT address lookup
3. Subscribes to gossip with bootstrap peer
4. DHT resolves bootstrap_node_id → relay/IP
5. Gossip connects and syncs

### Event loop

```rust
node.run_event_loop(receiver, callback).await?;
```

Processes gossip events in a loop:
- Decrypts received messages
- Updates the internal peer list (upsert by WG public key)
- Dispatches `PeerEvent` to the callback

### Broadcast

```rust
node.broadcast_peer_record(&my_record).await?;
```

Encrypts the PeerRecord with the mesh encryption key and broadcasts via gossip.

### Shutdown

```rust
node.shutdown().await?;
```

Gracefully shuts down the Router and Endpoint.

---

## Address Resolution

iroh handles address resolution automatically through multiple mechanisms:

```
PublicKey (NodeId)
  |
  +-- PKARR/DHT lookup (BitTorrent Mainline DHT)
  |     Returns: relay URL + direct socket addresses
  |
  +-- N0 DNS relay
  |     Returns: relay URL
  |
  +-- Memory lookup (manual entries)
        For testing or out-of-band address sharing
```

When a node does `subscribe_and_join(topic, [bootstrap_pk])`, iroh:
1. Looks up `bootstrap_pk` via PKARR/DHT
2. Finds the bootstrap node's relay URL + IP
3. Connects through relay (guaranteed NAT traversal)
4. Optionally upgrades to direct UDP connection via hole punching

---

## State Management

```
MeshNode
  +-- endpoint: Endpoint        iroh QUIC endpoint
  +-- _gossip: Gossip           gossip protocol handler
  +-- router: Router            dispatches incoming connections
  +-- sender: GossipSender      broadcast to gossip topic
  +-- mesh_secret: MeshSecret   for encryption/decryption
  +-- peers: Arc<RwLock<Vec<PeerRecord>>>   current known peers
```

The peer list is updated atomically on each gossip event. It is shared (Arc) so the daemon can read it for WireGuard reconciliation and the CLI can read it for `syfrah peers`.

---

## Dependencies

| Crate | Purpose |
|-------|---------|
| `iroh` 0.97 | QUIC endpoint, PKARR/DHT address publishing, relay NAT traversal |
| `iroh-gossip` 0.97 | Topic-based gossip broadcast (HyParView + PlumTree) |
| `bytes` | Zero-copy byte buffers for gossip messages |
| `futures-lite` | `StreamExt::try_next()` for gossip receiver |

Feature flag: `iroh` with `address-lookup-pkarr-dht` for DHT support.
