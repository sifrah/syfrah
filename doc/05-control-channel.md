# Control Channel

The CLI communicates with the running daemon via a Unix domain socket at `~/.syfrah/control.sock`.

## Protocol

Same length-prefixed JSON as the peering protocol:

```
[4 bytes: length, big-endian u32]
[N bytes: JSON payload]
```

Request-response: CLI sends a `ControlRequest`, daemon responds with a `ControlResponse`.

## Requests

| Request | Description |
|---------|-------------|
| `PeeringStart { port, pin }` | Activate peering listener, optionally with PIN |
| `PeeringStop` | Deactivate peering listener |
| `PeeringList` | List pending join requests |
| `PeeringAccept { request_id }` | Accept a pending request |
| `PeeringReject { request_id, reason }` | Reject a pending request |

## Responses

| Response | Description |
|----------|-------------|
| `Ok` | Success (no data) |
| `PeeringList { requests }` | List of `JoinRequestInfo` |
| `PeeringAccepted { peer_name }` | Peer was accepted |
| `Error { message }` | Operation failed |

## Security

The socket file is created with mode `0600` — only the daemon's user can connect.

**Source:** `crates/syfrah-net/src/control.rs`
