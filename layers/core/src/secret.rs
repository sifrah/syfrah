use std::str::FromStr;

use hkdf::Hkdf;
use sha2::{Digest, Sha256};
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

/// Derivation version for backward compatibility.
///
/// - `V1`: Legacy SHA-256 derivation (`SHA256(domain || secret)`).
/// - `V2`: HKDF-SHA256 (RFC 5869) with versioned salt.
///
/// Existing secrets default to V1 so old nodes can still communicate.
/// Newly generated secrets use V2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DerivationVersion {
    V1,
    V2,
}

/// The shared secret for a mesh. This is the ONLY credential needed to join.
/// Derives all discovery and encryption keys.
///
/// Format: `syf_sk_{base58(32 bytes)}`
#[derive(Clone)]
pub struct MeshSecret {
    bytes: [u8; SECRET_BYTES],
    version: DerivationVersion,
}

impl MeshSecret {
    pub fn from_bytes(bytes: [u8; SECRET_BYTES]) -> Self {
        Self {
            bytes,
            version: DerivationVersion::V1,
        }
    }

    pub fn from_bytes_v2(bytes: [u8; SECRET_BYTES]) -> Self {
        Self {
            bytes,
            version: DerivationVersion::V2,
        }
    }

    /// RNG policy: all cryptographic material MUST use OsRng to draw
    /// directly from the operating-system entropy source.
    /// New secrets always use V2 (HKDF) derivation.
    pub fn generate() -> Self {
        let mut bytes = [0u8; SECRET_BYTES];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut bytes);
        Self {
            bytes,
            version: DerivationVersion::V2,
        }
    }

    /// Return the derivation version for this secret.
    pub fn version(&self) -> DerivationVersion {
        self.version
    }

    /// Upgrade an existing secret to V2 derivation.
    /// WARNING: This changes all derived keys. Only use when all mesh nodes
    /// have been upgraded and are ready for the transition.
    pub fn upgrade_to_v2(&self) -> Self {
        Self {
            bytes: self.bytes,
            version: DerivationVersion::V2,
        }
    }

    /// Deterministic mesh identifier (first 16 bytes of derived key).
    pub fn mesh_id(&self) -> [u8; 16] {
        let derived = match self.version {
            // Legacy: mesh_id was SHA256(secret) with no domain prefix.
            DerivationVersion::V1 => {
                let hash: [u8; 32] = Sha256::digest(self.bytes).into();
                hash
            }
            DerivationVersion::V2 => Self::derive_hkdf("mesh-id", &self.bytes),
        };
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
        self.derive("encryption-key", "encrypt:")
    }

    /// IPFS discovery key — used to derive the CID/path where peer records are published.
    pub fn ipfs_key(&self) -> [u8; 32] {
        self.derive("ipfs-key", "ipfs:")
    }

    /// IPFS key as hex string (used as filename/path on IPFS).
    pub fn ipfs_key_hex(&self) -> String {
        let key = self.ipfs_key();
        key.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn as_bytes(&self) -> &[u8; SECRET_BYTES] {
        &self.bytes
    }

    /// Derive a sub-key using the algorithm selected by the secret's version.
    ///
    /// - V1 (legacy): `SHA256(domain_v1 || secret)` — preserves compatibility
    ///   with nodes running the old code.
    /// - V2: HKDF-SHA256 (RFC 5869) with versioned salt and the `domain_v2`
    ///   info label.
    fn derive(&self, domain_v2: &str, domain_v1: &str) -> [u8; 32] {
        match self.version {
            DerivationVersion::V1 => Self::derive_legacy(domain_v1, &self.bytes),
            DerivationVersion::V2 => Self::derive_hkdf(domain_v2, &self.bytes),
        }
    }

    /// Legacy SHA-256 derivation for V1 secrets.
    fn derive_legacy(domain: &str, secret: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(domain.as_bytes());
        hasher.update(secret);
        hasher.finalize().into()
    }

    /// HKDF-SHA256 derivation for V2 secrets (RFC 5869).
    /// Salt is versioned to allow future rotation.
    fn derive_hkdf(info: &str, secret: &[u8]) -> [u8; 32] {
        let hkdf = Hkdf::<Sha256>::new(Some(b"syfrah-fabric-v1"), secret);
        let mut output = [0u8; 32];
        hkdf.expand(info.as_bytes(), &mut output)
            .expect("32 bytes is valid for HKDF-SHA256");
        output
    }
}

