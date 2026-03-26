use rand::Rng;
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Error code constants
// ---------------------------------------------------------------------------

pub const FABRIC_PEER_NOT_FOUND: &str = "FABRIC_PEER_NOT_FOUND";
pub const FABRIC_DAEMON_NOT_RUNNING: &str = "FABRIC_DAEMON_NOT_RUNNING";
pub const FABRIC_MESH_NOT_INITIALIZED: &str = "FABRIC_MESH_NOT_INITIALIZED";
pub const FABRIC_HANDSHAKE_FAILED: &str = "FABRIC_HANDSHAKE_FAILED";
pub const FABRIC_TUNNEL_TIMEOUT: &str = "FABRIC_TUNNEL_TIMEOUT";
pub const STATE_STORE_UNAVAILABLE: &str = "STATE_STORE_UNAVAILABLE";
pub const STATE_CONFLICT: &str = "STATE_CONFLICT";
pub const AUTH_UNAUTHORIZED: &str = "AUTH_UNAUTHORIZED";
pub const AUTH_FORBIDDEN: &str = "AUTH_FORBIDDEN";
pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";

// ---------------------------------------------------------------------------
// Trace ID generation
// ---------------------------------------------------------------------------

/// Generate a 12-character random hex trace ID prefixed with `req-`.
///
/// Example output: `req-a7f3e29b1c04`
pub fn generate_trace_id() -> String {
    let mut rng = rand::thread_rng();
    let bytes: [u8; 6] = rng.gen();
    format!("req-{}", hex_encode(&bytes))
}

/// Minimal hex encoder to avoid pulling in the `hex` crate.
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

// ---------------------------------------------------------------------------
// ApiError
// ---------------------------------------------------------------------------

/// Structured API error returned to clients.
///
/// Serialises to:
/// ```json
/// {"code":"FABRIC_PEER_NOT_FOUND","message":"peer xyz not found","trace_id":"req-a7f3e29b1c04"}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiError {
    /// Machine-readable error code (e.g. `FABRIC_PEER_NOT_FOUND`).
    pub code: String,
    /// Human-readable description of what went wrong.
    pub message: String,
    /// Per-request trace identifier for log correlation.
    pub trace_id: String,
}

impl ApiError {
    /// Create a new `ApiError`, automatically generating a trace ID.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            trace_id: generate_trace_id(),
        }
    }

    /// Create an `ApiError` with an explicit trace ID (useful in tests or
    /// when propagating a trace ID from an incoming request).
    pub fn with_trace_id(
        code: impl Into<String>,
        message: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            trace_id: trace_id.into(),
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Error [{}]: {}", self.trace_id, self.message)
    }
}

impl std::error::Error for ApiError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn trace_id_format() {
        let tid = generate_trace_id();
        assert!(tid.starts_with("req-"), "should start with req-: {tid}");
        // "req-" (4 chars) + 12 hex chars = 16 total
        assert_eq!(tid.len(), 16, "unexpected length: {tid}");
        assert!(
            tid[4..].chars().all(|c| c.is_ascii_hexdigit()),
            "non-hex chars in: {tid}"
        );
    }

    #[test]
    fn trace_id_uniqueness() {
        let ids: HashSet<String> = (0..1000).map(|_| generate_trace_id()).collect();
        assert_eq!(ids.len(), 1000, "generated duplicate trace IDs");
    }

    #[test]
    fn serialization_roundtrip() {
        let err = ApiError::with_trace_id(
            FABRIC_PEER_NOT_FOUND,
            "peer abc123 not found",
            "req-aabbccddeeff",
        );

        let json = serde_json::to_string(&err).expect("serialize");
        let back: ApiError = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(err, back);
    }

    #[test]
    fn display_format() {
        let err = ApiError::with_trace_id(
            FABRIC_DAEMON_NOT_RUNNING,
            "daemon is not running",
            "req-000000000001",
        );
        assert_eq!(
            err.to_string(),
            "Error [req-000000000001]: daemon is not running"
        );
    }

    #[test]
    fn json_shape() {
        let err = ApiError::with_trace_id(INTERNAL_ERROR, "something broke", "req-ffffffffffff");
        let val: serde_json::Value = serde_json::to_value(&err).expect("to_value");

        assert_eq!(val["code"], "INTERNAL_ERROR");
        assert_eq!(val["message"], "something broke");
        assert_eq!(val["trace_id"], "req-ffffffffffff");
    }

    #[test]
    fn new_generates_trace_id() {
        let err = ApiError::new(AUTH_UNAUTHORIZED, "bad token");
        assert!(err.trace_id.starts_with("req-"));
        assert_eq!(err.trace_id.len(), 16);
    }

    #[test]
    fn error_code_constants_are_non_empty() {
        let codes = [
            FABRIC_PEER_NOT_FOUND,
            FABRIC_DAEMON_NOT_RUNNING,
            FABRIC_MESH_NOT_INITIALIZED,
            FABRIC_HANDSHAKE_FAILED,
            FABRIC_TUNNEL_TIMEOUT,
            STATE_STORE_UNAVAILABLE,
            STATE_CONFLICT,
            AUTH_UNAUTHORIZED,
            AUTH_FORBIDDEN,
            INTERNAL_ERROR,
        ];
        for code in codes {
            assert!(!code.is_empty(), "error code constant must not be empty");
        }
    }
}
