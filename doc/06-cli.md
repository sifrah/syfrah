# CLI Reference

## Commands

### `syfrah init`

Create a new mesh network and start the daemon.

```bash
syfrah init --name production
syfrah init --name production --node-name dc1-node1 --port 51820 --endpoint 203.0.113.1:51820
```

| Flag | Default | Description |
|------|---------|-------------|
| `--name` | required | Mesh name |
| `--node-name` | hostname | Node name |
| `--port` | 51820 | WireGuard UDP port |
| `--endpoint` | 0.0.0.0:port | Public endpoint |
| `--peering-port` | port + 1 | TCP peering port |
| `-d, --daemon` | false | Run in background |

### `syfrah join`

Join an existing mesh. Just pass the IP of an existing node.

```bash
syfrah join 203.0.113.1                          # default port 51821
syfrah join 203.0.113.1:51821                    # explicit port
syfrah join 203.0.113.1 --pin 4829               # auto-accept with PIN
syfrah join 203.0.113.1 --node-name dc2-node1    # custom name
```

| Flag | Default | Description |
|------|---------|-------------|
| `--node-name` | hostname | Node name |
| `--port` | 51820 | WireGuard UDP port |
| `--endpoint` | auto-detected | Public endpoint |
| `--pin` | none | PIN for auto-accept |
| `-d, --daemon` | false | Run in background |

### `syfrah peering`

Manage peering. Without a subcommand, enters interactive mode.

```bash
# Interactive mode: watch for requests, prompt accept/reject
syfrah peering

# Interactive with auto-accept PIN
syfrah peering --pin 4829

# Non-interactive subcommands
syfrah peering start                    # start listener
syfrah peering start --pin 4829         # start with auto-accept
syfrah peering stop                     # stop listener
syfrah peering list                     # show pending requests
syfrah peering accept <request_id>      # accept a request
syfrah peering reject <request_id>      # reject a request
```

### `syfrah start`

Restart the daemon from saved state (after stop or crash).

```bash
syfrah start
syfrah start --daemon     # background
```

### `syfrah stop`

Stop the running daemon (sends SIGTERM).

### `syfrah status`

Show mesh info, daemon status, WireGuard stats, and metrics.

### `syfrah peers`

List all peers with WireGuard handshake times and traffic stats.

### `syfrah token`

Display the mesh secret for reference.

### `syfrah leave`

Tear down the WireGuard interface, remove control socket, delete all state.

## Typical Workflows

### First-time setup (2 servers)

```bash
# Server A
syfrah init --name prod --endpoint 203.0.113.1:51820
syfrah peering --pin 4829

# Server B
syfrah join 203.0.113.1 --pin 4829 --endpoint 198.51.100.5:51820
```

### Adding a third server

```bash
# Server C (Server A's peering is still active with PIN)
syfrah join 203.0.113.1 --pin 4829 --endpoint 192.0.2.10:51820
```

### Manual approval

```bash
# Server A
syfrah peering              # interactive, no PIN
# → sees request, types Y/n

# Server B
syfrah join 203.0.113.1     # waits for approval
```
