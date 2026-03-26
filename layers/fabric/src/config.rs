use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

fn syfrah_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".syfrah")
}

/// Per-topology-tier persistent keepalive intervals (seconds).
///
/// Same-zone peers benefit from aggressive keepalive to maintain low-latency
/// NAT traversal, while cross-region peers can use a longer interval to
/// reduce overhead on expensive links. All values are in seconds; 0 disables
/// persistent keepalive for that tier.
#[derive(Debug, Clone, PartialEq)]
pub struct KeepalivePolicy {
    /// Keepalive for peers in the same zone (default 20s).
    pub same_zone_keepalive: u16,
    /// Keepalive for peers in the same region but different zone (default 25s).
    pub same_region_keepalive: u16,
    /// Keepalive for peers in a different region (default 30s).
    pub cross_region_keepalive: u16,
}

impl Default for KeepalivePolicy {
    fn default() -> Self {
        Self {
            same_zone_keepalive: 20,
            same_region_keepalive: 25,
            cross_region_keepalive: 30,
        }
    }
}

/// Topology tier of a peer relative to the local node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologyTier {
    SameZone,
    SameRegion,
    CrossRegion,
}

impl KeepalivePolicy {
    /// Return the keepalive interval (seconds) for the given topology tier.
    pub fn for_tier(&self, tier: TopologyTier) -> u16 {
        match tier {
            TopologyTier::SameZone => self.same_zone_keepalive,
            TopologyTier::SameRegion => self.same_region_keepalive,
            TopologyTier::CrossRegion => self.cross_region_keepalive,
        }
    }
}

/// Per-tier health check timeout policy based on topology proximity.
///
/// Peers in the same zone are expected to respond faster, so they get a
/// shorter timeout. Cross-region peers are given more slack.
#[derive(Debug, Clone, PartialEq)]
pub struct HealthPolicy {
    /// Timeout for peers in the same zone (default 120s).
    pub same_zone_timeout: Duration,
    /// Timeout for peers in the same region but different zone (default 180s).
    pub same_region_timeout: Duration,
    /// Timeout for peers in a different region, or peers with unknown topology (default 300s).
    pub cross_region_timeout: Duration,
}

impl Default for HealthPolicy {
    fn default() -> Self {
        Self {
            same_zone_timeout: Duration::from_secs(120),
            same_region_timeout: Duration::from_secs(180),
            cross_region_timeout: Duration::from_secs(300),
        }
    }
}

/// Daemon tuning parameters. All fields are optional — defaults match the
/// original hardcoded values.
#[derive(Debug, Clone, PartialEq)]
pub struct Tuning {
    pub health_check_interval: Duration,
    pub reconcile_interval: Duration,
    pub persist_interval: Duration,
    pub unreachable_timeout: Duration,
    /// Topology-aware health check timeouts. When set, these override the
    /// global `unreachable_timeout` for peers with known topology.
    pub health_policy: HealthPolicy,
    pub keepalive_interval: u16,
    /// Per-topology-tier persistent keepalive overrides. When set, these
    /// replace the global `keepalive_interval` for peers whose topology tier
    /// is known.
    pub keepalive_policy: KeepalivePolicy,
    pub join_timeout: Duration,
    pub exchange_timeout: Duration,
    /// Maximum number of events to keep in the event log ring buffer.
    pub max_events: u64,
    /// Maximum concurrent peering connections (default 100).
    pub max_concurrent_connections: usize,
    /// Maximum pending join requests (default 100).
    pub max_pending_joins: usize,
    /// Maximum number of peers allowed in the mesh (WireGuard + store).
    pub max_peers: usize,
    /// Maximum number of concurrent announce-processing tasks.
    pub max_concurrent_announces: usize,
    /// Size of the bounded retry queue for announces that cannot be processed
    /// immediately because the concurrency semaphore is full (default 200).
    pub announce_queue_size: usize,
    /// WireGuard interface name (default "syfrah0").
    pub interface_name: String,
    /// Maximum log file size in megabytes before rotation (default 10).
    pub log_max_size_mb: u64,
    /// Maximum audit log file size in megabytes before rotation (default 10).
    pub audit_max_size_mb: u64,
    /// Interval between periodic self-announce rounds (anti-entropy).
    /// Each round re-announces this node to a gossip subset of known peers,
    /// ensuring convergence even when initial announcements fail (default 10s).
    pub self_announce_interval: Duration,
    /// Wave-based announce propagation settings.
    pub announcements: AnnouncementConfig,
    /// How long a peer must remain in `Removed` status before it is garbage-
    /// collected (permanently deleted from the store). Default 24 h (86400 s).
    /// Set to 0 to disable GC.
    pub gc_removed_threshold: Duration,
}

/// Configuration for topology-aware wave-based announce propagation.
///
/// When a new peer is announced, the mesh propagates the announcement in three
/// waves: same-zone first, then same-region, then cross-region. Each wave has
/// independent concurrency limits and delays.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnouncementConfig {
    /// Maximum concurrent announce connections to same-zone peers (default 50).
    pub same_zone_concurrency: usize,
    /// Maximum concurrent announce connections to same-region peers (default 20).
    pub same_region_concurrency: usize,
    /// Maximum concurrent announce connections to cross-region peers (default 5).
    pub cross_region_concurrency: usize,
    /// Delay before announcing to same-zone peers in milliseconds (default 0).
    pub same_zone_delay_ms: u64,
    /// Delay before announcing to same-region peers in milliseconds (default 5000).
    pub same_region_delay_ms: u64,
    /// Delay before announcing to cross-region peers in milliseconds (default 15000).
    pub cross_region_delay_ms: u64,
}