impl FromStr for MeshSecret {
    type Err = SecretError;

    /// Parse an existing secret string. Defaults to V1 (legacy derivation)
    /// so that secrets loaded from state are backward-compatible.
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
        Ok(Self {
            bytes,
            version: DerivationVersion::V1,
        })
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
        let secret = MeshSecret::from_bytes_v2([0xAB; 32]);
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
        let secret = MeshSecret::from_bytes_v2([0x42; 32]);
        let key1 = secret.encryption_key();
        let key2 = secret.encryption_key();
        assert_eq!(key1, key2);
    }

    #[test]
    fn v1_uses_legacy_sha256_derivation() {
        // V1 secrets must produce the same output as the old SHA256(domain || secret) code.
        let bytes = [0xAB; 32];
        let secret_v1 = MeshSecret::from_bytes(bytes);
        assert_eq!(secret_v1.version(), DerivationVersion::V1);

        // Manually compute expected legacy derivation for encryption_key (domain "encrypt:")
        let mut hasher = Sha256::new();
        hasher.update(b"encrypt:");
        hasher.update(&bytes);
        let expected: [u8; 32] = hasher.finalize().into();
        assert_eq!(secret_v1.encryption_key(), expected);
    }

    #[test]
    fn v2_uses_hkdf_derivation() {
        let bytes = [0xAB; 32];
        let secret_v2 = MeshSecret::from_bytes_v2(bytes);
        assert_eq!(secret_v2.version(), DerivationVersion::V2);

        // V2 must NOT match legacy derivation
        let mut hasher = Sha256::new();
        hasher.update(b"encrypt:");
        hasher.update(&bytes);
        let legacy: [u8; 32] = hasher.finalize().into();
        assert_ne!(secret_v2.encryption_key(), legacy);
    }

    #[test]
    fn parsed_secret_defaults_to_v1() {
        let secret = MeshSecret::generate();
        let encoded = secret.to_string();
        let parsed: MeshSecret = encoded.parse().unwrap();
        // Parsed from string => V1 for backward compatibility
        assert_eq!(parsed.version(), DerivationVersion::V1);
    }

    #[test]
    fn generated_secret_is_v2() {
        let secret = MeshSecret::generate();
        assert_eq!(secret.version(), DerivationVersion::V2);
    }

    #[test]
    fn upgrade_to_v2_changes_derivation() {
        let bytes = [0x42; 32];
        let v1 = MeshSecret::from_bytes(bytes);
        let v2 = v1.upgrade_to_v2();
        assert_eq!(v1.as_bytes(), v2.as_bytes());
        assert_eq!(v1.version(), DerivationVersion::V1);
        assert_eq!(v2.version(), DerivationVersion::V2);
        // Different derivation algorithms must produce different keys
        assert_ne!(v1.encryption_key(), v2.encryption_key());
        assert_ne!(v1.ipfs_key(), v2.ipfs_key());
    }

    #[test]
    fn v1_legacy_mesh_id_matches_sha256() {
        let bytes = [0x42; 32];
        let secret_v1 = MeshSecret::from_bytes(bytes);

        // Legacy mesh_id was SHA256(secret) with no domain prefix.
        let hash: [u8; 32] = Sha256::digest(bytes).into();
        let mut expected = [0u8; 16];
        expected.copy_from_slice(&hash[..16]);
        assert_eq!(secret_v1.mesh_id(), expected);
    }
}
