use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

fn syfrah_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".syfrah")
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
    /// Interval between periodic self-announce rounds (anti-entropy).
    /// Each round re-announces this node to a gossip subset of known peers,
    /// ensuring convergence even when initial announcements fail (default 10s).
    pub self_announce_interval: Duration,
    /// Wave-based announce propagation settings.
    pub announcements: AnnouncementConfig,
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
            self_announce_interval: Duration::from_secs(10),
            announcements: AnnouncementConfig::default(),
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
    self_announce_interval: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct WireguardSection {
    keepalive_interval: Option<u16>,
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
const NON_HOT_RELOADABLE: &[&str] = &["interface_name", "keepalive_interval"];

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
    cmp_dur!(self_announce_interval);

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

/// Load tuning from `~/.syfrah/config.toml`. Returns defaults if file
/// doesn't exist. Returns error only if file exists but is invalid.
pub fn load_tuning() -> Result<Tuning, String> {
    let path = syfrah_dir().join("config.toml");
    if !path.exists() {
        return Ok(Tuning::default());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;

    let config: ConfigFile =
        toml::from_str(&content).map_err(|e| format!("invalid config.toml: {e}"))?;

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
        self_announce_interval: config
            .daemon
            .self_announce_interval
            .map(Duration::from_secs)
            .unwrap_or(defaults.self_announce_interval),
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
}
