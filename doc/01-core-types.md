# Core Types

This document describes the foundational types in `syfrah-core`. These types are pure (no I/O, no async) and form the data model for the entire mesh network.

## Overview

```
syfrah-core/src/
  secret.rs      MeshSecret + MeshToken — shared secret, derives all keys
  identity.rs    NodeIdentity — a node's public identity
  addressing.rs  ULA IPv6 prefix generation + node address derivation
  mesh.rs        PeerRecord, PeerStatus, encrypted serialization
```

---

## MeshSecret (`secret.rs`)

The mesh secret is the single credential shared between all nodes of a mesh. Everything is derived from it.

### Key derivation

```
mesh_secret (256 bits, random)
  |
  |-- SHA256(secret)[0..16]           --> mesh_id         (identifies the mesh)
  |-- SHA256("topic:"  || secret)     --> dht_topic_key   (DHT lookup key)
  |-- SHA256("encrypt:" || secret)    --> encryption_key  (AES-256-GCM for DHT records)
  |-- SHA256("gossip:" || secret)     --> gossip_topic    (iroh-gossip topic)
```

All derivations are deterministic: same secret always produces the same keys.

### MeshToken

The user-facing credential combines the mesh secret with a bootstrap node's iroh PublicKey:

```
MeshToken (64 bytes total):
+-- 32 bytes --+-- 32 bytes --------+
| mesh_secret  | bootstrap_node_id  |
+--------------+--------------------+

Displayed as: syf_sk_{base58(64 bytes)}
```

The MeshSecret is extracted from the first 32 bytes for key derivation. The last 32 bytes are the bootstrap node's iroh ed25519 PublicKey, used to find them on the DHT.

### Security properties

| Without the secret | With the secret |
|-|-|
| Cannot derive the DHT topic | Can look up DHT records |
| Cannot decrypt DHT records | Can decrypt all peer records |
| Cannot subscribe to gossip | Can join gossip and discover peers |
| Cannot join the mesh | Can join and add a new node |

---

## NodeIdentity (`identity.rs`)

A node's public identity in the mesh.

```
NodeIdentity
  +-- name: String            human-readable name ("node-hetzner-1")
  +-- wg_public_key: String   WireGuard x25519 public key (base64)
```

The WireGuard private key is stored separately (not part of NodeIdentity). NodeIdentity is serializable (serde JSON) and can be exchanged freely.

---

## Addressing (`addressing.rs`)

IPv6-native addressing using Unique Local Addresses (ULA, `fd00::/8`).

### Mesh prefix

Generated once at mesh creation. A random `/48` prefix:

```
fd{40 random bits}::/48

Example: fd9a:bc12:7800::/48
         ||  |         |
         fd  random    /48 boundary
```

### Node address derivation

Each node's address is derived deterministically from the mesh prefix and its WireGuard public key:

```
prefix:  fd9a:bc12:7800 : 0000 : 0000 : 0000 : 0000 : 0000
                          |____________ filled from hash ___|

SHA256(wg_public_key) --> first 10 bytes --> segments [3..7]

Result:  fd9a:bc12:7800:a1f3:b2e4:c5d6:e7f8:0192/128
         |__ prefix __| |_____ from pubkey hash _____|
```

### Properties

- **Deterministic**: same key always produces the same address
- **No coordination**: no central allocator needed
- **Collision-resistant**: 80 bits from SHA256 = negligible collision probability
- **ULA-compliant**: stays within `fd00::/8`, no external allocation required

### Diagram: address space

```
fd00::/8  (ULA range)
  |
  +-- fd{mesh1}::/48  (mesh "production")
  |     +-- fd{mesh1}::a1f3:.../128  (node-1)
  |     +-- fd{mesh1}::b2e4:.../128  (node-2)
  |     +-- fd{mesh1}::c5d6:.../128  (node-3)
  |
  +-- fd{mesh2}::/48  (mesh "staging")
        +-- ...
```

---

## PeerRecord & Encryption (`mesh.rs`)

### PeerRecord

The data structure exchanged between nodes (via DHT and gossip).

```
PeerRecord
  +-- name: String              "node-hetzner-1"
  +-- wg_public_key: String     WireGuard public key (base64)
  +-- endpoint: SocketAddr      public IP:port for WireGuard (e.g. 203.0.113.1:51820)
  +-- mesh_ipv6: Ipv6Addr       ULA address in the mesh
  +-- last_seen: u64            unix timestamp of last heartbeat
  +-- status: PeerStatus        Active | Unreachable | Removed
```

### PeerStatus lifecycle

```
           join
            |
            v
  +-----> Active <------+
  |         |            |
  |    5min no heartbeat |
  |         |         heartbeat
  |         v          resumes
  |     Unreachable ----+
  |
  |     syfrah leave / syfrah net remove
  |         |
  |         v
  +------ Removed  (tombstone, GC after 24h)
```

### Encrypted serialization

Records stored on the public DHT are encrypted with AES-256-GCM using the mesh's `encryption_key`.

```
Encrypted payload layout:

+-- 12 bytes --+-- variable length --+
|    nonce     |     ciphertext      |
+--------------+---------------------+

nonce:      random, generated per encryption
ciphertext: AES-256-GCM(key=encryption_key, nonce, plaintext=JSON(PeerRecord))
```

An attacker without the secret sees only opaque blobs on the DHT. The nonce ensures that encrypting the same record twice produces different ciphertexts.