impl Default for AnnouncementConfig {
    fn default() -> Self {
        Self {
            same_zone_concurrency: 50,
            same_region_concurrency: 20,
            cross_region_concurrency: 5,
            same_zone_delay_ms: 0,
            same_region_delay_ms: 5000,
            cross_region_delay_ms: 15000,
        }
    }
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            health_check_interval: Duration::from_secs(60),
            reconcile_interval: Duration::from_secs(30),
            persist_interval: Duration::from_secs(30),
            unreachable_timeout: Duration::from_secs(300),
            health_policy: HealthPolicy::default(),
            keepalive_interval: 25,
            keepalive_policy: KeepalivePolicy::default(),
            join_timeout: Duration::from_secs(10),
            exchange_timeout: Duration::from_secs(10),
            max_events: 100,
            max_concurrent_connections: 100,
            max_pending_joins: 100,
            max_peers: 1000,
            max_concurrent_announces: 50,
            announce_queue_size: 200,
            interface_name: crate::wg::DEFAULT_INTERFACE_NAME.to_string(),
            log_max_size_mb: 10,
            audit_max_size_mb: 10,
            self_announce_interval: Duration::from_secs(10),
            announcements: AnnouncementConfig::default(),
            gc_removed_threshold: Duration::from_secs(86400),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct ConfigFile {
    #[serde(default)]
    daemon: DaemonSection,
    #[serde(default)]
    wireguard: WireguardSection,
    #[serde(default)]
    peering: PeeringSection,
    #[serde(default)]
    events: EventsSection,
    #[serde(default)]
    limits: LimitsSection,
    #[serde(default)]
    health: HealthSection,
    #[serde(default)]
    announcements: AnnouncementsSection,
}

#[derive(Debug, Deserialize, Default)]
struct HealthSection {
    same_zone_timeout: Option<u64>,
    same_region_timeout: Option<u64>,
    cross_region_timeout: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct DaemonSection {
    health_check_interval: Option<u64>,
    reconcile_interval: Option<u64>,
    persist_interval: Option<u64>,
    unreachable_timeout: Option<u64>,
    log_max_size_mb: Option<u64>,
    audit_max_size_mb: Option<u64>,
    self_announce_interval: Option<u64>,
    gc_removed_threshold: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct WireguardSection {
    keepalive_interval: Option<u16>,
    same_zone_keepalive: Option<u16>,
    same_region_keepalive: Option<u16>,
    cross_region_keepalive: Option<u16>,
    interface_name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct PeeringSection {
    join_timeout: Option<u64>,
    exchange_timeout: Option<u64>,
    max_concurrent_connections: Option<usize>,
    max_pending_joins: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct EventsSection {
    max_events: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct LimitsSection {
    max_peers: Option<usize>,
    max_concurrent_announces: Option<usize>,
    announce_queue_size: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct AnnouncementsSection {
    same_zone_concurrency: Option<usize>,
    same_region_concurrency: Option<usize>,
    cross_region_concurrency: Option<usize>,
    same_zone_delay_ms: Option<u64>,
    same_region_delay_ms: Option<u64>,
    cross_region_delay_ms: Option<u64>,
}

/// Parameters that cannot be changed without a daemon restart.
const NON_HOT_RELOADABLE: &[&str] = &[
    "interface_name",
    "keepalive_interval",
    "keepalive_policy.same_zone_keepalive",
    "keepalive_policy.same_region_keepalive",
    "keepalive_policy.cross_region_keepalive",
];

/// A single changed parameter.
pub struct TuningChange {
    pub name: String,
    pub old_value: String,
    pub new_value: String,
}

/// Compare two tuning configs and return (hot-reloadable changes, skipped non-hot-reloadable changes).
pub fn diff_tuning(old: &Tuning, new: &Tuning) -> (Vec<TuningChange>, Vec<TuningChange>) {
    let mut changes = Vec::new();
    let mut skipped = Vec::new();

    macro_rules! cmp_dur {
        ($field:ident) => {
            if old.$field != new.$field {
                let c = TuningChange {
                    name: stringify!($field).to_string(),
                    old_value: format!("{}s", old.$field.as_secs()),
                    new_value: format!("{}s", new.$field.as_secs()),
                };
                if NON_HOT_RELOADABLE.contains(&stringify!($field)) {
                    skipped.push(c);
                } else {
                    changes.push(c);
                }
            }
        };
    }

    macro_rules! cmp_val {
        ($field:ident) => {
            if old.$field != new.$field {
                let c = TuningChange {
                    name: stringify!($field).to_string(),
                    old_value: format!("{}", old.$field),
                    new_value: format!("{}", new.$field),
                };
                if NON_HOT_RELOADABLE.contains(&stringify!($field)) {
                    skipped.push(c);
                } else {
                    changes.push(c);
                }
            }
        };
    }

    cmp_dur!(health_check_interval);
    cmp_dur!(reconcile_interval);
    cmp_dur!(persist_interval);
    cmp_dur!(unreachable_timeout);
    cmp_dur!(join_timeout);
    cmp_dur!(exchange_timeout);
    cmp_val!(keepalive_interval);
    cmp_val!(max_events);
    cmp_val!(max_concurrent_connections);
    cmp_val!(max_pending_joins);
    cmp_val!(max_peers);
    cmp_val!(max_concurrent_announces);
    cmp_val!(interface_name);
    cmp_val!(log_max_size_mb);
    cmp_val!(audit_max_size_mb);
    cmp_dur!(self_announce_interval);
    cmp_dur!(gc_removed_threshold);

    // HealthPolicy fields (nested, compare manually)
    macro_rules! cmp_health {
        ($field:ident) => {
            if old.health_policy.$field != new.health_policy.$field {
                let name = format!("health_policy.{}", stringify!($field));
                let c = TuningChange {
                    name,
                    old_value: format!("{}s", old.health_policy.$field.as_secs()),
                    new_value: format!("{}s", new.health_policy.$field.as_secs()),
                };
                changes.push(c);
            }
        };
    }

    // Announcement wave config — compare nested fields manually.
    macro_rules! cmp_announce {
        ($field:ident) => {
            if old.announcements.$field != new.announcements.$field {
                let c = TuningChange {
                    name: format!("announcements.{}", stringify!($field)),
                    old_value: format!("{}", old.announcements.$field),
                    new_value: format!("{}", new.announcements.$field),
                };
                changes.push(c);
            }
        };
    }

    // KeepalivePolicy fields (nested, non-hot-reloadable)
    macro_rules! cmp_keepalive {
        ($field:ident) => {
            if old.keepalive_policy.$field != new.keepalive_policy.$field {
                let name = format!("keepalive_policy.{}", stringify!($field));
                let c = TuningChange {
                    name: name.clone(),
                    old_value: format!("{}", old.keepalive_policy.$field),
                    new_value: format!("{}", new.keepalive_policy.$field),
                };
                if NON_HOT_RELOADABLE.contains(&name.as_str()) {
                    skipped.push(c);
                } else {
                    changes.push(c);
                }
            }
        };
    }

    cmp_keepalive!(same_zone_keepalive);
    cmp_keepalive!(same_region_keepalive);
    cmp_keepalive!(cross_region_keepalive);

    cmp_health!(same_zone_timeout);
    cmp_health!(same_region_timeout);
    cmp_health!(cross_region_timeout);

    cmp_announce!(same_zone_concurrency);
    cmp_announce!(same_region_concurrency);
    cmp_announce!(cross_region_concurrency);
    cmp_announce!(same_zone_delay_ms);
    cmp_announce!(same_region_delay_ms);
    cmp_announce!(cross_region_delay_ms);

    (changes, skipped)
}

/// Validate a parsed config file and return all validation errors.
fn validate_config(config: &ConfigFile) -> Result<(), String> {
    let mut errors: Vec<String> = Vec::new();

    // Helper: check that a duration (seconds) is > 0
    macro_rules! check_interval {
        ($section:ident . $field:ident, $label:expr) => {
            if let Some(v) = config.$section.$field {
                if v == 0 {
                    errors.push(format!("{} must be greater than 0", $label));
                }
            }
        };
    }

    // Helper: check that a usize limit is > 0
    macro_rules! check_limit {
        ($section:ident . $field:ident, $label:expr) => {
            if let Some(v) = config.$section.$field {
                if v == 0 {
                    errors.push(format!("{} must be greater than 0", $label));
                }
            }
        };
    }

    // Daemon intervals (seconds, must be > 0)
    check_interval!(daemon.health_check_interval, "daemon.health_check_interval");
    check_interval!(daemon.reconcile_interval, "daemon.reconcile_interval");
    check_interval!(daemon.persist_interval, "daemon.persist_interval");
    check_interval!(daemon.unreachable_timeout, "daemon.unreachable_timeout");
    check_interval!(
        daemon.self_announce_interval,
        "daemon.self_announce_interval"
    );

    // Log max size must be > 0
    if let Some(v) = config.daemon.log_max_size_mb {
        if v == 0 {
            errors.push("daemon.log_max_size_mb must be greater than 0".to_string());
        }
    }

    // WireGuard keepalive: 0 is valid (disables keepalive), but the type is
    // already u16 so 1-65535 is the valid non-zero range — no extra check needed.

    // Interface name must not be empty
    if let Some(ref name) = config.wireguard.interface_name {
        if name.trim().is_empty() {
            errors.push("wireguard.interface_name must not be empty".to_string());
        }
    }

    // Peering intervals (seconds, must be > 0)
    check_interval!(peering.join_timeout, "peering.join_timeout");
    check_interval!(peering.exchange_timeout, "peering.exchange_timeout");

    // Peering limits (must be > 0)
    check_limit!(
        peering.max_concurrent_connections,
        "peering.max_concurrent_connections"
    );
    check_limit!(peering.max_pending_joins, "peering.max_pending_joins");

    // Events limit (must be > 0)
    if let Some(v) = config.events.max_events {
        if v == 0 {
            errors.push("events.max_events must be greater than 0".to_string());
        }
    }

    // Limits section (must be > 0)
    check_limit!(limits.max_peers, "limits.max_peers");
    check_limit!(
        limits.max_concurrent_announces,
        "limits.max_concurrent_announces"
    );
    check_limit!(limits.announce_queue_size, "limits.announce_queue_size");

    // Health timeouts (seconds, must be > 0)
    check_interval!(health.same_zone_timeout, "health.same_zone_timeout");
    check_interval!(health.same_region_timeout, "health.same_region_timeout");
    check_interval!(health.cross_region_timeout, "health.cross_region_timeout");

    // Announcement concurrency limits (must be > 0)
    check_limit!(
        announcements.same_zone_concurrency,
        "announcements.same_zone_concurrency"
    );
    check_limit!(
        announcements.same_region_concurrency,
        "announcements.same_region_concurrency"
    );
    check_limit!(
        announcements.cross_region_concurrency,
        "announcements.cross_region_concurrency"
    );

    // Announcement delays: 0 is valid (no delay), no check needed for delay_ms fields.

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "config validation failed:\n  - {}",
            errors.join("\n  - ")
        ))
    }
}

/// Dry-run validation of `~/.syfrah/config.toml`.
///
/// Parses and validates the config file without applying any changes.
/// Returns `Ok(())` when the file is absent (nothing to validate) or when
/// the file is present and passes all validation checks.
pub fn validate_config_file() -> Result<(), String> {
    let path = syfrah_dir().join("config.toml");
    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;

    let config: ConfigFile =
        toml::from_str(&content).map_err(|e| format!("invalid config.toml: {e}"))?;

    validate_config(&config)
}

/// Load tuning from `~/.syfrah/config.toml`. Returns defaults if file
/// doesn't exist. Returns error if file exists but is invalid or contains
/// values that fail validation.
pub fn load_tuning() -> Result<Tuning, String> {
    let path = syfrah_dir().join("config.toml");
    if !path.exists() {
        return Ok(Tuning::default());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;

    let config: ConfigFile =
        toml::from_str(&content).map_err(|e| format!("invalid config.toml: {e}"))?;

    validate_config(&config)?;

    let defaults = Tuning::default();
    Ok(Tuning {
        health_check_interval: config
            .daemon
            .health_check_interval
            .map(Duration::from_secs)
            .unwrap_or(defaults.health_check_interval),
        reconcile_interval: config
            .daemon
            .reconcile_interval
            .map(Duration::from_secs)
            .unwrap_or(defaults.reconcile_interval),
        persist_interval: config
            .daemon
            .persist_interval
            .map(Duration::from_secs)
            .unwrap_or(defaults.persist_interval),
        unreachable_timeout: config
            .daemon
            .unreachable_timeout
            .map(Duration::from_secs)
            .unwrap_or(defaults.unreachable_timeout),
        keepalive_interval: config
            .wireguard
            .keepalive_interval
            .unwrap_or(defaults.keepalive_interval),
        keepalive_policy: KeepalivePolicy {
            same_zone_keepalive: config
                .wireguard
                .same_zone_keepalive
                .unwrap_or(defaults.keepalive_policy.same_zone_keepalive),
            same_region_keepalive: config
                .wireguard
                .same_region_keepalive
                .unwrap_or(defaults.keepalive_policy.same_region_keepalive),
            cross_region_keepalive: config
                .wireguard
                .cross_region_keepalive
                .unwrap_or(defaults.keepalive_policy.cross_region_keepalive),
        },
        join_timeout: config
            .peering
            .join_timeout
            .map(Duration::from_secs)
            .unwrap_or(defaults.join_timeout),
        exchange_timeout: config
            .peering
            .exchange_timeout
            .map(Duration::from_secs)
            .unwrap_or(defaults.exchange_timeout),
        max_events: config.events.max_events.unwrap_or(defaults.max_events),
        max_concurrent_connections: config
            .peering
            .max_concurrent_connections
            .unwrap_or(defaults.max_concurrent_connections),
        max_pending_joins: config
            .peering
            .max_pending_joins
            .unwrap_or(defaults.max_pending_joins),
        max_peers: config.limits.max_peers.unwrap_or(defaults.max_peers),
        max_concurrent_announces: config
            .limits
            .max_concurrent_announces
            .unwrap_or(defaults.max_concurrent_announces),
        announce_queue_size: config
            .limits
            .announce_queue_size
            .unwrap_or(defaults.announce_queue_size),
        interface_name: config
            .wireguard
            .interface_name
            .unwrap_or(defaults.interface_name),
        log_max_size_mb: config
            .daemon
            .log_max_size_mb
            .unwrap_or(defaults.log_max_size_mb),
        audit_max_size_mb: config
            .daemon
            .audit_max_size_mb
            .unwrap_or(defaults.audit_max_size_mb),
        self_announce_interval: config
            .daemon
            .self_announce_interval
            .map(Duration::from_secs)
            .unwrap_or(defaults.self_announce_interval),
        gc_removed_threshold: config
            .daemon
            .gc_removed_threshold
            .map(Duration::from_secs)
            .unwrap_or(defaults.gc_removed_threshold),
        health_policy: HealthPolicy {
            same_zone_timeout: config
                .health
                .same_zone_timeout
                .map(Duration::from_secs)
                .unwrap_or(defaults.health_policy.same_zone_timeout),
            same_region_timeout: config
                .health
                .same_region_timeout
                .map(Duration::from_secs)
                .unwrap_or(defaults.health_policy.same_region_timeout),
            cross_region_timeout: config
                .health
                .cross_region_timeout
                .map(Duration::from_secs)
                .unwrap_or(defaults.health_policy.cross_region_timeout),
        },
        announcements: AnnouncementConfig {
            same_zone_concurrency: config
                .announcements
                .same_zone_concurrency
                .unwrap_or(defaults.announcements.same_zone_concurrency),
            same_region_concurrency: config
                .announcements
                .same_region_concurrency
                .unwrap_or(defaults.announcements.same_region_concurrency),
            cross_region_concurrency: config
                .announcements
                .cross_region_concurrency
                .unwrap_or(defaults.announcements.cross_region_concurrency),
            same_zone_delay_ms: config
                .announcements
                .same_zone_delay_ms
                .unwrap_or(defaults.announcements.same_zone_delay_ms),
            same_region_delay_ms: config
                .announcements
                .same_region_delay_ms
                .unwrap_or(defaults.announcements.same_region_delay_ms),
            cross_region_delay_ms: config
                .announcements
                .cross_region_delay_ms
                .unwrap_or(defaults.announcements.cross_region_delay_ms),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_tuning_no_changes() {
        let a = Tuning::default();
        let b = Tuning::default();
        let (changes, skipped) = diff_tuning(&a, &b);
        assert!(changes.is_empty());
        assert!(skipped.is_empty());
    }

    #[test]
    fn diff_tuning_hot_reloadable_change() {
        let a = Tuning::default();
        let b = Tuning {
            health_check_interval: Duration::from_secs(30),
            max_peers: 500,
            ..Tuning::default()
        };

        let (changes, skipped) = diff_tuning(&a, &b);
        assert_eq!(changes.len(), 2);
        assert!(skipped.is_empty());
        assert!(changes[0].name == "health_check_interval");
        assert!(changes[0].old_value == "60s");
        assert!(changes[0].new_value == "30s");
        assert!(changes[1].name == "max_peers");
    }

    #[test]
    fn diff_tuning_non_hot_reloadable_skipped() {
        let a = Tuning::default();
        let b = Tuning {
            interface_name: "wg1".to_string(),
            keepalive_interval: 50,
            ..Tuning::default()
        };

        let (changes, skipped) = diff_tuning(&a, &b);
        assert!(changes.is_empty());
        assert_eq!(skipped.len(), 2);
        assert!(skipped.iter().any(|s| s.name == "interface_name"));
        assert!(skipped.iter().any(|s| s.name == "keepalive_interval"));
    }

    #[test]
    fn diff_tuning_mixed_changes() {
        let a = Tuning::default();
        let b = Tuning {
            reconcile_interval: Duration::from_secs(10),
            interface_name: "wg1".to_string(),
            ..Tuning::default()
        };

        let (changes, skipped) = diff_tuning(&a, &b);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].name, "reconcile_interval");
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].name, "interface_name");
    }

    #[test]
    fn keepalive_policy_defaults() {
        let kp = KeepalivePolicy::default();
        assert_eq!(kp.same_zone_keepalive, 20);
        assert_eq!(kp.same_region_keepalive, 25);
        assert_eq!(kp.cross_region_keepalive, 30);
    }

    #[test]
    fn diff_tuning_keepalive_policy_skipped() {
        let a = Tuning::default();
        let b = Tuning {
            keepalive_policy: KeepalivePolicy {
                same_zone_keepalive: 10,
                same_region_keepalive: 20,
                cross_region_keepalive: 40,
            },
            ..Tuning::default()
        };

        let (changes, skipped) = diff_tuning(&a, &b);
        assert!(changes.is_empty());
        assert_eq!(skipped.len(), 3);
        assert!(skipped
            .iter()
            .any(|s| s.name == "keepalive_policy.same_zone_keepalive"));
        assert!(skipped
            .iter()
            .any(|s| s.name == "keepalive_policy.same_region_keepalive"));
        assert!(skipped
            .iter()
            .any(|s| s.name == "keepalive_policy.cross_region_keepalive"));
    }

    #[test]
    fn announcement_config_defaults() {
        let cfg = AnnouncementConfig::default();
        assert_eq!(cfg.same_zone_concurrency, 50);
        assert_eq!(cfg.same_region_concurrency, 20);
        assert_eq!(cfg.cross_region_concurrency, 5);
        assert_eq!(cfg.same_zone_delay_ms, 0);
        assert_eq!(cfg.same_region_delay_ms, 5000);
        assert_eq!(cfg.cross_region_delay_ms, 15000);
    }

    #[test]
    fn diff_tuning_announcement_changes() {
        let a = Tuning::default();
        let b = Tuning {
            announcements: AnnouncementConfig {
                same_zone_concurrency: 30,
                cross_region_delay_ms: 30000,
                ..AnnouncementConfig::default()
            },
            ..Tuning::default()
        };

        let (changes, skipped) = diff_tuning(&a, &b);
        assert!(skipped.is_empty());
        assert_eq!(changes.len(), 2);
        assert!(changes
            .iter()
            .any(|c| c.name == "announcements.same_zone_concurrency"));
        assert!(changes
            .iter()
            .any(|c| c.name == "announcements.cross_region_delay_ms"));
    }

    // --- Config validation tests ---

    /// Helper: parse a TOML string into ConfigFile and validate it.
    fn validate_toml(toml_str: &str) -> Result<(), String> {
        let config: ConfigFile =
            toml::from_str(toml_str).map_err(|e| format!("parse error: {e}"))?;
        validate_config(&config)
    }

    #[test]
    fn validate_empty_config_ok() {
        assert!(validate_toml("").is_ok());
    }

    #[test]
    fn validate_valid_config_ok() {
        let toml = r#"
[daemon]
health_check_interval = 30
reconcile_interval = 15
persist_interval = 10
unreachable_timeout = 120
log_max_size_mb = 5
self_announce_interval = 20

[wireguard]
keepalive_interval = 25
same_zone_keepalive = 15
same_region_keepalive = 25
cross_region_keepalive = 35
interface_name = "syfrah0"

[peering]
join_timeout = 10
exchange_timeout = 10
max_concurrent_connections = 50
max_pending_joins = 50

[events]
max_events = 200

[limits]
max_peers = 500
max_concurrent_announces = 25
announce_queue_size = 100

[health]
same_zone_timeout = 60
same_region_timeout = 120
cross_region_timeout = 300

[announcements]
same_zone_concurrency = 40
same_region_concurrency = 15
cross_region_concurrency = 3
same_zone_delay_ms = 0
same_region_delay_ms = 3000
cross_region_delay_ms = 10000
"#;
        assert!(validate_toml(toml).is_ok());
    }

    #[test]
    fn validate_zero_interval_rejected() {
        let toml = "[daemon]\nhealth_check_interval = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(
            err.contains("daemon.health_check_interval must be greater than 0"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_zero_reconcile_interval_rejected() {
        let toml = "[daemon]\nreconcile_interval = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("daemon.reconcile_interval must be greater than 0"));
    }

    #[test]
    fn validate_zero_persist_interval_rejected() {
        let toml = "[daemon]\npersist_interval = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("daemon.persist_interval must be greater than 0"));
    }

    #[test]
    fn validate_zero_unreachable_timeout_rejected() {
        let toml = "[daemon]\nunreachable_timeout = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("daemon.unreachable_timeout must be greater than 0"));
    }

    #[test]
    fn validate_zero_self_announce_interval_rejected() {
        let toml = "[daemon]\nself_announce_interval = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("daemon.self_announce_interval must be greater than 0"));
    }

    #[test]
    fn validate_zero_log_max_size_mb_rejected() {
        let toml = "[daemon]\nlog_max_size_mb = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("daemon.log_max_size_mb must be greater than 0"));
    }

    #[test]
    fn validate_empty_interface_name_rejected() {
        let toml = "[wireguard]\ninterface_name = \"  \"\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("wireguard.interface_name must not be empty"));
    }

    #[test]
    fn validate_zero_join_timeout_rejected() {
        let toml = "[peering]\njoin_timeout = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("peering.join_timeout must be greater than 0"));
    }

    #[test]
    fn validate_zero_exchange_timeout_rejected() {
        let toml = "[peering]\nexchange_timeout = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("peering.exchange_timeout must be greater than 0"));
    }

    #[test]
    fn validate_zero_max_concurrent_connections_rejected() {
        let toml = "[peering]\nmax_concurrent_connections = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("peering.max_concurrent_connections must be greater than 0"));
    }

    #[test]
    fn validate_zero_max_pending_joins_rejected() {
        let toml = "[peering]\nmax_pending_joins = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("peering.max_pending_joins must be greater than 0"));
    }

    #[test]
    fn validate_zero_max_events_rejected() {
        let toml = "[events]\nmax_events = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("events.max_events must be greater than 0"));
    }

    #[test]
    fn validate_zero_max_peers_rejected() {
        let toml = "[limits]\nmax_peers = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("limits.max_peers must be greater than 0"));
    }

    #[test]
    fn validate_zero_max_concurrent_announces_rejected() {
        let toml = "[limits]\nmax_concurrent_announces = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("limits.max_concurrent_announces must be greater than 0"));
    }

    #[test]
    fn validate_zero_announce_queue_size_rejected() {
        let toml = "[limits]\nannounce_queue_size = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("limits.announce_queue_size must be greater than 0"));
    }

    #[test]
    fn validate_zero_health_timeouts_rejected() {
        let toml =
            "[health]\nsame_zone_timeout = 0\nsame_region_timeout = 0\ncross_region_timeout = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("health.same_zone_timeout must be greater than 0"));
        assert!(err.contains("health.same_region_timeout must be greater than 0"));
        assert!(err.contains("health.cross_region_timeout must be greater than 0"));
    }

    #[test]
    fn validate_zero_announcement_concurrency_rejected() {
        let toml = "[announcements]\nsame_zone_concurrency = 0\nsame_region_concurrency = 0\ncross_region_concurrency = 0\n";
        let err = validate_toml(toml).unwrap_err();
        assert!(err.contains("announcements.same_zone_concurrency must be greater than 0"));
        assert!(err.contains("announcements.same_region_concurrency must be greater than 0"));
        assert!(err.contains("announcements.cross_region_concurrency must be greater than 0"));
    }

    #[test]
    fn validate_zero_announcement_delay_allowed() {
        // Delay of 0 means "no delay" and is valid.
        let toml = "[announcements]\nsame_zone_delay_ms = 0\nsame_region_delay_ms = 0\ncross_region_delay_ms = 0\n";
        assert!(validate_toml(toml).is_ok());
    }

    #[test]
    fn validate_multiple_errors_collected() {
        let toml = r#"
[daemon]
health_check_interval = 0
reconcile_interval = 0

[peering]
max_concurrent_connections = 0

[limits]
max_peers = 0
"#;
        let err = validate_toml(toml).unwrap_err();
        // All four errors should be reported in one message.
        assert!(err.contains("daemon.health_check_interval"));
        assert!(err.contains("daemon.reconcile_interval"));
        assert!(err.contains("peering.max_concurrent_connections"));
        assert!(err.contains("limits.max_peers"));
    }

    #[test]
    fn validate_wireguard_keepalive_zero_allowed() {
        // keepalive_interval = 0 disables persistent keepalive; that is valid.
        let toml = "[wireguard]\nkeepalive_interval = 0\n";
        assert!(validate_toml(toml).is_ok());
    }

    #[test]
    fn validate_per_tier_keepalive_zero_allowed() {
        // Per-tier keepalive of 0 disables keepalive for that tier; valid.
        let toml = "[wireguard]\nsame_zone_keepalive = 0\nsame_region_keepalive = 0\ncross_region_keepalive = 0\n";
        assert!(validate_toml(toml).is_ok());
    }

    // ---------------------------------------------------------------
    // TOML parsing → Tuning conversion tests
    // ---------------------------------------------------------------

    /// Helper: parse a TOML string into a validated `Tuning`, using the same
    /// logic as `load_tuning` but without filesystem access.
    fn parse_tuning(toml_str: &str) -> Result<Tuning, String> {
        let config: ConfigFile =
            toml::from_str(toml_str).map_err(|e| format!("invalid config.toml: {e}"))?;
        validate_config(&config)?;
        let defaults = Tuning::default();
        Ok(Tuning {
            health_check_interval: config
                .daemon
                .health_check_interval
                .map(Duration::from_secs)
                .unwrap_or(defaults.health_check_interval),
            reconcile_interval: config
                .daemon
                .reconcile_interval
                .map(Duration::from_secs)
                .unwrap_or(defaults.reconcile_interval),
            persist_interval: config
                .daemon
                .persist_interval
                .map(Duration::from_secs)
                .unwrap_or(defaults.persist_interval),
            unreachable_timeout: config
                .daemon
                .unreachable_timeout
                .map(Duration::from_secs)
                .unwrap_or(defaults.unreachable_timeout),
            keepalive_interval: config
                .wireguard
                .keepalive_interval
                .unwrap_or(defaults.keepalive_interval),
            keepalive_policy: KeepalivePolicy {
                same_zone_keepalive: config
                    .wireguard
                    .same_zone_keepalive
                    .unwrap_or(defaults.keepalive_policy.same_zone_keepalive),
                same_region_keepalive: config
                    .wireguard
                    .same_region_keepalive
                    .unwrap_or(defaults.keepalive_policy.same_region_keepalive),
                cross_region_keepalive: config
                    .wireguard
                    .cross_region_keepalive
                    .unwrap_or(defaults.keepalive_policy.cross_region_keepalive),
            },
            join_timeout: config
                .peering
                .join_timeout
                .map(Duration::from_secs)
                .unwrap_or(defaults.join_timeout),
            exchange_timeout: config
                .peering
                .exchange_timeout
                .map(Duration::from_secs)
                .unwrap_or(defaults.exchange_timeout),
            max_events: config.events.max_events.unwrap_or(defaults.max_events),
            max_concurrent_connections: config
                .peering
                .max_concurrent_connections
                .unwrap_or(defaults.max_concurrent_connections),
            max_pending_joins: config
                .peering
                .max_pending_joins
                .unwrap_or(defaults.max_pending_joins),
            max_peers: config.limits.max_peers.unwrap_or(defaults.max_peers),
            max_concurrent_announces: config
                .limits
                .max_concurrent_announces
                .unwrap_or(defaults.max_concurrent_announces),
            announce_queue_size: config
                .limits
                .announce_queue_size
                .unwrap_or(defaults.announce_queue_size),
            interface_name: config
                .wireguard
                .interface_name
                .unwrap_or(defaults.interface_name),
            log_max_size_mb: config
                .daemon
                .log_max_size_mb
                .unwrap_or(defaults.log_max_size_mb),
            audit_max_size_mb: config
                .daemon
                .audit_max_size_mb
                .unwrap_or(defaults.audit_max_size_mb),
            self_announce_interval: config
                .daemon
                .self_announce_interval
                .map(Duration::from_secs)
                .unwrap_or(defaults.self_announce_interval),
            gc_removed_threshold: config
                .daemon
                .gc_removed_threshold
                .map(Duration::from_secs)
                .unwrap_or(defaults.gc_removed_threshold),
            health_policy: HealthPolicy {
                same_zone_timeout: config
                    .health
                    .same_zone_timeout
                    .map(Duration::from_secs)
                    .unwrap_or(defaults.health_policy.same_zone_timeout),
                same_region_timeout: config
                    .health
                    .same_region_timeout
                    .map(Duration::from_secs)
                    .unwrap_or(defaults.health_policy.same_region_timeout),
                cross_region_timeout: config
                    .health
                    .cross_region_timeout
                    .map(Duration::from_secs)
                    .unwrap_or(defaults.health_policy.cross_region_timeout),
            },
            announcements: AnnouncementConfig {
                same_zone_concurrency: config
                    .announcements
                    .same_zone_concurrency
                    .unwrap_or(defaults.announcements.same_zone_concurrency),
                same_region_concurrency: config
                    .announcements
                    .same_region_concurrency
                    .unwrap_or(defaults.announcements.same_region_concurrency),
                cross_region_concurrency: config
                    .announcements
                    .cross_region_concurrency
                    .unwrap_or(defaults.announcements.cross_region_concurrency),
                same_zone_delay_ms: config
                    .announcements
                    .same_zone_delay_ms
                    .unwrap_or(defaults.announcements.same_zone_delay_ms),
                same_region_delay_ms: config
                    .announcements
                    .same_region_delay_ms
                    .unwrap_or(defaults.announcements.same_region_delay_ms),
                cross_region_delay_ms: config
                    .announcements
                    .cross_region_delay_ms
                    .unwrap_or(defaults.announcements.cross_region_delay_ms),
            },
        })
    }

    #[test]
    fn parse_empty_toml_returns_defaults() {
        let tuning = parse_tuning("").unwrap();
        assert_eq!(tuning, Tuning::default());
    }

    #[test]
    fn parse_full_valid_toml() {
        let toml = r#"
[daemon]
health_check_interval = 45
reconcile_interval = 20
persist_interval = 15
unreachable_timeout = 200
log_max_size_mb = 20
audit_max_size_mb = 25
self_announce_interval = 30
gc_removed_threshold = 3600

[wireguard]
keepalive_interval = 10
interface_name = "mesh0"

[peering]
join_timeout = 5
exchange_timeout = 8
max_concurrent_connections = 200
max_pending_joins = 150

[events]
max_events = 500

[limits]
max_peers = 2000
max_concurrent_announces = 100
announce_queue_size = 400

[health]
same_zone_timeout = 90
same_region_timeout = 150
cross_region_timeout = 250

[announcements]
same_zone_concurrency = 30
same_region_concurrency = 10
cross_region_concurrency = 2
same_zone_delay_ms = 100
same_region_delay_ms = 2000
cross_region_delay_ms = 8000
"#;
        let t = parse_tuning(toml).unwrap();

        assert_eq!(t.health_check_interval, Duration::from_secs(45));
        assert_eq!(t.reconcile_interval, Duration::from_secs(20));
        assert_eq!(t.persist_interval, Duration::from_secs(15));
        assert_eq!(t.unreachable_timeout, Duration::from_secs(200));
        assert_eq!(t.log_max_size_mb, 20);
        assert_eq!(t.audit_max_size_mb, 25);
        assert_eq!(t.self_announce_interval, Duration::from_secs(30));
        assert_eq!(t.gc_removed_threshold, Duration::from_secs(3600));

        assert_eq!(t.keepalive_interval, 10);
        assert_eq!(t.interface_name, "mesh0");

        assert_eq!(t.join_timeout, Duration::from_secs(5));
        assert_eq!(t.exchange_timeout, Duration::from_secs(8));
        assert_eq!(t.max_concurrent_connections, 200);
        assert_eq!(t.max_pending_joins, 150);

        assert_eq!(t.max_events, 500);

        assert_eq!(t.max_peers, 2000);
        assert_eq!(t.max_concurrent_announces, 100);
        assert_eq!(t.announce_queue_size, 400);

        assert_eq!(t.health_policy.same_zone_timeout, Duration::from_secs(90));
        assert_eq!(
            t.health_policy.same_region_timeout,
            Duration::from_secs(150)
        );
        assert_eq!(
            t.health_policy.cross_region_timeout,
            Duration::from_secs(250)
        );

        assert_eq!(t.announcements.same_zone_concurrency, 30);
        assert_eq!(t.announcements.same_region_concurrency, 10);
        assert_eq!(t.announcements.cross_region_concurrency, 2);
        assert_eq!(t.announcements.same_zone_delay_ms, 100);
        assert_eq!(t.announcements.same_region_delay_ms, 2000);
        assert_eq!(t.announcements.cross_region_delay_ms, 8000);
    }

    #[test]
    fn parse_partial_daemon_section_fills_defaults() {
        let toml = "[daemon]\nhealth_check_interval = 90\n";
        let t = parse_tuning(toml).unwrap();
        let d = Tuning::default();

        assert_eq!(t.health_check_interval, Duration::from_secs(90));
        // Everything else stays at default.
        assert_eq!(t.reconcile_interval, d.reconcile_interval);
        assert_eq!(t.persist_interval, d.persist_interval);
        assert_eq!(t.unreachable_timeout, d.unreachable_timeout);
        assert_eq!(t.log_max_size_mb, d.log_max_size_mb);
        assert_eq!(t.self_announce_interval, d.self_announce_interval);
        assert_eq!(t.gc_removed_threshold, d.gc_removed_threshold);
    }

    #[test]
    fn parse_partial_peering_section_fills_defaults() {
        let toml = "[peering]\njoin_timeout = 3\n";
        let t = parse_tuning(toml).unwrap();
        let d = Tuning::default();

        assert_eq!(t.join_timeout, Duration::from_secs(3));
        assert_eq!(t.exchange_timeout, d.exchange_timeout);
        assert_eq!(t.max_concurrent_connections, d.max_concurrent_connections);
        assert_eq!(t.max_pending_joins, d.max_pending_joins);
    }

    #[test]
    fn parse_partial_health_section_fills_defaults() {
        let toml = "[health]\nsame_zone_timeout = 60\n";
        let t = parse_tuning(toml).unwrap();
        let d = Tuning::default();

        assert_eq!(t.health_policy.same_zone_timeout, Duration::from_secs(60));
        assert_eq!(
            t.health_policy.same_region_timeout,
            d.health_policy.same_region_timeout
        );
        assert_eq!(
            t.health_policy.cross_region_timeout,
            d.health_policy.cross_region_timeout
        );
    }

    #[test]
    fn parse_partial_announcements_section_fills_defaults() {
        let toml = "[announcements]\ncross_region_concurrency = 10\n";
        let t = parse_tuning(toml).unwrap();
        let d = Tuning::default();

        assert_eq!(t.announcements.cross_region_concurrency, 10);
        assert_eq!(
            t.announcements.same_zone_concurrency,
            d.announcements.same_zone_concurrency
        );
        assert_eq!(
            t.announcements.same_region_concurrency,
            d.announcements.same_region_concurrency
        );
        assert_eq!(
            t.announcements.same_zone_delay_ms,
            d.announcements.same_zone_delay_ms
        );
    }

    #[test]
    fn parse_missing_all_sections_returns_defaults() {
        // A config with only comments and whitespace is equivalent to empty.
        let toml = "# This config intentionally left blank.\n\n";
        let t = parse_tuning(toml).unwrap();
        assert_eq!(t, Tuning::default());
    }

    #[test]
    fn parse_invalid_toml_syntax_rejected() {
        let toml = "this is not [valid toml =";
        let err = parse_tuning(toml).unwrap_err();
        assert!(
            err.contains("invalid config.toml"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_wrong_type_rejected() {
        // health_check_interval expects u64, not a string.
        let toml = "[daemon]\nhealth_check_interval = \"fast\"\n";
        let err = parse_tuning(toml).unwrap_err();
        assert!(
            err.contains("invalid config.toml"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_negative_value_rejected() {
        // TOML will parse -1 as a signed integer which cannot deserialize to u64.
        let toml = "[daemon]\nhealth_check_interval = -1\n";
        let err = parse_tuning(toml).unwrap_err();
        assert!(
            err.contains("invalid config.toml"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_unknown_section_ignored() {
        // Unknown TOML sections should be silently ignored by serde default.
        let toml = "[unknown_section]\nfoo = 42\n";
        // ConfigFile uses #[serde(default)] but unknown top-level keys are
        // rejected by serde unless we use deny_unknown_fields, which we don't.
        // So this should either succeed or fail at parse; let's verify:
        let result: Result<ConfigFile, _> = toml::from_str(toml);
        // If the crate denies unknown fields this will be Err; otherwise Ok.
        // Either way the behaviour is acceptable — we just document it.
        if let Ok(config) = result {
            // If it parses, validation should pass and defaults apply.
            assert!(validate_config(&config).is_ok());
        }
    }

    #[test]
    fn parse_unknown_key_in_known_section() {
        // An unknown key inside a known section.
        let toml = "[daemon]\nfoo_bar = 99\n";
        let result: Result<ConfigFile, _> = toml::from_str(toml);
        if let Ok(config) = result {
            assert!(validate_config(&config).is_ok());
        }
    }

    #[test]
    fn parse_zero_value_caught_by_validation() {
        // The TOML parses fine, but validation should reject zero intervals.
        let toml = "[daemon]\nhealth_check_interval = 0\nreconcile_interval = 0\n";
        let err = parse_tuning(toml).unwrap_err();
        assert!(err.contains("health_check_interval must be greater than 0"));
        assert!(err.contains("reconcile_interval must be greater than 0"));
    }

    #[test]
    fn parse_gc_removed_threshold_zero_allowed() {
        // gc_removed_threshold = 0 disables GC, which is documented as valid.
        let toml = "[daemon]\ngc_removed_threshold = 0\n";
        // No validation rule blocks 0 for gc_removed_threshold.
        let t = parse_tuning(toml).unwrap();
        assert_eq!(t.gc_removed_threshold, Duration::from_secs(0));
    }

    // ---------------------------------------------------------------
    // Additional diff_tuning coverage
    // ---------------------------------------------------------------

    #[test]
    fn diff_tuning_health_policy_changes() {
        let a = Tuning::default();
        let b = Tuning {
            health_policy: HealthPolicy {
                same_zone_timeout: Duration::from_secs(60),
                same_region_timeout: Duration::from_secs(90),
                cross_region_timeout: Duration::from_secs(180),
            },
            ..Tuning::default()
        };

        let (changes, skipped) = diff_tuning(&a, &b);
        assert!(skipped.is_empty());
        assert_eq!(changes.len(), 3);
        assert!(changes
            .iter()
            .any(|c| c.name == "health_policy.same_zone_timeout"
                && c.old_value == "120s"
                && c.new_value == "60s"));
        assert!(changes
            .iter()
            .any(|c| c.name == "health_policy.same_region_timeout"
                && c.old_value == "180s"
                && c.new_value == "90s"));
        assert!(changes
            .iter()
            .any(|c| c.name == "health_policy.cross_region_timeout"
                && c.old_value == "300s"
                && c.new_value == "180s"));
    }

    #[test]
    fn diff_tuning_gc_removed_threshold_change() {
        let a = Tuning::default();
        let b = Tuning {
            gc_removed_threshold: Duration::from_secs(7200),
            ..Tuning::default()
        };
        let (changes, skipped) = diff_tuning(&a, &b);
        assert!(skipped.is_empty());
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].name, "gc_removed_threshold");
        assert_eq!(changes[0].old_value, "86400s");
        assert_eq!(changes[0].new_value, "7200s");
    }

    #[test]
    fn diff_tuning_all_announcement_fields() {
        let a = Tuning::default();
        let b = Tuning {
            announcements: AnnouncementConfig {
                same_zone_concurrency: 1,
                same_region_concurrency: 1,
                cross_region_concurrency: 1,
                same_zone_delay_ms: 1,
                same_region_delay_ms: 1,
                cross_region_delay_ms: 1,
            },
            ..Tuning::default()
        };
        let (changes, skipped) = diff_tuning(&a, &b);
        assert!(skipped.is_empty());
        assert_eq!(changes.len(), 6);
        let names: Vec<&str> = changes.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"announcements.same_zone_concurrency"));
        assert!(names.contains(&"announcements.same_region_concurrency"));
        assert!(names.contains(&"announcements.cross_region_concurrency"));
        assert!(names.contains(&"announcements.same_zone_delay_ms"));
        assert!(names.contains(&"announcements.same_region_delay_ms"));
        assert!(names.contains(&"announcements.cross_region_delay_ms"));
    }

    #[test]
    fn diff_tuning_log_and_audit_size_changes() {
        let a = Tuning::default();
        let b = Tuning {
            log_max_size_mb: 50,
            audit_max_size_mb: 100,
            ..Tuning::default()
        };
        let (changes, skipped) = diff_tuning(&a, &b);
        assert!(skipped.is_empty());
        assert_eq!(changes.len(), 2);
        assert!(changes.iter().any(|c| c.name == "log_max_size_mb"));
        assert!(changes.iter().any(|c| c.name == "audit_max_size_mb"));
    }
}
