use std::net::{Ipv6Addr, SocketAddr};

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Nonce};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MeshError {
    #[error("encryption failed")]
    EncryptionFailed,
    #[error("decryption failed: invalid secret or corrupted data")]
    DecryptionFailed,
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("encrypted payload too short")]
    PayloadTooShort,
    #[error("validation error: {0}")]
    Validation(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerStatus {
    Active,
    Unreachable,
    Removed,
}

/// Record exchanged between mesh peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerRecord {
    pub name: String,
    pub wg_public_key: String,
    pub endpoint: SocketAddr,
    pub mesh_ipv6: Ipv6Addr,
    pub last_seen: u64,
    pub status: PeerStatus,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub zone: Option<String>,
}

// --- Peering protocol types ---

/// A request from a new node wanting to join the mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRequest {
    pub request_id: String,
    pub node_name: String,
    pub wg_public_key: String,
    pub endpoint: SocketAddr,
    pub wg_listen_port: u16,
    /// Optional PIN for auto-accept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
    /// Joiner's region (sent so the leader can store it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Joiner's zone (sent so the leader can store it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
}

/// Response sent back to a new node after acceptance or rejection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinResponse {
    pub accepted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_prefix: Option<Ipv6Addr>,
    #[serde(default)]
    pub peers: Vec<PeerRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// How the join was approved: "pin" or "manual". None if rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_by: Option<String>,
}

/// Wire protocol message envelope for TCP peering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PeeringMessage {
    JoinRequest(JoinRequest),
    JoinResponse(JoinResponse),
    /// Encrypted PeerRecord announcement (ciphertext, uses mesh secret).
    PeerAnnounce(Vec<u8>),
}

// --- Validation constants ---

/// Maximum length for name, region, and zone fields.
pub const MAX_NAME_LENGTH: usize = 255;

/// Maximum length for request_id and pin fields.
pub const MAX_SHORT_FIELD_LENGTH: usize = 32;

/// Maximum number of peers in a JoinResponse.
pub const MAX_PEERS_IN_RESPONSE: usize = 500;

/// Expected base64-encoded WireGuard public key length (32 bytes -> 44 chars base64).
pub const WG_KEY_BASE64_LENGTH: usize = 44;

// --- Validation functions ---

