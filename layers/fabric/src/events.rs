//! Persistent event log for mesh activity.
//!
//! Events are stored in a redb `events` table with auto-incrementing keys.
//! A ring buffer prunes the oldest entries when the count exceeds a
//! configurable maximum (default 100).

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::warn;

use syfrah_state::LayerDb;

use crate::store::StoreError;

const LAYER_NAME: &str = "fabric";
const EVENTS_TABLE: &str = "events";
const DEFAULT_MAX_EVENTS: u64 = 100;

/// A single mesh event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshEvent {
    pub timestamp: u64,
    pub event_type: EventType,
    pub peer_name: Option<String>,
    pub peer_endpoint: Option<String>,
    pub details: Option<String>,
}

/// All event types tracked by the fabric layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    DaemonStarted,
    DaemonStopped,
    JoinRequestReceived,
    JoinAutoAccepted,
    JoinManuallyAccepted,
    JoinRejected,
    JoinTimeout,
    PeerAnnounceReceived,
    PeerAnnounceFailed,
    PeerActive,
    PeerUnreachable,
    PeerRecovered,
    PeerRemoved,
    ReconciliationRun,
    HealthCheckRun,
    AnnounceDropped,
    PeerLimitReached,
    ConfigReloaded,
    ConfigReloadFailed,
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            EventType::DaemonStarted => "daemon-started",
            EventType::DaemonStopped => "daemon-stopped",
            EventType::JoinRequestReceived => "join-request-received",
            EventType::JoinAutoAccepted => "join-auto-accepted",
            EventType::JoinManuallyAccepted => "join-manually-accepted",
            EventType::JoinRejected => "join-rejected",
            EventType::JoinTimeout => "join-timeout",
            EventType::PeerAnnounceReceived => "peer-announce-received",
            EventType::PeerAnnounceFailed => "peer-announce-failed",
            EventType::PeerActive => "peer-active",
            EventType::PeerUnreachable => "peer-unreachable",
            EventType::PeerRecovered => "peer-recovered",
            EventType::PeerRemoved => "peer-removed",
            EventType::ReconciliationRun => "reconciliation-run",
            EventType::HealthCheckRun => "health-check-run",
            EventType::AnnounceDropped => "announce-dropped",
            EventType::PeerLimitReached => "peer-limit-reached",
            EventType::ConfigReloaded => "config-reloaded",
            EventType::ConfigReloadFailed => "config-reload-failed",
        };
        write!(f, "{s}")
    }
}

/// Record a mesh event. Silently logs a warning on failure (events are
/// best-effort and must never crash the daemon).
pub fn record(event: MeshEvent, max_events: Option<u64>) {
    if let Err(e) = record_inner(event, max_events) {
        warn!("failed to record event: {e}");
    }
}

fn record_inner(event: MeshEvent, max_events: Option<u64>) -> Result<(), StoreError> {
    let db = LayerDb::open(LAYER_NAME)?;
    let max = max_events.unwrap_or(DEFAULT_MAX_EVENTS);

    // Auto-incrementing key: use next_event_id metric
    let id = db.inc_metric("next_event_id", 1)?;
    let key = format!("{id:020}");
    db.set(EVENTS_TABLE, &key, &event)?;

    // Ring buffer: prune oldest if over limit
    let count = db.count(EVENTS_TABLE)?;
    if count > max {
        let excess = count - max;
        let entries: Vec<(String, MeshEvent)> = db.list(EVENTS_TABLE)?;
        for (k, _) in entries.into_iter().take(excess as usize) {
            let _ = db.delete(EVENTS_TABLE, &k);
        }
    }

    Ok(())
}

/// Load all events, sorted oldest first.
pub fn list_events() -> Result<Vec<MeshEvent>, StoreError> {
    if !LayerDb::layer_exists(LAYER_NAME) {
        return Ok(vec![]);
    }
    let db = LayerDb::open(LAYER_NAME)?;
    let entries: Vec<(String, MeshEvent)> = db.list(EVENTS_TABLE)?;
    // Keys are zero-padded numbers, so lexicographic order == chronological order
    Ok(entries.into_iter().map(|(_, e)| e).collect())
}

/// Helper to build and record a simple event.
pub fn emit(
    event_type: EventType,
    peer_name: Option<&str>,
    peer_endpoint: Option<&str>,
    details: Option<&str>,
    max_events: Option<u64>,
) {
    let event = MeshEvent {
        timestamp: now(),
        event_type,
        peer_name: peer_name.map(|s| s.to_string()),
        peer_endpoint: peer_endpoint.map(|s| s.to_string()),
        details: details.map(|s| s.to_string()),
    };
    record(event, max_events);
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
    fn event_type_display() {
        assert_eq!(EventType::DaemonStarted.to_string(), "daemon-started");
        assert_eq!(
            EventType::JoinRequestReceived.to_string(),
            "join-request-received"
        );
        assert_eq!(EventType::PeerRecovered.to_string(), "peer-recovered");
    }

    #[test]
    fn mesh_event_serialization_roundtrip() {
        let event = MeshEvent {
            timestamp: 1234567890,
            event_type: EventType::PeerActive,
            peer_name: Some("node-1".into()),
            peer_endpoint: Some("10.0.0.1:51820".into()),
            details: Some("mesh_ipv6=fd12::1".into()),
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: MeshEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.timestamp, 1234567890);
        assert_eq!(deserialized.peer_name, Some("node-1".into()));
    }
}
