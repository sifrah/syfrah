//! Append-only audit log for security-relevant mesh events.
//!
//! Writes JSON lines to `~/.syfrah/audit.log` with `0o600` permissions.
//! Audit writes are best-effort: failures warn but never block the operation.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::warn;

/// Security-relevant audit event types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    /// A node requested to join the mesh.
    PeerJoinRequested,
    /// A join request was accepted (manual or PIN).
    PeerJoinAccepted,
    /// A join request was rejected (bad PIN, limit, timeout, operator).
    PeerJoinRejected,
    /// A peer was removed from the mesh.
    PeerRemoved,
    /// Peering listener was started.
    PeeringStarted,
    /// Peering listener was stopped.
    PeeringStopped,
    /// The mesh secret was rotated.
    SecretRotated,
    /// The daemon was started.
    DaemonStarted,
    /// The daemon was stopped.
    DaemonStopped,
    /// Configuration was reloaded at runtime.
    ConfigReloaded,
}

impl std::fmt::Display for AuditEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AuditEventType::PeerJoinRequested => "peer.join.requested",
            AuditEventType::PeerJoinAccepted => "peer.join.accepted",
            AuditEventType::PeerJoinRejected => "peer.join.rejected",
            AuditEventType::PeerRemoved => "peer.removed",
            AuditEventType::PeeringStarted => "peering.started",
            AuditEventType::PeeringStopped => "peering.stopped",
            AuditEventType::SecretRotated => "secret.rotated",
            AuditEventType::DaemonStarted => "daemon.started",
            AuditEventType::DaemonStopped => "daemon.stopped",
            AuditEventType::ConfigReloaded => "config.reloaded",
        };
        write!(f, "{s}")
    }
}

impl AuditEventType {
    /// Parse a dotted event type string (e.g. `"peer.join.accepted"`).
    pub fn from_dotted(s: &str) -> Option<Self> {
        match s {
            "peer.join.requested" => Some(Self::PeerJoinRequested),
            "peer.join.accepted" => Some(Self::PeerJoinAccepted),
            "peer.join.rejected" => Some(Self::PeerJoinRejected),
            "peer.removed" => Some(Self::PeerRemoved),
            "peering.started" => Some(Self::PeeringStarted),
            "peering.stopped" => Some(Self::PeeringStopped),
            "secret.rotated" => Some(Self::SecretRotated),
            "daemon.started" => Some(Self::DaemonStarted),
            "daemon.stopped" => Some(Self::DaemonStopped),
            "config.reloaded" => Some(Self::ConfigReloaded),
            _ => None,
        }
    }
}

/// A single audit log entry (one JSON line).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Unix timestamp (seconds).
    pub timestamp: u64,
    /// Dotted event type string.
    pub event_type: String,
    /// Peer node name, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_name: Option<String>,
    /// Peer endpoint (IP:port), if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer_endpoint: Option<String>,
    /// Free-form details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    /// UID of the Unix peer that issued the control command (via SO_PEERCRED).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller_uid: Option<u32>,
}

/// Path to the audit log file.
pub fn audit_log_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".syfrah")
        .join("audit.log")
}

/// Emit an audit event. Best-effort: logs a warning on failure, never panics.
pub fn emit(
    event_type: AuditEventType,
    peer_name: Option<&str>,
    peer_endpoint: Option<&str>,
    details: Option<&str>,
) {
    if let Err(e) = emit_inner(event_type, peer_name, peer_endpoint, details, None) {
        warn!("failed to write audit log: {e}");
    }
}

/// Emit an audit event with the caller's UID attached.
pub fn emit_with_uid(
    event_type: AuditEventType,
    peer_name: Option<&str>,
    peer_endpoint: Option<&str>,
    details: Option<&str>,
    caller_uid: Option<u32>,
) {
    if let Err(e) = emit_inner(event_type, peer_name, peer_endpoint, details, caller_uid) {
        warn!("failed to write audit log: {e}");
    }
}

fn emit_inner(
    event_type: AuditEventType,
    peer_name: Option<&str>,
    peer_endpoint: Option<&str>,
    details: Option<&str>,
    caller_uid: Option<u32>,
) -> std::io::Result<()> {
    let path = audit_log_path();
    emit_to_path(
        &path,
        event_type,
        peer_name,
        peer_endpoint,
        details,
        caller_uid,
    )
}

/// Write an audit entry to an explicit file path.
/// Used by tests to avoid relying on `HOME` env var.
fn emit_to_path(
    path: &std::path::Path,
    event_type: AuditEventType,
    peer_name: Option<&str>,
    peer_endpoint: Option<&str>,
    details: Option<&str>,
    caller_uid: Option<u32>,
) -> std::io::Result<()> {
    let entry = AuditEntry {
        timestamp: now(),
        event_type: event_type.to_string(),
        peer_name: peer_name.map(String::from),
        peer_endpoint: peer_endpoint.map(String::from),
        details: details.map(String::from),
        caller_uid,
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }

    // Rotate if the audit log exceeds the configured maximum size.
    let tuning = crate::config::load_tuning().unwrap_or_default();
    let max_bytes = tuning.audit_max_size_mb * 1024 * 1024;
    rotate_if_needed(path, max_bytes);

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }

    let mut line = serde_json::to_string(&entry).map_err(std::io::Error::other)?;
    line.push('\n');
    file.write_all(line.as_bytes())?;
    Ok(())
}

