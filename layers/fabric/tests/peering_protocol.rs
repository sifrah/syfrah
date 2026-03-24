//! E2E test for the TCP peering protocol.
//!
//! Tests the full join flow: a new node sends a JoinRequest,
//! the existing node auto-accepts via PIN, and the new node
//! receives the mesh secret + peer list.
//!
//! This test does NOT require root or WireGuard — it only tests
//! the TCP protocol layer.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use syfrah_core::addressing;
use syfrah_core::mesh::{JoinRequest, PeerRecord, PeerStatus};
use syfrah_core::secret::MeshSecret;
use syfrah_fabric::peering::{
    generate_pin, generate_request_id, send_join_request, AutoAcceptConfig, OnAccepted,
    PeeringState,
};

/// Find a free TCP port by binding to port 0
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[tokio::test]
async fn join_with_pin_auto_accept() {
    // ── Setup: create a mesh "leader" node ──

    let mesh_secret = MeshSecret::generate();
    let mesh_prefix = syfrah_fabric::daemon::derive_prefix_from_secret(&mesh_secret);
    let encryption_key = mesh_secret.encryption_key();
    let pin = generate_pin();
    let peering_port = free_port();

    // Leader's WireGuard keypair (we just need the public key for the protocol)
    let leader_keypair = syfrah_fabric::wg::generate_keypair();
    let leader_ipv6 =
        addressing::derive_node_address(&mesh_prefix, leader_keypair.public.as_bytes());
    let leader_endpoint: SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();

    let leader_record = PeerRecord {
        name: "leader".to_string(),
        wg_public_key: leader_keypair.public.to_base64(),
        endpoint: leader_endpoint,
        mesh_ipv6: leader_ipv6,
        last_seen: 0,
        status: PeerStatus::Active,
        region: None,
        zone: None,
    };

    // ── Start peering listener with PIN auto-accept ──

    let peering_state = Arc::new(PeeringState::new());

    peering_state
        .set_auto_accept(Some(AutoAcceptConfig {
            pin: pin.clone(),
            mesh_name: "test-mesh".to_string(),
            mesh_secret_str: mesh_secret.to_string(),
            mesh_prefix,
            my_record: leader_record.clone(),
            wg_pubkey: leader_keypair.public.clone(),
            encryption_key,
            peering_port,
            max_peers: 1000,
        }))
        .await;

    // Track accepted peers
    let accepted_peers: Arc<tokio::sync::Mutex<Vec<PeerRecord>>> =
        Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let accepted_clone = accepted_peers.clone();

    let on_accepted: OnAccepted = Arc::new(move |record| {
        let peers = accepted_clone.clone();
        tokio::spawn(async move {
            peers.lock().await.push(record);
        });
    });

    let on_announce: Arc<dyn Fn(PeerRecord) + Send + Sync> = Arc::new(|_| {});

    // Start the listener in background
    let listener_state = peering_state.clone();
    let listener_handle = tokio::spawn(async move {
        listener_state
            .run_listener(peering_port, Some(encryption_key), on_announce, on_accepted)
            .await
            .ok();
    });

    // Give the listener time to bind
    tokio::time::sleep(Duration::from_millis(100)).await;

    // ── Joiner: send a join request with the correct PIN ──

    let joiner_keypair = syfrah_fabric::wg::generate_keypair();
    let joiner_endpoint: SocketAddr = "127.0.0.1:0".parse().unwrap();

    let join_request = JoinRequest {
        request_id: generate_request_id(),
        node_name: "joiner".to_string(),
        wg_public_key: joiner_keypair.public.to_base64(),
        endpoint: joiner_endpoint,
        wg_listen_port: 51820,
        pin: Some(pin.clone()),
        region: Some("region-1".to_string()),
        zone: Some("region-1-zone-1".to_string()),
    };

    let target: SocketAddr = format!("127.0.0.1:{peering_port}").parse().unwrap();
    let response = send_join_request(target, join_request).await.unwrap();

    // ── Verify: join was accepted ──

    assert!(response.accepted, "join request should be accepted");
    assert_eq!(response.mesh_name.as_deref(), Some("test-mesh"));
    assert_eq!(
        response.mesh_secret.as_deref(),
        Some(mesh_secret.to_string().as_str())
    );
    assert_eq!(response.mesh_prefix, Some(mesh_prefix));

    // The response should contain the leader as a peer
    assert!(
        !response.peers.is_empty(),
        "response should contain at least the leader peer"
    );
    assert!(
        response.peers.iter().any(|p| p.name == "leader"),
        "leader should be in the peer list"
    );

    // The leader should have recorded the joiner
    tokio::time::sleep(Duration::from_millis(100)).await;
    let accepted = accepted_peers.lock().await;
    assert_eq!(accepted.len(), 1, "one peer should have been accepted");
    assert_eq!(accepted[0].name, "joiner");
    assert_eq!(
        accepted[0].region.as_deref(),
        Some("region-1"),
        "accepted peer should have the joiner's region"
    );
    assert_eq!(
        accepted[0].zone.as_deref(),
        Some("region-1-zone-1"),
        "accepted peer should have the joiner's zone"
    );

    // Cleanup
    listener_handle.abort();
}

