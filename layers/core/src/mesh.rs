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
}

/// Wire protocol message envelope for TCP peering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PeeringMessage {
    JoinRequest(JoinRequest),
    JoinResponse(JoinResponse),
    /// Encrypted PeerRecord announcement (ciphertext, uses mesh secret).
    PeerAnnounce(Vec<u8>),
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
}
