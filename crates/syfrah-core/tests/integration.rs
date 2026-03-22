use syfrah_core::addressing;
use syfrah_core::mesh::{decrypt_record, encrypt_record, PeerRecord, PeerStatus};
use syfrah_core::secret::MeshSecret;

/// Full flow: secret → parse → addresses → encrypt/decrypt.
#[test]
fn full_secret_to_encrypted_record_flow() {
    let secret = MeshSecret::generate();

    // Roundtrip
    let secret_str = secret.to_string();
    let parsed: MeshSecret = secret_str.parse().unwrap();
    assert_eq!(parsed.as_bytes(), secret.as_bytes());

    // Derive addresses
    let prefix = derive_prefix(&parsed);
    let key_a = b"wireguard-public-key-node-aaaaa";
    let key_b = b"wireguard-public-key-node-bbbbb";
    let addr_a = addressing::derive_node_address(&prefix, key_a);
    let addr_b = addressing::derive_node_address(&prefix, key_b);
    assert_ne!(addr_a, addr_b);
    assert_eq!(addr_a.segments()[0], prefix.segments()[0]);

    // Encrypt/decrypt
    let record_a = PeerRecord {
        name: "node-a".into(),
        wg_public_key: "key-a-base64".into(),
        endpoint: "203.0.113.1:51820".parse().unwrap(),
        mesh_ipv6: addr_a,
        last_seen: 1700000000,
        status: PeerStatus::Active,
        iroh_node_id: None,
    };

    let enc_key = parsed.encryption_key();
    let encrypted = encrypt_record(&record_a, &enc_key).unwrap();
    let decrypted = decrypt_record(&encrypted, &enc_key).unwrap();
    assert_eq!(decrypted.name, "node-a");

    // Wrong key fails
    let wrong = MeshSecret::generate();
    assert!(decrypt_record(&encrypted, &wrong.encryption_key()).is_err());
}

/// All derivations distinct.
#[test]
fn all_derivations_distinct() {
    let secret = MeshSecret::generate();
    assert_ne!(secret.encryption_key(), secret.ipfs_key());
}

/// IPFS key deterministic.
#[test]
fn ipfs_key_deterministic() {
    let secret = MeshSecret::generate();
    assert_eq!(secret.ipfs_key_hex(), secret.ipfs_key_hex());

    let other = MeshSecret::generate();
    assert_ne!(secret.ipfs_key_hex(), other.ipfs_key_hex());
}

/// Multiple nodes derive distinct addresses.
#[test]
fn many_nodes_unique_addresses() {
    let prefix = addressing::generate_mesh_prefix();
    let mut addrs = std::collections::HashSet::new();
    for i in 0..100 {
        let key = format!("node-key-{i:04}");
        let addr = addressing::derive_node_address(&prefix, key.as_bytes());
        assert!(addrs.insert(addr), "collision at node {i}");
    }
}

/// PeerRecord serde roundtrip.
#[test]
fn peer_record_json_roundtrip() {
    let record = PeerRecord {
        name: "test-node".into(),
        wg_public_key: "base64key==".into(),
        endpoint: "[::1]:51820".parse().unwrap(),
        mesh_ipv6: "fd12::1".parse().unwrap(),
        last_seen: 999,
        status: PeerStatus::Unreachable,
        iroh_node_id: None,
    };
    let json = serde_json::to_string(&record).unwrap();
    let parsed: PeerRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.name, "test-node");
    assert_eq!(parsed.status, PeerStatus::Unreachable);
}

fn derive_prefix(secret: &MeshSecret) -> std::net::Ipv6Addr {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest([b"mesh-prefix:" as &[u8], secret.as_bytes()].concat());
    std::net::Ipv6Addr::new(
        0xfd00 | (hash[0] as u16),
        ((hash[1] as u16) << 8) | (hash[2] as u16),
        ((hash[3] as u16) << 8) | (hash[4] as u16),
        0, 0, 0, 0, 0,
    )
}
