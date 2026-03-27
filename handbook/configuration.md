# Configuration

## Overview

The syfrah daemon reads optional configuration from `~/.syfrah/config.toml` at startup. Every setting has a sensible default, so the file is not required. Create it only when you need to tune behavior for your environment.

If the file does not exist, the daemon starts with built-in defaults. If the file exists but contains invalid TOML syntax or type mismatches (e.g. a string where an integer is expected), the daemon refuses to start and prints the parse error. Unrecognized keys are silently ignored — double-check spelling if a setting does not seem to take effect.

## File location

```
~/.syfrah/config.toml
```

## Sections

### `[daemon]`

Controls the daemon's background loop timers.

| Key | Type | Default | Description |
|---|---|---|---|
| `health_check_interval` | integer (seconds) | `60` | How often the daemon checks peer health via WireGuard handshake timestamps. |
| `reconcile_interval` | integer (seconds) | `30` | How often the daemon reconciles WireGuard interface state with the peer list. |
| `persist_interval` | integer (seconds) | `30` | How often the daemon writes in-memory state to `~/.syfrah/state.json`. |
| `unreachable_timeout` | integer (seconds) | `300` | After this many seconds without a WireGuard handshake, a peer is marked unreachable. Must be greater than `health_check_interval` (see note below). |

> **Note:** `health_check_interval` must be less than `unreachable_timeout`. The health check runs periodically and compares each peer's last WireGuard handshake against the unreachable timeout. If `health_check_interval` were equal to or greater than `unreachable_timeout`, the daemon could miss the detection window entirely or detect unreachable peers with excessive delay. The defaults (60s check interval, 300s timeout) provide up to five health check opportunities before a peer is marked unreachable, giving a worst-case detection time of approximately 6 minutes (300s timeout + up to 60s until the next check).

### `[wireguard]`

WireGuard interface settings.

| Key | Type | Default | Description |
|---|---|---|---|
| `interface_name` | string | `"syfrah0"` | WireGuard interface name. The default `syfrah0` is hardcoded in `layers/fabric/src/wg.rs` as `DEFAULT_INTERFACE_NAME`. Override this if you need a different interface name (e.g., to avoid conflicts with existing WireGuard interfaces). Must not be empty. |
| `keepalive_interval` | integer (seconds) | `25` | WireGuard persistent keepalive interval. Keeps NAT mappings alive. |

### `[peering]`

Controls the TCP peering protocol used for join requests and peer announcements.

| Key | Type | Default | Description |
|---|---|---|---|
| `join_timeout` | integer (seconds) | `300` | How long a joining node waits for its request to be accepted before giving up. |
| `exchange_timeout` | integer (seconds) | `30` | Timeout for individual peering protocol message exchanges. |
| `max_concurrent_connections` | integer | `100` | Maximum number of simultaneous TCP peering connections the daemon accepts. |
| `max_pending_joins` | integer | `100` | Maximum number of pending join requests the daemon holds before rejecting new ones. |

> **Note:** The peering port (`peering_port`) is not set in `config.toml`. It is specified at mesh creation or join time via the `--peering-port` CLI flag (default: WireGuard port + 1) and persisted in the node's state database.

### `[gateway]`

Controls the dedicated gateway node role. When enabled, the node exposes the external REST/gRPC API with TLS. See [external-api.md](external-api.md) for full gateway documentation.

| Key | Type | Default | Description |
|---|---|---|---|
| `enabled` | boolean | `false` | Whether this node acts as a gateway. |
| `bind_address` | string (ip:port) | `0.0.0.0:8443` | Socket address to bind the external API to. |
| `tls_cert_path` | string (file path) | *(none)* | Path to a PEM-encoded TLS certificate. If omitted, a self-signed certificate is generated at startup. |
| `tls_key_path` | string (file path) | *(none)* | Path to a PEM-encoded TLS private key. Required when `tls_cert_path` is set. |

### `[events]`

Event log settings.

| Key | Type | Default | Description |
|---|---|---|---|
| `max_events` | integer | `100` | Maximum number of events kept in the in-memory ring buffer. Older events are dropped when the limit is reached. |

### `[limits]`

Hard limits that protect the daemon from resource exhaustion.

| Key | Type | Default | Description |
|---|---|---|---|
| `max_peers` | integer | `1000` | Maximum number of peers allowed in the mesh. The daemon rejects new joins once this limit is reached. |
| `max_concurrent_announces` | integer | `50` | Maximum number of peer announcement tasks that run in parallel. |

## Example config.toml

A minimal file that only overrides what you need:

```toml
# ~/.syfrah/config.toml

[daemon]
health_check_interval = 30    # check peer health every 30s instead of 60s
unreachable_timeout = 600     # tolerate 10 minutes of silence before marking unreachable

[limits]
max_peers = 500               # smaller mesh, tighter limit
```

A full file with every option set to its default value:

```toml
# ~/.syfrah/config.toml — all defaults shown

[daemon]
health_check_interval = 60
reconcile_interval = 30
persist_interval = 30
unreachable_timeout = 300

[wireguard]
keepalive_interval = 25

[peering]
join_timeout = 300
exchange_timeout = 30
max_concurrent_connections = 100
max_pending_joins = 100

[events]
max_events = 100

[gateway]
enabled = false
bind_address = "0.0.0.0:8443"
# tls_cert_path = "/etc/syfrah/tls/cert.pem"
# tls_key_path  = "/etc/syfrah/tls/key.pem"

[limits]
max_peers = 1000
max_concurrent_announces = 50
```

## When changes take effect

The daemon reads `config.toml` once at startup. To apply changes, restart the daemon:

```bash
syfrah fabric stop
syfrah fabric start
```

## Source

The configuration is defined in `layers/fabric/src/config.rs`.
