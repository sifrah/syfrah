//! API key lifecycle: create, validate, rotate, delete.
//!
//! Keys follow the format `syf_key_{project}_{random_256bit_base58}`.
//! Only the SHA-256 hash of the raw key is persisted; the plaintext is
//! returned exactly once at creation time.
//!
//! Storage is a JSON file at `~/.syfrah/apikeys.json` (mode 0600).
//! This will be replaced by Raft-replicated state once the controlplane
//! layer lands.

use crate::error::{ApiError, AUTH_FORBIDDEN, AUTH_UNAUTHORIZED, INTERNAL_ERROR};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Role
// ---------------------------------------------------------------------------

/// Roles assignable to an API key, ordered from most to least privileged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Owner,
    Admin,
    Developer,
    Viewer,
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Role::Owner => write!(f, "owner"),
            Role::Admin => write!(f, "admin"),
            Role::Developer => write!(f, "developer"),
            Role::Viewer => write!(f, "viewer"),
        }
    }
}

// ---------------------------------------------------------------------------
// ApiKey
// ---------------------------------------------------------------------------

/// Metadata stored alongside the SHA-256 hash of an API key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    /// Human-readable name (unique within a project).
    pub name: String,
    /// Short project identifier this key belongs to.
    pub project: String,
    /// Permission role.
    pub role: Role,
    /// Hex-encoded SHA-256 hash of the raw key string.
    pub hash: String,
    /// Optional CIDR allowlist. Empty means any source IP is accepted.
    pub allowed_cidrs: Vec<String>,
    /// Time-to-live in seconds. `0` means no expiry.
    pub ttl: u64,
    /// Unix timestamp (seconds) when the key was created.
    pub created_at: u64,
    /// Unix timestamp of the last successful validation, or `None`.
    pub last_used_at: Option<u64>,
    /// IP address that last used the key, or `None`.
    pub last_used_ip: Option<String>,
    /// If set, the key is in a grace period and expires at this unix timestamp.
    pub grace_expires_at: Option<u64>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Base58 alphabet (Bitcoin-style, no ambiguous characters).
const BASE58_ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn base58_encode(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        result.push(BASE58_ALPHABET[(b as usize) % 58] as char);
    }
    result
}

fn sha256_hex(data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    let hash = hasher.finalize();
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Return the default key store path: `~/.syfrah/apikeys.json`.
pub fn default_store_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    PathBuf::from(home).join(".syfrah").join("apikeys.json")
}

// ---------------------------------------------------------------------------
// Key store (JSON file)
// ---------------------------------------------------------------------------

/// A thin wrapper around a `Vec<ApiKey>` persisted as JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyStore {
    pub keys: Vec<ApiKey>,
}

