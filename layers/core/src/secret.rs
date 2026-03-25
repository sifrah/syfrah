use std::str::FromStr;

use hkdf::Hkdf;
use sha2::Sha256;
use thiserror::Error;

const SECRET_PREFIX: &str = "syf_sk_";
const SECRET_BYTES: usize = 32;

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("invalid secret format: must start with 'syf_sk_'")]
    InvalidPrefix,
    #[error("invalid secret encoding: {0}")]
    InvalidEncoding(#[from] bs58::decode::Error),
    #[error("invalid secret length: expected {SECRET_BYTES} bytes, got {0}")]
    InvalidLength(usize),
}

/// The shared secret for a mesh. This is the ONLY credential needed to join.
/// Derives all discovery and encryption keys.
///
/// Format: `syf_sk_{base58(32 bytes)}`
#[derive(Clone)]
pub struct MeshSecret {
    bytes: [u8; SECRET_BYTES],
}

impl MeshSecret {
    pub fn from_bytes(bytes: [u8; SECRET_BYTES]) -> Self {
        Self { bytes }
    }

    /// RNG policy: all cryptographic material MUST use OsRng to draw
    /// directly from the operating-system entropy source.
    pub fn generate() -> Self {
        let mut bytes = [0u8; SECRET_BYTES];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut bytes);
        Self { bytes }
    }

    /// Deterministic mesh identifier (first 16 bytes of HKDF-derived key).
    pub fn mesh_id(&self) -> [u8; 16] {
        let derived = Self::derive("mesh-id", &self.bytes);
        let mut id = [0u8; 16];
        id.copy_from_slice(&derived[..16]);
        id
    }

    /// Mesh ID as a short hex string for display.
    pub fn mesh_id_short(&self) -> String {
        let id = self.mesh_id();
        format!("{:02x}{:02x}{:02x}{:02x}", id[0], id[1], id[2], id[3])
    }

    /// AES-256-GCM encryption key for IPFS records.
    pub fn encryption_key(&self) -> [u8; 32] {
        Self::derive("encryption-key", &self.bytes)
    }

    /// IPFS discovery key — used to derive the CID/path where peer records are published.
    pub fn ipfs_key(&self) -> [u8; 32] {
        Self::derive("ipfs-key", &self.bytes)
    }

    /// IPFS key as hex string (used as filename/path on IPFS).
    pub fn ipfs_key_hex(&self) -> String {
        let key = self.ipfs_key();
        key.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn as_bytes(&self) -> &[u8; SECRET_BYTES] {
        &self.bytes
    }

    /// Derive a sub-key using HKDF-SHA256 (RFC 5869).
    /// Salt is versioned to allow future rotation.
    fn derive(info: &str, secret: &[u8]) -> [u8; 32] {
        let hkdf = Hkdf::<Sha256>::new(Some(b"syfrah-fabric-v1"), secret);
        let mut output = [0u8; 32];
        hkdf.expand(info.as_bytes(), &mut output)
            .expect("32 bytes is valid for HKDF-SHA256");
        output
    }
}

impl FromStr for MeshSecret {
    type Err = SecretError;

    fn from_str(s: &str) -> Result<Self, SecretError> {
        let encoded = s
            .strip_prefix(SECRET_PREFIX)
            .ok_or(SecretError::InvalidPrefix)?;
        let decoded = bs58::decode(encoded).into_vec()?;
        if decoded.len() != SECRET_BYTES {
            return Err(SecretError::InvalidLength(decoded.len()));
        }
        let mut bytes = [0u8; SECRET_BYTES];
        bytes.copy_from_slice(&decoded);
        Ok(Self { bytes })
    }
}

impl std::fmt::Display for MeshSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}",
            SECRET_PREFIX,
            bs58::encode(&self.bytes).into_string()
        )
    }
}

impl std::fmt::Debug for MeshSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MeshSecret({}...)", &self.mesh_id_short())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_and_roundtrip() {
        let secret = MeshSecret::generate();
        let encoded = secret.to_string();
        assert!(encoded.starts_with(SECRET_PREFIX));

        let parsed: MeshSecret = encoded.parse().unwrap();
        assert_eq!(parsed.as_bytes(), secret.as_bytes());
    }

    #[test]
    fn derivations_distinct() {
        let secret = MeshSecret::generate();
        assert_ne!(secret.encryption_key(), secret.ipfs_key());
    }

    #[test]
    fn ipfs_key_deterministic() {
        let secret = MeshSecret::generate();
        assert_eq!(secret.ipfs_key_hex(), secret.ipfs_key_hex());
    }

    #[test]
    fn different_secrets_different_ids() {
        let s1 = MeshSecret::generate();
        let s2 = MeshSecret::generate();
        assert_ne!(s1.mesh_id(), s2.mesh_id());
        assert_ne!(s1.ipfs_key(), s2.ipfs_key());
    }

    #[test]
    fn invalid_prefix() {
        let err = "bad_prefix_abc".parse::<MeshSecret>().unwrap_err();
        assert!(matches!(err, SecretError::InvalidPrefix));
    }

    #[test]
    fn invalid_encoding() {
        let err = "syf_sk_!!!invalid!!!".parse::<MeshSecret>().unwrap_err();
        assert!(matches!(err, SecretError::InvalidEncoding(_)));
    }

    #[test]
    fn wrong_length() {
        let long = format!("syf_sk_{}", bs58::encode(&[0u8; 64]).into_string());
        let err = long.parse::<MeshSecret>().unwrap_err();
        assert!(matches!(err, SecretError::InvalidLength(64)));
    }

    #[test]
    fn hkdf_domain_separation() {
        let secret = MeshSecret::from_bytes([0xAB; 32]);
        let enc = secret.encryption_key();
        let ipfs = secret.ipfs_key();
        let mid = secret.mesh_id();
        // All derived values must be distinct
        assert_ne!(enc, ipfs);
        assert_ne!(&enc[..16], &mid[..]);
        assert_ne!(&ipfs[..16], &mid[..]);
    }

    #[test]
    fn hkdf_deterministic_output() {
        let secret = MeshSecret::from_bytes([0x42; 32]);
        let key1 = secret.encryption_key();
        let key2 = secret.encryption_key();
        assert_eq!(key1, key2);
    }
}
