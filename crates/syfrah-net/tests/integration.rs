use std::net::{Ipv6Addr, SocketAddr};

use syfrah_core::addressing;
use syfrah_core::mesh::{decrypt_record, encrypt_record, PeerRecord, PeerStatus};
use syfrah_core::secret::MeshSecret;
use syfrah_net::store::NodeState;
use syfrah_net::wg;

/// Full init flow without network: secret → addresses → state roundtrip.
#[test]
fn init_flow_no_network() {
    let secret = MeshSecret::generate();
    let wg_keypair = wg::generate_keypair();

    let mesh_prefix = derive_prefix(&secret);
    let mesh_ipv6 = addressing::derive_node_address(&mesh_prefix, wg_keypair.public.as_bytes());

    // Secret roundtrip
    let secret_str = secret.to_string();
    let parsed: MeshSecret = secret_str.parse().unwrap();
    assert_eq!(parsed.as_bytes(), secret.as_bytes());

    let state = NodeState {
        mesh_name: "test-mesh".into(),
        mesh_secret: secret_str,
        wg_private_key: wg_keypair.private.to_base64(),
        wg_public_key: wg_keypair.public.to_base64(),
        mesh_ipv6,
        mesh_prefix,
        wg_listen_port: 51820,
        node_name: "node-test".into(),
        public_endpoint: None,
        ipfs_api: None,
        peers: vec![],
        metrics: Default::default(),
    };

    let json = serde_json::to_string_pretty(&state).unwrap();
    let loaded: NodeState = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded.mesh_name, "test-mesh");
    assert_eq!(loaded.mesh_ipv6, mesh_ipv6);
}

/// Join: same secret → same prefix, different keys → different addresses.
#[test]
fn join_flow_derives_same_prefix() {
    let secret = MeshSecret::generate();

    let kp_init = wg::generate_keypair();
    let prefix = derive_prefix(&secret);
    let addr_init = addressing::derive_node_address(&prefix, kp_init.public.as_bytes());

    // "Join" node uses same secret
    let parsed: MeshSecret = secret.to_string().parse().unwrap();
    let kp_join = wg::generate_keypair();
    let prefix_join = derive_prefix(&parsed);
    let addr_join = addressing::derive_node_address(&prefix_join, kp_join.public.as_bytes());

    assert_eq!(prefix, prefix_join);
    assert_ne!(addr_init, addr_join);
    assert_eq!(addr_init.segments()[0], addr_join.segments()[0]);
}

/// Encrypted record exchange between nodes.
#[test]
fn encrypted_record_exchange() {
    let secret = MeshSecret::generate();
    let enc_key = secret.encryption_key();
    let prefix = derive_prefix(&secret);

    let kp_a = wg::generate_keypair();
    let addr_a = addressing::derive_node_address(&prefix, kp_a.public.as_bytes());
    let record_a = PeerRecord {
        name: "node-a".into(),
        wg_public_key: kp_a.public.to_base64(),
        endpoint: "203.0.113.1:51820".parse().unwrap(),
        mesh_ipv6: addr_a,
        last_seen: 1000,
        status: PeerStatus::Active,
        iroh_node_id: None,
    };

    let encrypted = encrypt_record(&record_a, &enc_key).unwrap();
    let decrypted = decrypt_record(&encrypted, &enc_key).unwrap();
    assert_eq!(decrypted.name, "node-a");

    let wrong = MeshSecret::generate();
    assert!(decrypt_record(&encrypted, &wrong.encryption_key()).is_err());
}

/// Peer list upsert and tombstone.
#[test]
fn peer_list_upsert_and_tombstone() {
    let mut peers: Vec<PeerRecord> = Vec::new();

    let record = PeerRecord {
        name: "node-1".into(),
        wg_public_key: "pubkey-1".into(),
        endpoint: "1.2.3.4:51820".parse().unwrap(),
        mesh_ipv6: "fd12::1".parse().unwrap(),
        last_seen: 1000,
        status: PeerStatus::Active,
        iroh_node_id: None,
    };

    peers.push(record.clone());

    let updated = PeerRecord { last_seen: 2000, ..record.clone() };
    if let Some(existing) = peers.iter_mut().find(|p| p.wg_public_key == updated.wg_public_key) {
        *existing = updated;
    }
    assert_eq!(peers[0].last_seen, 2000);

    let record2 = PeerRecord {
        name: "node-2".into(),
        wg_public_key: "pubkey-2".into(),
        endpoint: "5.6.7.8:51820".parse().unwrap(),
        mesh_ipv6: "fd12::2".parse().unwrap(),
        last_seen: 2000,
        status: PeerStatus::Active,
        iroh_node_id: None,
    };
    peers.push(record2);

    if let Some(existing) = peers.iter_mut().find(|p| p.wg_public_key == "pubkey-1") {
        existing.status = PeerStatus::Removed;
    }

    let active: Vec<_> = peers.iter().filter(|p| p.status == PeerStatus::Active).collect();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].name, "node-2");
}

/// NodeState with peers roundtrip.
#[test]
fn node_state_with_peers_roundtrip() {
    let state = NodeState {
        mesh_name: "prod".into(),
        mesh_secret: "syf_sk_test".into(),
        wg_private_key: "priv".into(),
        wg_public_key: "pub".into(),
        mesh_ipv6: "fd12::1".parse().unwrap(),
        mesh_prefix: "fd12::".parse().unwrap(),
        wg_listen_port: 51820,
        node_name: "n1".into(),
        public_endpoint: None,
        ipfs_api: None,
        peers: vec![
            PeerRecord {
                name: "peer-a".into(),
                wg_public_key: "pk-a".into(),
                endpoint: "1.1.1.1:51820".parse().unwrap(),
                mesh_ipv6: "fd12::a".parse().unwrap(),
                last_seen: 100,
                status: PeerStatus::Active,
                iroh_node_id: None,
            },
        ],
        metrics: Default::default(),
    };

    let json = serde_json::to_string(&state).unwrap();
    let loaded: NodeState = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded.peers.len(), 1);
    assert_eq!(loaded.peers[0].name, "peer-a");
}

/// IPFS key is deterministic from secret.
#[test]
fn ipfs_key_deterministic() {
    let secret = MeshSecret::generate();
    assert_eq!(secret.ipfs_key_hex(), secret.ipfs_key_hex());

    let other = MeshSecret::generate();
    assert_ne!(secret.ipfs_key_hex(), other.ipfs_key_hex());
}

fn derive_prefix(secret: &MeshSecret) -> Ipv6Addr {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest([b"mesh-prefix:" as &[u8], secret.as_bytes()].concat());
    Ipv6Addr::new(
        0xfd00 | (hash[0] as u16),
        ((hash[1] as u16) << 8) | (hash[2] as u16),
        ((hash[3] as u16) << 8) | (hash[4] as u16),
        0, 0, 0, 0, 0,
    )
}