impl KeyStore {
    /// Load keys from the given path, returning an empty store if the file
    /// does not exist.
    pub fn load(path: &std::path::Path) -> Result<Self, ApiError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = std::fs::read_to_string(path)
            .map_err(|e| ApiError::new(INTERNAL_ERROR, format!("failed to read key store: {e}")))?;
        serde_json::from_str(&data)
            .map_err(|e| ApiError::new(INTERNAL_ERROR, format!("failed to parse key store: {e}")))
    }

    /// Persist the store to `path`, creating parent directories as needed and
    /// setting file permissions to 0600.
    pub fn save(&self, path: &std::path::Path) -> Result<(), ApiError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ApiError::new(INTERNAL_ERROR, format!("failed to create dir: {e}")))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| ApiError::new(INTERNAL_ERROR, format!("failed to serialize keys: {e}")))?;
        std::fs::write(path, &json).map_err(|e| {
            ApiError::new(INTERNAL_ERROR, format!("failed to write key store: {e}"))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(path, perms)
                .map_err(|e| ApiError::new(INTERNAL_ERROR, format!("failed to set perms: {e}")))?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Generate a new API key for the given project and role.
///
/// Returns `(raw_key_string, ApiKey)`. The raw key is shown once and never
/// stored; only its SHA-256 hash is kept.
pub fn generate_key(project: &str, role: Role) -> (String, ApiKey) {
    let mut rng = rand::thread_rng();
    let random_bytes: [u8; 32] = rng.gen();
    let encoded = base58_encode(&random_bytes);
    let raw_key = format!("syf_key_{project}_{encoded}");
    let hash = sha256_hex(&raw_key);
    let name = format!("{project}-{}", &encoded[..8]);

    let key = ApiKey {
        name,
        project: project.to_string(),
        role,
        hash,
        allowed_cidrs: Vec::new(),
        ttl: 0,
        created_at: now_secs(),
        last_used_at: None,
        last_used_ip: None,
        grace_expires_at: None,
    };
    (raw_key, key)
}

/// Validate a raw key string against the store.
///
/// On success the matching `ApiKey` is returned (with `last_used_at` and
/// `last_used_ip` updated). On failure an `ApiError` is returned.
pub fn validate_key(
    raw_key: &str,
    keys: &mut [ApiKey],
    source_ip: Option<IpAddr>,
) -> Result<ApiKey, ApiError> {
    let hash = sha256_hex(raw_key);
    let now = now_secs();

    let key = keys
        .iter_mut()
        .find(|k| k.hash == hash)
        .ok_or_else(|| ApiError::new(AUTH_UNAUTHORIZED, "invalid API key"))?;

    // Check grace period expiry.
    if let Some(grace) = key.grace_expires_at {
        if now > grace {
            return Err(ApiError::new(
                AUTH_UNAUTHORIZED,
                "API key grace period expired",
            ));
        }
    }

    // Check TTL-based expiry.
    if key.ttl > 0 && now > key.created_at + key.ttl {
        return Err(ApiError::new(AUTH_UNAUTHORIZED, "API key expired"));
    }

    // Check CIDR allowlist.
    if !key.allowed_cidrs.is_empty() {
        if let Some(ip) = source_ip {
            if !cidr_contains_ip(&key.allowed_cidrs, ip) {
                return Err(ApiError::new(
                    AUTH_FORBIDDEN,
                    "source IP not in CIDR allowlist",
                ));
            }
        } else {
            return Err(ApiError::new(
                AUTH_FORBIDDEN,
                "CIDR allowlist set but no source IP provided",
            ));
        }
    }

    // Update usage metadata.
    key.last_used_at = Some(now);
    if let Some(ip) = source_ip {
        key.last_used_ip = Some(ip.to_string());
    }

    Ok(key.clone())
}

/// Rotate an existing key: create a new key with the same project/role and
/// put the old key into a grace period.
///
/// Returns `(new_raw_key, new_ApiKey)`.
pub fn rotate_key(
    old_name: &str,
    grace_minutes: u64,
    keys: &mut Vec<ApiKey>,
) -> Result<(String, ApiKey), ApiError> {
    let now = now_secs();

    let old = keys
        .iter_mut()
        .find(|k| k.name == old_name)
        .ok_or_else(|| ApiError::new(AUTH_UNAUTHORIZED, "key not found"))?;

    // Set grace period on old key.
    old.grace_expires_at = Some(now + grace_minutes * 60);
    let project = old.project.clone();
    let role = old.role;

    let (raw, new_key) = generate_key(&project, role);
    keys.push(new_key.clone());
    Ok((raw, new_key))
}

/// Delete (revoke) a key immediately by name.
pub fn delete_key(name: &str, keys: &mut Vec<ApiKey>) -> Result<(), ApiError> {
    let before = keys.len();
    keys.retain(|k| k.name != name);
    if keys.len() == before {
        return Err(ApiError::new(AUTH_UNAUTHORIZED, "key not found"));
    }
    Ok(())
}

/// List keys for a project (never exposes the hash).
pub fn list_keys(project: &str, keys: &[ApiKey]) -> Vec<ApiKeySummary> {
    keys.iter()
        .filter(|k| k.project == project)
        .map(|k| ApiKeySummary {
            name: k.name.clone(),
            role: k.role,
            created_at: k.created_at,
            last_used_at: k.last_used_at,
        })
        .collect()
}

/// Summary returned by [`list_keys`] — never includes the hash or raw key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeySummary {
    pub name: String,
    pub role: Role,
    pub created_at: u64,
    pub last_used_at: Option<u64>,
}

// ---------------------------------------------------------------------------
// CIDR matching
// ---------------------------------------------------------------------------

/// Check whether `ip` falls within any of the given CIDR strings.
///
/// Supports both IPv4 (`10.0.0.0/8`) and IPv6 (`fd00::/8`).
/// A plain IP without a prefix length is treated as /32 or /128.
fn cidr_contains_ip(cidrs: &[String], ip: IpAddr) -> bool {
    for cidr in cidrs {
        if let Some(matched) = cidr_match(cidr, ip) {
            if matched {
                return true;
            }
        }
    }
    false
}

fn cidr_match(cidr: &str, ip: IpAddr) -> Option<bool> {
    let (net_str, prefix_len) = if let Some((n, p)) = cidr.split_once('/') {
        let prefix: u32 = p.parse().ok()?;
        (n, prefix)
    } else {
        let addr: IpAddr = cidr.parse().ok()?;
        let prefix = match addr {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        (cidr, prefix)
    };

    let net_addr: IpAddr = net_str.parse().ok()?;

    match (net_addr, ip) {
        (IpAddr::V4(net), IpAddr::V4(addr)) => {
            if prefix_len > 32 {
                return None;
            }
            let mask = if prefix_len == 0 {
                0u32
            } else {
                u32::MAX << (32 - prefix_len)
            };
            Some((u32::from(net) & mask) == (u32::from(addr) & mask))
        }
        (IpAddr::V6(net), IpAddr::V6(addr)) => {
            if prefix_len > 128 {
                return None;
            }
            let mask = if prefix_len == 0 {
                0u128
            } else {
                u128::MAX << (128 - prefix_len)
            };
            Some((u128::from(net) & mask) == (u128::from(addr) & mask))
        }
        _ => None, // v4/v6 mismatch
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn generate_roundtrip() {
        let (raw, key) = generate_key("proj1", Role::Developer);
        assert!(raw.starts_with("syf_key_proj1_"));
        assert_eq!(key.project, "proj1");
        assert_eq!(key.role, Role::Developer);
        assert_eq!(key.hash, sha256_hex(&raw));
        assert!(key.ttl == 0);
        assert!(key.last_used_at.is_none());
    }

    #[test]
    fn validate_correct_key() {
        let (raw, key) = generate_key("proj2", Role::Admin);
        let mut keys = vec![key];
        let result = validate_key(&raw, &mut keys, None);
        assert!(result.is_ok());
        let validated = result.unwrap();
        assert_eq!(validated.project, "proj2");
        assert_eq!(validated.role, Role::Admin);
        assert!(validated.last_used_at.is_some());
    }

    #[test]
    fn reject_wrong_key() {
        let (_raw, key) = generate_key("proj3", Role::Viewer);
        let mut keys = vec![key];
        let result = validate_key("syf_key_proj3_WRONG", &mut keys, None);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, AUTH_UNAUTHORIZED);
    }

    #[test]
    fn reject_expired_key() {
        let (raw, mut key) = generate_key("proj4", Role::Owner);
        // Set created_at in the past and a short TTL so it has expired.
        key.created_at = 1_000_000;
        key.ttl = 60; // expired long ago
        let mut keys = vec![key];
        let result = validate_key(&raw, &mut keys, None);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, AUTH_UNAUTHORIZED);
    }

    #[test]
    fn reject_wrong_cidr() {
        let (raw, mut key) = generate_key("proj5", Role::Developer);
        key.allowed_cidrs = vec!["10.0.0.0/8".to_string()];
        let mut keys = vec![key];

        // IP outside the allowlist.
        let bad_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let result = validate_key(&raw, &mut keys, Some(bad_ip));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, AUTH_FORBIDDEN);

        // IP inside the allowlist.
        let good_ip = IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3));
        let result = validate_key(&raw, &mut keys, Some(good_ip));
        assert!(result.is_ok());
    }

    #[test]
    fn rotate_sets_grace_period() {
        let (_raw, key) = generate_key("proj6", Role::Admin);
        let old_name = key.name.clone();
        let mut keys = vec![key];

        let result = rotate_key(&old_name, 5, &mut keys);
        assert!(result.is_ok());
        let (new_raw, new_key) = result.unwrap();
        assert!(new_raw.starts_with("syf_key_proj6_"));
        assert_eq!(new_key.role, Role::Admin);

        // Old key should have a grace expiry set.
        let old = keys.iter().find(|k| k.name == old_name).unwrap();
        assert!(old.grace_expires_at.is_some());
        // Grace should be ~5 minutes from now.
        let grace = old.grace_expires_at.unwrap();
        let now = now_secs();
        assert!(grace >= now + 4 * 60 && grace <= now + 6 * 60);

        // Store should have 2 keys now.
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn delete_key_removes_it() {
        let (_raw, key) = generate_key("proj7", Role::Viewer);
        let name = key.name.clone();
        let mut keys = vec![key];
        assert!(delete_key(&name, &mut keys).is_ok());
        assert!(keys.is_empty());
    }

    #[test]
    fn delete_missing_key_errors() {
        let mut keys: Vec<ApiKey> = Vec::new();
        let result = delete_key("no-such-key", &mut keys);
        assert!(result.is_err());
    }

    #[test]
    fn list_keys_filters_by_project() {
        let (_, k1) = generate_key("alpha", Role::Admin);
        let (_, k2) = generate_key("beta", Role::Viewer);
        let (_, k3) = generate_key("alpha", Role::Developer);
        let keys = vec![k1, k2, k3];
        let listed = list_keys("alpha", &keys);
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().all(|s| s.name.starts_with("alpha-")));
    }

    #[test]
    fn cidr_ipv6_match() {
        let cidrs = vec!["fd00::/8".to_string()];
        let ip = IpAddr::V6(Ipv6Addr::new(0xfd12, 0, 0, 0, 0, 0, 0, 1));
        assert!(cidr_contains_ip(&cidrs, ip));
        let outside = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        assert!(!cidr_contains_ip(&cidrs, outside));
    }

    #[test]
    fn store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("apikeys.json");

        let (_, key) = generate_key("storetest", Role::Owner);
        let store = KeyStore {
            keys: vec![key.clone()],
        };
        store.save(&path).unwrap();

        // Verify 0600 permissions.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&path).unwrap();
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }

        let loaded = KeyStore::load(&path).unwrap();
        assert_eq!(loaded.keys.len(), 1);
        assert_eq!(loaded.keys[0].name, key.name);
    }

    #[test]
    fn role_serialization() {
        let json = serde_json::to_string(&Role::Developer).unwrap();
        assert_eq!(json, "\"developer\"");
        let back: Role = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Role::Developer);
    }
}