/// Read and parse audit entries from the log file.
/// Returns entries in chronological order (oldest first).
pub fn read_entries() -> std::io::Result<Vec<AuditEntry>> {
    read_entries_from(&audit_log_path())
}

/// Read and parse audit entries from an explicit file path.
/// Used by tests to avoid relying on `HOME` env var.
fn read_entries_from(path: &std::path::Path) -> std::io::Result<Vec<AuditEntry>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let contents = fs::read_to_string(path)?;
    let mut entries = Vec::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<AuditEntry>(trimmed) {
            Ok(entry) => entries.push(entry),
            Err(e) => {
                warn!("skipping malformed audit line: {e}");
            }
        }
    }
    Ok(entries)
}

/// Rotate the audit log file if it exceeds `max_bytes`.
/// Renames the current file to `.log.old` and lets the caller create a fresh one.
fn rotate_if_needed(path: &std::path::Path, max_bytes: u64) {
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() > max_bytes {
            let old = path.with_extension("log.old");
            let _ = fs::rename(path, &old);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&old, fs::Permissions::from_mode(0o600));
            }
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_entry_serialization_roundtrip() {
        let entry = AuditEntry {
            timestamp: 1700000000,
            event_type: AuditEventType::PeerJoinAccepted.to_string(),
            peer_name: Some("node-1".into()),
            peer_endpoint: Some("10.0.0.1:51820".into()),
            details: Some("pin-matched".into()),
            caller_uid: Some(1000),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: AuditEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.timestamp, 1700000000);
        assert_eq!(parsed.event_type, "peer.join.accepted");
        assert_eq!(parsed.peer_name.as_deref(), Some("node-1"));
    }

    #[test]
    fn event_type_display_and_parse() {
        let cases = [
            (AuditEventType::PeerJoinRequested, "peer.join.requested"),
            (AuditEventType::PeerJoinAccepted, "peer.join.accepted"),
            (AuditEventType::PeerJoinRejected, "peer.join.rejected"),
            (AuditEventType::PeerRemoved, "peer.removed"),
            (AuditEventType::PeeringStarted, "peering.started"),
            (AuditEventType::PeeringStopped, "peering.stopped"),
            (AuditEventType::SecretRotated, "secret.rotated"),
            (AuditEventType::DaemonStarted, "daemon.started"),
            (AuditEventType::DaemonStopped, "daemon.stopped"),
            (AuditEventType::ConfigReloaded, "config.reloaded"),
        ];
        for (variant, expected) in &cases {
            assert_eq!(variant.to_string(), *expected);
            assert_eq!(AuditEventType::from_dotted(expected), Some(variant.clone()));
        }
        assert_eq!(AuditEventType::from_dotted("bogus"), None);
    }

    #[test]
    fn write_and_read_audit_log() {
        let tmp = tempfile::tempdir().unwrap();
        let log_path = tmp.path().join("audit.log");

        emit_to_path(
            &log_path,
            AuditEventType::DaemonStarted,
            None,
            None,
            Some("test-start"),
            None,
        )
        .unwrap();
        emit_to_path(
            &log_path,
            AuditEventType::PeerJoinAccepted,
            Some("peer-1"),
            Some("1.2.3.4:51820"),
            None,
            None,
        )
        .unwrap();

        let entries = read_entries_from(&log_path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].event_type, "daemon.started");
        assert_eq!(entries[0].details.as_deref(), Some("test-start"));
        assert_eq!(entries[1].event_type, "peer.join.accepted");
        assert_eq!(entries[1].peer_name.as_deref(), Some("peer-1"));
    }

    #[test]
    fn rotates_audit_log_when_exceeding_max_size() {
        let tmp = tempfile::tempdir().unwrap();
        let log_path = tmp.path().join("audit.log");
        let old_path = tmp.path().join("audit.log.old");

        // Write a file larger than the threshold.
        let big_data = vec![b'x'; 2 * 1024 * 1024];
        std::fs::write(&log_path, &big_data).unwrap();
        assert!(!old_path.exists());

        // Rotation should rename to .old when file exceeds max_bytes.
        let max_bytes: u64 = 1024 * 1024; // 1 MB threshold
        rotate_if_needed(&log_path, max_bytes);

        assert!(
            old_path.exists(),
            "audit.log.old should exist after rotation"
        );
        assert!(
            !log_path.exists(),
            "original audit.log should be gone after rename"
        );
    }

    #[test]
    fn does_not_rotate_audit_log_under_max_size() {
        let tmp = tempfile::tempdir().unwrap();
        let log_path = tmp.path().join("audit.log");
        let old_path = tmp.path().join("audit.log.old");

        // Write a small file.
        std::fs::write(&log_path, b"small").unwrap();

        let max_bytes: u64 = 1024 * 1024; // 1 MB threshold
        rotate_if_needed(&log_path, max_bytes);

        assert!(
            log_path.exists(),
            "audit.log should remain when under threshold"
        );
        assert!(
            !old_path.exists(),
            "audit.log.old should not exist when under threshold"
        );
    }

    #[test]
    fn skips_none_fields_in_json() {
        let entry = AuditEntry {
            timestamp: 100,
            event_type: AuditEventType::DaemonStarted.to_string(),
            peer_name: None,
            peer_endpoint: None,
            details: None,
            caller_uid: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("peer_name"));
        assert!(!json.contains("peer_endpoint"));
        assert!(!json.contains("details"));
    }
}
