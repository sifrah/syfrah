# Peering Protocol

Syfrah uses manual peering — no automatic discovery. An operator explicitly approves each node that joins the mesh.

## Wire Protocol

TCP connections on the peering port (default: WG port + 1 = 51821). Messages are length-prefixed JSON:

```
[4 bytes: payload length, big-endian u32]
[N bytes: JSON-encoded PeeringMessage]
```

## Message Types

```rust
enum PeeringMessage {
    JoinRequest(JoinRequest),      // new node → existing node
    JoinResponse(JoinResponse),    // existing node → new node
    PeerAnnounce(Vec<u8>),         // encrypted PeerRecord between members
}
```

## Join Flow

```
New Node                              Existing Node
   |                                        |
   |--- TCP connect to peering port ------> |
   |--- JoinRequest ----------------------> |
   |    (name, wg_pubkey, endpoint, pin?)   |
   |                                        | stores in pending
   |                                        | operator sees request
   |                                        | operator accepts/rejects
   |                                        |   (or PIN auto-accepts)
   | <--- JoinResponse ------------------- |
   |    (mesh_secret, prefix, all peers)    |
   |                                        |
   | configures WG, starts daemon           | adds new peer to WG
   |                                        | announces to all peers
```

### Endpoint Auto-Detection

If the joining node doesn't specify `--endpoint`, the existing node uses the TCP connection's source IP combined with the WG listen port.

### PIN Auto-Accept

When the existing node runs `syfrah peering --pin 4829`, any join request with a matching PIN is accepted immediately without operator intervention.

## Peer Announcement

After accepting a new node, the existing node announces it to all known mesh members:

1. Connect to each peer's IP on the peering port
2. Send `PeeringMessage::PeerAnnounce(encrypted_peer_record)`
3. The receiving node decrypts with the mesh secret and adds the peer to WireGuard

This ensures full mesh convergence without requiring the new node to contact every peer.

## Timeouts

| Timeout | Duration | Context |
|---------|----------|---------|
| Join approval | 5 minutes | Time for operator to accept/reject |
| TCP connect | 30 seconds | Connecting to a peer |

**Source:** `crates/syfrah-net/src/peering.rs`