#[tokio::test]
async fn join_with_wrong_pin_falls_to_pending() {
    // ── Setup ──

    let mesh_secret = MeshSecret::generate();
    let encryption_key = mesh_secret.encryption_key();
    let correct_pin = "1234".to_string();
    let wrong_pin = "9999".to_string();
    let peering_port = free_port();
    let mesh_prefix = syfrah_fabric::daemon::derive_prefix_from_secret(&mesh_secret);

    let leader_keypair = syfrah_fabric::wg::generate_keypair();
    let leader_ipv6 =
        addressing::derive_node_address(&mesh_prefix, leader_keypair.public.as_bytes());

    let leader_record = PeerRecord {
        name: "leader".to_string(),
        wg_public_key: leader_keypair.public.to_base64(),
        endpoint: format!("127.0.0.1:{}", free_port()).parse().unwrap(),
        mesh_ipv6: leader_ipv6,
        last_seen: 0,
        status: PeerStatus::Active,
        region: None,
        zone: None,
    };

    let peering_state = Arc::new(PeeringState::new());

    peering_state
        .set_auto_accept(Some(AutoAcceptConfig {
            pin: correct_pin,
            mesh_name: "test-mesh".to_string(),
            mesh_secret_str: mesh_secret.to_string(),
            mesh_prefix,
            my_record: leader_record,
            wg_pubkey: leader_keypair.public.clone(),
            encryption_key,
            peering_port,
            max_peers: 1000,
        }))
        .await;

    let on_accepted: OnAccepted = Arc::new(|_| {});
    let on_announce: Arc<dyn Fn(PeerRecord) + Send + Sync> = Arc::new(|_| {});

    let listener_state = peering_state.clone();
    let listener_handle = tokio::spawn(async move {
        listener_state
            .run_listener(peering_port, Some(encryption_key), on_announce, on_accepted)
            .await
            .ok();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    // ── Send join request with WRONG pin ──

    let joiner_keypair = syfrah_fabric::wg::generate_keypair();

    let join_request = JoinRequest {
        request_id: generate_request_id(),
        node_name: "joiner".to_string(),
        wg_public_key: joiner_keypair.public.to_base64(),
        endpoint: "127.0.0.1:0".parse().unwrap(),
        wg_listen_port: 51820,
        pin: Some(wrong_pin),
        region: Some("region-1".to_string()),
        zone: Some("region-1-zone-1".to_string()),
    };

    let target: SocketAddr = format!("127.0.0.1:{peering_port}").parse().unwrap();

    // The request should be pending (not auto-accepted), which means
    // it will timeout since nobody approves it manually.
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        send_join_request(target, join_request),
    )
    .await;

    // Should timeout (request is pending, not auto-accepted)
    assert!(result.is_err(), "wrong PIN should not be auto-accepted");

    // The request should be in the pending list
    let pending = peering_state.list_pending().await;
    assert_eq!(pending.len(), 1, "request should be pending");
    assert_eq!(pending[0].node_name, "joiner");

    // Cleanup
    listener_handle.abort();
}

#[tokio::test]
async fn join_without_pin_goes_to_pending() {
    // ── Setup ──

    let mesh_secret = MeshSecret::generate();
    let encryption_key = mesh_secret.encryption_key();
    let peering_port = free_port();
    let mesh_prefix = syfrah_fabric::daemon::derive_prefix_from_secret(&mesh_secret);

    let leader_keypair = syfrah_fabric::wg::generate_keypair();
    let leader_ipv6 =
        addressing::derive_node_address(&mesh_prefix, leader_keypair.public.as_bytes());

    let leader_record = PeerRecord {
        name: "leader".to_string(),
        wg_public_key: leader_keypair.public.to_base64(),
        endpoint: format!("127.0.0.1:{}", free_port()).parse().unwrap(),
        mesh_ipv6: leader_ipv6,
        last_seen: 0,
        status: PeerStatus::Active,
        region: None,
        zone: None,
    };

    let peering_state = Arc::new(PeeringState::new());

    // Auto-accept IS configured, but the joiner sends NO pin
    peering_state
        .set_auto_accept(Some(AutoAcceptConfig {
            pin: "1234".to_string(),
            mesh_name: "test-mesh".to_string(),
            mesh_secret_str: mesh_secret.to_string(),
            mesh_prefix,
            my_record: leader_record,
            wg_pubkey: leader_keypair.public.clone(),
            encryption_key,
            peering_port,
            max_peers: 1000,
        }))
        .await;

    let on_accepted: OnAccepted = Arc::new(|_| {});
    let on_announce: Arc<dyn Fn(PeerRecord) + Send + Sync> = Arc::new(|_| {});

    let listener_state = peering_state.clone();
    let listener_handle = tokio::spawn(async move {
        listener_state
            .run_listener(peering_port, Some(encryption_key), on_announce, on_accepted)
            .await
            .ok();
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    // ── Send join request WITHOUT pin ──

    let joiner_keypair = syfrah_fabric::wg::generate_keypair();

    let join_request = JoinRequest {
        request_id: generate_request_id(),
        node_name: "no-pin-joiner".to_string(),
        wg_public_key: joiner_keypair.public.to_base64(),
        endpoint: "127.0.0.1:0".parse().unwrap(),
        wg_listen_port: 51820,
        pin: None, // No PIN
        region: Some("region-1".to_string()),
        zone: Some("region-1-zone-1".to_string()),
    };

    let target: SocketAddr = format!("127.0.0.1:{peering_port}").parse().unwrap();

    // Should timeout (goes to pending, no auto-accept without PIN)
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        send_join_request(target, join_request),
    )
    .await;

    assert!(
        result.is_err(),
        "no-pin request should not be auto-accepted"
    );

    // Verify it's pending
    let pending = peering_state.list_pending().await;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].node_name, "no-pin-joiner");

    // Cleanup
    listener_handle.abort();
}
