use std::net::Ipv6Addr;

use sha2::{Digest, Sha256};

/// Generate a random ULA /48 mesh prefix: fd{40 random bits}::/48
pub fn generate_mesh_prefix() -> Ipv6Addr {
    let mut rng_bytes = [0u8; 5];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut rng_bytes);

    // fd{5 bytes}::
    let segments: [u16; 8] = [
        0xfd00 | (rng_bytes[0] as u16),
        ((rng_bytes[1] as u16) << 8) | (rng_bytes[2] as u16),
        ((rng_bytes[3] as u16) << 8) | (rng_bytes[4] as u16),
        0,
        0,
        0,
        0,
        0,
    ];

    Ipv6Addr::new(
        segments[0],
        segments[1],
        segments[2],
        segments[3],
        segments[4],
        segments[5],
        segments[6],
        segments[7],
    )
}

/// Derive a node's ULA /128 address from the mesh prefix and the WG public key.
///
/// Takes the first 10 bytes (80 bits) of SHA256(wg_pubkey) and fills
/// the lower 80 bits of the address, keeping the /48 prefix intact.
pub fn derive_node_address(mesh_prefix: &Ipv6Addr, wg_public_key: &[u8]) -> Ipv6Addr {
    let hash = Sha256::digest(wg_public_key);
    let prefix = mesh_prefix.segments();

    // Keep the /48 prefix (first 3 segments), fill the rest from hash
    Ipv6Addr::new(
        prefix[0],
        prefix[1],
        prefix[2],
        ((hash[0] as u16) << 8) | (hash[1] as u16),
        ((hash[2] as u16) << 8) | (hash[3] as u16),
        ((hash[4] as u16) << 8) | (hash[5] as u16),
        ((hash[6] as u16) << 8) | (hash[7] as u16),
        ((hash[8] as u16) << 8) | (hash[9] as u16),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_prefix_is_ula() {
        let prefix = generate_mesh_prefix();
        let octets = prefix.octets();
        // ULA always starts with fd
        assert_eq!(octets[0], 0xfd);
        // Lower 10 bytes should be zero (/48 prefix)
        assert_eq!(&octets[6..16], &[0u8; 10]);
    }

    #[test]
    fn node_address_preserves_prefix() {
        let prefix = generate_mesh_prefix();
        let addr = derive_node_address(&prefix, b"test-public-key");
        let prefix_segs = prefix.segments();
        let addr_segs = addr.segments();
        // First 3 segments (/48) must match
        assert_eq!(prefix_segs[0], addr_segs[0]);
        assert_eq!(prefix_segs[1], addr_segs[1]);
        assert_eq!(prefix_segs[2], addr_segs[2]);
    }

    #[test]
    fn node_address_is_deterministic() {
        let prefix = Ipv6Addr::new(0xfd12, 0x3456, 0x7800, 0, 0, 0, 0, 0);
        let a1 = derive_node_address(&prefix, b"key-a");
        let a2 = derive_node_address(&prefix, b"key-a");
        assert_eq!(a1, a2);
    }

    #[test]
    fn different_keys_different_addresses() {
        let prefix = Ipv6Addr::new(0xfd12, 0x3456, 0x7800, 0, 0, 0, 0, 0);
        let a1 = derive_node_address(&prefix, b"key-a");
        let a2 = derive_node_address(&prefix, b"key-b");
        assert_ne!(a1, a2);
    }
}