/// Validate a name field (node name, region, zone): 1-255 chars, alphanumeric + `-_.` only.
pub fn validate_name(field_name: &str, value: &str) -> Result<(), MeshError> {
    if value.is_empty() {
        return Err(MeshError::Validation(format!(
            "{field_name} must not be empty"
        )));
    }
    if value.len() > MAX_NAME_LENGTH {
        return Err(MeshError::Validation(format!(
            "{field_name} must be at most {MAX_NAME_LENGTH} chars, got {}",
            value.len()
        )));
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    {
        return Err(MeshError::Validation(format!(
            "{field_name} contains invalid characters (allowed: alphanumeric, dash, underscore, dot)"
        )));
    }
    Ok(())
}

/// Validate a short field (request_id, pin): 1-32 chars, alphanumeric + `-_.` only.
pub fn validate_short_field(field_name: &str, value: &str) -> Result<(), MeshError> {
    if value.is_empty() {
        return Err(MeshError::Validation(format!(
            "{field_name} must not be empty"
        )));
    }
    if value.len() > MAX_SHORT_FIELD_LENGTH {
        return Err(MeshError::Validation(format!(
            "{field_name} must be at most {MAX_SHORT_FIELD_LENGTH} chars, got {}",
            value.len()
        )));
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    {
        return Err(MeshError::Validation(format!(
            "{field_name} contains invalid characters (allowed: alphanumeric, dash, underscore, dot)"
        )));
    }
    Ok(())
}

/// Validate a WireGuard public key: must be 44 chars of valid base64, decoding to 32 bytes.
pub fn validate_wg_public_key(key: &str) -> Result<(), MeshError> {
    use base64::Engine;
    if key.len() != WG_KEY_BASE64_LENGTH {
        return Err(MeshError::Validation(format!(
            "WireGuard public key must be {WG_KEY_BASE64_LENGTH} chars base64, got {}",
            key.len()
        )));
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(key)
        .map_err(|_| {
            MeshError::Validation("WireGuard public key is not valid base64".to_string())
        })?;
    if decoded.len() != 32 {
        return Err(MeshError::Validation(format!(
            "WireGuard public key must decode to 32 bytes, got {}",
            decoded.len()
        )));
    }
    Ok(())
}

/// Validate a socket address endpoint: no loopback, no unspecified, port > 0.
pub fn validate_endpoint(addr: &SocketAddr) -> Result<(), MeshError> {
    if addr.ip().is_loopback() {
        return Err(MeshError::Validation(
            "endpoint cannot be loopback".to_string(),
        ));
    }
    if addr.ip().is_unspecified() {
        return Err(MeshError::Validation(
            "endpoint cannot be unspecified (0.0.0.0 / ::)".to_string(),
        ));
    }
    if addr.port() == 0 {
        return Err(MeshError::Validation(
            "endpoint port cannot be 0".to_string(),
        ));
    }
    Ok(())
}

/// Validate that a mesh IPv6 address belongs to the given /48 prefix.
pub fn validate_mesh_ipv6(addr: &Ipv6Addr, prefix: &Ipv6Addr) -> Result<(), MeshError> {
    let a = addr.segments();
    let p = prefix.segments();
    if a[0] != p[0] || a[1] != p[1] || a[2] != p[2] {
        return Err(MeshError::Validation(format!(
            "mesh IPv6 {addr} does not match mesh /48 prefix {prefix}"
        )));
    }
    Ok(())
}

/// Validate all fields of a PeerRecord.
pub fn validate_peer_record(record: &PeerRecord) -> Result<(), MeshError> {
    validate_name("name", &record.name)?;
    validate_wg_public_key(&record.wg_public_key)?;
    validate_endpoint(&record.endpoint)?;
    if let Some(ref region) = record.region {
        validate_name("region", region)?;
    }
    if let Some(ref zone) = record.zone {
        validate_name("zone", zone)?;
    }
    Ok(())
}

/// Validate all fields of a JoinRequest.
pub fn validate_join_request(req: &JoinRequest) -> Result<(), MeshError> {
    validate_short_field("request_id", &req.request_id)?;
    validate_name("node_name", &req.node_name)?;
    validate_wg_public_key(&req.wg_public_key)?;
    // Endpoint validation is relaxed for join requests: 0.0.0.0 is allowed
    // because the leader auto-detects the endpoint from the TCP peer address.
    if req.endpoint.port() == 0 {
        return Err(MeshError::Validation(
            "endpoint port cannot be 0".to_string(),
        ));
    }
    if let Some(ref pin) = req.pin {
        validate_short_field("pin", pin)?;
    }
    if let Some(ref region) = req.region {
        validate_name("region", region)?;
    }
    if let Some(ref zone) = req.zone {
        validate_name("zone", zone)?;
    }
    Ok(())
}

/// Validate a JoinResponse (peer list size limit).
pub fn validate_join_response(resp: &JoinResponse) -> Result<(), MeshError> {
    if resp.peers.len() > MAX_PEERS_IN_RESPONSE {
        return Err(MeshError::Validation(format!(
            "JoinResponse contains {} peers, maximum is {MAX_PEERS_IN_RESPONSE}",
            resp.peers.len()
        )));
    }
    Ok(())
}

/// Encrypt a PeerRecord with AES-256-GCM using the mesh encryption key.
/// Returns nonce (12 bytes) || ciphertext.
pub fn encrypt_record(
    record: &PeerRecord,
    encryption_key: &[u8; 32],
) -> Result<Vec<u8>, MeshError> {
    let plaintext = serde_json::to_vec(record)?;
    let cipher =
        Aes256Gcm::new_from_slice(encryption_key).map_err(|_| MeshError::EncryptionFailed)?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_ref())
        .map_err(|_| MeshError::EncryptionFailed)?;

    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend(ciphertext);
    Ok(out)
}

/// Decrypt a PeerRecord from nonce || ciphertext using the mesh encryption key.
pub fn decrypt_record(data: &[u8], encryption_key: &[u8; 32]) -> Result<PeerRecord, MeshError> {
    if data.len() < 12 {
        return Err(MeshError::PayloadTooShort);
    }
    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher =
        Aes256Gcm::new_from_slice(encryption_key).map_err(|_| MeshError::DecryptionFailed)?;
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| MeshError::DecryptionFailed)?;
    let record: PeerRecord = serde_json::from_slice(&plaintext)?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::MeshSecret;
    use std::net::{Ipv6Addr, SocketAddr};

    fn sample_record() -> PeerRecord {
        PeerRecord {
            name: "node-1".into(),
            wg_public_key: "dGVzdC1wdWJsaWMta2V5".into(),
            endpoint: "203.0.113.1:51820".parse::<SocketAddr>().unwrap(),
            mesh_ipv6: Ipv6Addr::new(0xfd12, 0x3456, 0x7800, 0, 0, 0, 0, 1),
            last_seen: 1700000000,
            status: PeerStatus::Active,
            region: None,
            zone: None,
        }
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let secret = MeshSecret::generate();
        let key = secret.encryption_key();
        let record = sample_record();

        let encrypted = encrypt_record(&record, &key).unwrap();
        let decrypted = decrypt_record(&encrypted, &key).unwrap();

        assert_eq!(decrypted.name, "node-1");
        assert_eq!(decrypted.wg_public_key, record.wg_public_key);
        assert_eq!(decrypted.endpoint, record.endpoint);
        assert_eq!(decrypted.mesh_ipv6, record.mesh_ipv6);
    }

    #[test]
    fn wrong_key_fails_decryption() {
        let s1 = MeshSecret::generate();
        let s2 = MeshSecret::generate();
        let record = sample_record();

        let encrypted = encrypt_record(&record, &s1.encryption_key()).unwrap();
        let result = decrypt_record(&encrypted, &s2.encryption_key());
        assert!(result.is_err());
    }

    #[test]
    fn corrupted_data_fails() {
        let secret = MeshSecret::generate();
        let key = secret.encryption_key();
        let record = sample_record();

        let mut encrypted = encrypt_record(&record, &key).unwrap();
        // Flip a byte in the ciphertext
        if let Some(byte) = encrypted.last_mut() {
            *byte ^= 0xff;
        }
        let result = decrypt_record(&encrypted, &key);
        assert!(result.is_err());
    }

    #[test]
    fn too_short_payload_fails() {
        let key = [0u8; 32];
        let result = decrypt_record(&[0u8; 5], &key);
        assert!(matches!(result, Err(MeshError::PayloadTooShort)));
    }

    #[test]
    fn peer_status_serde() {
        let json = serde_json::to_string(&PeerStatus::Unreachable).unwrap();
        let parsed: PeerStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, PeerStatus::Unreachable);
    }

    // --- Validation tests ---

    /// A valid 32-byte WireGuard key encoded as base64 (44 chars).
    const VALID_WG_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

    fn valid_record() -> PeerRecord {
        PeerRecord {
            name: "node-1".into(),
            wg_public_key: VALID_WG_KEY.into(),
            endpoint: "203.0.113.1:51820".parse::<SocketAddr>().unwrap(),
            mesh_ipv6: Ipv6Addr::new(0xfd12, 0x3456, 0x7800, 0, 0, 0, 0, 1),
            last_seen: 1700000000,
            status: PeerStatus::Active,
            region: Some("us-east-1".into()),
            zone: Some("zone-a".into()),
        }
    }

    #[test]
    fn validate_name_valid() {
        assert!(validate_name("name", "node-1").is_ok());
        assert!(validate_name("name", "my_node.test").is_ok());
        assert!(validate_name("name", "a").is_ok());
        assert!(validate_name("name", &"a".repeat(255)).is_ok());
    }

    #[test]
    fn validate_name_empty() {
        let err = validate_name("name", "").unwrap_err();
        assert!(matches!(err, MeshError::Validation(_)));
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn validate_name_too_long() {
        let err = validate_name("name", &"a".repeat(256)).unwrap_err();
        assert!(err.to_string().contains("at most 255"));
    }

    #[test]
    fn validate_name_invalid_chars() {
        let err = validate_name("name", "node name").unwrap_err();
        assert!(err.to_string().contains("invalid characters"));
        let err = validate_name("name", "node@host").unwrap_err();
        assert!(err.to_string().contains("invalid characters"));
        let err = validate_name("name", "node/path").unwrap_err();
        assert!(err.to_string().contains("invalid characters"));
    }

    #[test]
    fn validate_short_field_valid() {
        assert!(validate_short_field("request_id", "abc123").is_ok());
        assert!(validate_short_field("pin", "1234").is_ok());
        assert!(validate_short_field("id", &"a".repeat(32)).is_ok());
    }

    #[test]
    fn validate_short_field_too_long() {
        let err = validate_short_field("request_id", &"a".repeat(33)).unwrap_err();
        assert!(err.to_string().contains("at most 32"));
    }

    #[test]
    fn validate_wg_key_valid() {
        assert!(validate_wg_public_key(VALID_WG_KEY).is_ok());
    }

    #[test]
    fn validate_wg_key_wrong_length() {
        let err = validate_wg_public_key("short").unwrap_err();
        assert!(err.to_string().contains("44 chars"));
    }

    #[test]
    fn validate_wg_key_invalid_base64() {
        // 44 chars but not valid base64
        let err =
            validate_wg_public_key("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!").unwrap_err();
        assert!(err.to_string().contains("not valid base64"));
    }

    #[test]
    fn validate_endpoint_valid() {
        let addr: SocketAddr = "203.0.113.1:51820".parse().unwrap();
        assert!(validate_endpoint(&addr).is_ok());
    }

    #[test]
    fn validate_endpoint_loopback() {
        let addr: SocketAddr = "127.0.0.1:51820".parse().unwrap();
        let err = validate_endpoint(&addr).unwrap_err();
        assert!(err.to_string().contains("loopback"));
    }

    #[test]
    fn validate_endpoint_unspecified() {
        let addr: SocketAddr = "0.0.0.0:51820".parse().unwrap();
        let err = validate_endpoint(&addr).unwrap_err();
        assert!(err.to_string().contains("unspecified"));
    }

    #[test]
    fn validate_endpoint_port_zero() {
        let addr: SocketAddr = "203.0.113.1:0".parse().unwrap();
        let err = validate_endpoint(&addr).unwrap_err();
        assert!(err.to_string().contains("port cannot be 0"));
    }

    #[test]
    fn validate_mesh_ipv6_valid() {
        let addr = Ipv6Addr::new(0xfd12, 0x3456, 0x7800, 0, 0, 0, 0, 1);
        let prefix = Ipv6Addr::new(0xfd12, 0x3456, 0x7800, 0, 0, 0, 0, 0);
        assert!(validate_mesh_ipv6(&addr, &prefix).is_ok());
    }

    #[test]
    fn validate_mesh_ipv6_wrong_prefix() {
        let addr = Ipv6Addr::new(0xfd99, 0x0000, 0x0000, 0, 0, 0, 0, 1);
        let prefix = Ipv6Addr::new(0xfd12, 0x3456, 0x7800, 0, 0, 0, 0, 0);
        let err = validate_mesh_ipv6(&addr, &prefix).unwrap_err();
        assert!(err.to_string().contains("does not match"));
    }

    #[test]
    fn validate_peer_record_valid() {
        assert!(validate_peer_record(&valid_record()).is_ok());
    }

    #[test]
    fn validate_peer_record_bad_name() {
        let mut record = valid_record();
        record.name = "".into();
        assert!(validate_peer_record(&record).is_err());
    }

    #[test]
    fn validate_peer_record_bad_key() {
        let mut record = valid_record();
        record.wg_public_key = "not-a-key".into();
        assert!(validate_peer_record(&record).is_err());
    }

    #[test]
    fn validate_peer_record_bad_endpoint() {
        let mut record = valid_record();
        record.endpoint = "127.0.0.1:51820".parse().unwrap();
        assert!(validate_peer_record(&record).is_err());
    }

    #[test]
    fn validate_peer_record_bad_region() {
        let mut record = valid_record();
        record.region = Some("region with spaces".into());
        assert!(validate_peer_record(&record).is_err());
    }

    #[test]
    fn validate_join_request_valid() {
        let req = JoinRequest {
            request_id: "abc12345".into(),
            node_name: "node-2".into(),
            wg_public_key: VALID_WG_KEY.into(),
            endpoint: "203.0.113.2:51820".parse().unwrap(),
            wg_listen_port: 51820,
            pin: Some("1234".into()),
            region: Some("us-east-1".into()),
            zone: Some("zone-a".into()),
        };
        assert!(validate_join_request(&req).is_ok());
    }

    #[test]
    fn validate_join_request_oversized_name() {
        let req = JoinRequest {
            request_id: "abc12345".into(),
            node_name: "a".repeat(256),
            wg_public_key: VALID_WG_KEY.into(),
            endpoint: "203.0.113.2:51820".parse().unwrap(),
            wg_listen_port: 51820,
            pin: None,
            region: None,
            zone: None,
        };
        assert!(validate_join_request(&req).is_err());
    }

    #[test]
    fn validate_join_request_oversized_request_id() {
        let req = JoinRequest {
            request_id: "a".repeat(33),
            node_name: "node-2".into(),
            wg_public_key: VALID_WG_KEY.into(),
            endpoint: "203.0.113.2:51820".parse().unwrap(),
            wg_listen_port: 51820,
            pin: None,
            region: None,
            zone: None,
        };
        assert!(validate_join_request(&req).is_err());
    }

    #[test]
    fn validate_join_request_oversized_pin() {
        let req = JoinRequest {
            request_id: "abc12345".into(),
            node_name: "node-2".into(),
            wg_public_key: VALID_WG_KEY.into(),
            endpoint: "203.0.113.2:51820".parse().unwrap(),
            wg_listen_port: 51820,
            pin: Some("a".repeat(33)),
            region: None,
            zone: None,
        };
        assert!(validate_join_request(&req).is_err());
    }

    #[test]
    fn validate_join_request_allows_unspecified_endpoint() {
        // Join requests allow 0.0.0.0 because the leader auto-detects the endpoint
        let req = JoinRequest {
            request_id: "abc12345".into(),
            node_name: "node-2".into(),
            wg_public_key: VALID_WG_KEY.into(),
            endpoint: "0.0.0.0:51820".parse().unwrap(),
            wg_listen_port: 51820,
            pin: None,
            region: None,
            zone: None,
        };
        assert!(validate_join_request(&req).is_ok());
    }

    #[test]
    fn validate_join_request_rejects_port_zero() {
        let req = JoinRequest {
            request_id: "abc12345".into(),
            node_name: "node-2".into(),
            wg_public_key: VALID_WG_KEY.into(),
            endpoint: "203.0.113.2:0".parse().unwrap(),
            wg_listen_port: 51820,
            pin: None,
            region: None,
            zone: None,
        };
        assert!(validate_join_request(&req).is_err());
    }

    #[test]
    fn validate_join_response_within_limit() {
        let resp = JoinResponse {
            accepted: true,
            mesh_name: Some("test-mesh".into()),
            mesh_secret: None,
            mesh_prefix: None,
            peers: vec![valid_record(); 100],
            reason: None,
            approved_by: None,
        };
        assert!(validate_join_response(&resp).is_ok());
    }

    #[test]
    fn validate_join_response_exceeds_limit() {
        let resp = JoinResponse {
            accepted: true,
            mesh_name: Some("test-mesh".into()),
            mesh_secret: None,
            mesh_prefix: None,
            peers: vec![valid_record(); MAX_PEERS_IN_RESPONSE + 1],
            reason: None,
            approved_by: None,
        };
        assert!(validate_join_response(&resp).is_err());
    }
}
