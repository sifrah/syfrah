# Core Types

## MeshSecret

The single shared credential for a mesh. Format: `syf_sk_{base58(32 bytes)}`.

```
mesh_secret (256 bits)
  ├── encryption_key    AES-256-GCM key for peer record encryption
  ├── mesh_id           16-byte identifier (for display)
  └── mesh_prefix       deterministic fd::/48 ULA prefix
```

All keys are derived via `SHA-256(domain_prefix + secret)`.

**Source:** `crates/syfrah-core/src/secret.rs`

## PeerRecord

Describes a mesh peer. Exchanged during peering and announcements.

| Field | Type | Description |
|-------|------|-------------|
| `name` | String | Node name (hostname) |
| `wg_public_key` | String | WireGuard x25519 public key (base64) |
| `endpoint` | SocketAddr | Public IP:port for WireGuard |
| `mesh_ipv6` | Ipv6Addr | Mesh-internal IPv6 /128 address |
| `last_seen` | u64 | UNIX timestamp of last activity |
| `status` | PeerStatus | Active / Unreachable / Removed |

**Source:** `crates/syfrah-core/src/mesh.rs`

## Addressing

- **Mesh prefix**: `fd{40 bits from secret}::/48` — deterministic from the mesh secret
- **Node address**: `prefix + SHA-256(wg_public_key)[0:10]` — deterministic /128 within the /48

Two nodes with the same secret always share the same prefix. Different WG keys produce different addresses.

**Source:** `crates/syfrah-core/src/addressing.rs`

## Encryption

PeerRecords are encrypted with AES-256-GCM before transmission between mesh members.

- **Key**: derived from mesh secret (`encryption_key`)
- **Format**: `nonce (12 bytes) || ciphertext`
- **Used in**: peer announcements between established mesh members

**Source:** `crates/syfrah-core/src/mesh.rs` (`encrypt_record` / `decrypt_record`)
