use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

fn syfrah_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".syfrah")
}

/// Daemon tuning parameters. All fields are optional — defaults match the
/// original hardcoded values.
#[derive(Debug, Clone)]
pub struct Tuning {
    pub health_check_interval: Duration,
    pub reconcile_interval: Duration,
    pub persist_interval: Duration,
    pub unreachable_timeout: Duration,
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
    /// WireGuard interface name (default "syfrah0").
    pub interface_name: String,
    /// Maximum log file size in megabytes before rotation (default 10).
    pub log_max_size_mb: u64,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            health_check_interval: Duration::from_secs(60),
            reconcile_interval: Duration::from_secs(30),
            persist_interval: Duration::from_secs(30),
            unreachable_timeout: Duration::from_secs(300),
            keepalive_interval: 25,
            join_timeout: Duration::from_secs(10),
            exchange_timeout: Duration::from_secs(10),
            max_events: 100,
            max_concurrent_connections: 100,
            max_pending_joins: 100,
            max_peers: 1000,
            max_concurrent_announces: 50,
            interface_name: crate::wg::DEFAULT_INTERFACE_NAME.to_string(),
            log_max_size_mb: 10,
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
}

#[derive(Debug, Deserialize, Default)]
struct DaemonSection {
    health_check_interval: Option<u64>,
    reconcile_interval: Option<u64>,
    persist_interval: Option<u64>,
    unreachable_timeout: Option<u64>,
    log_max_size_mb: Option<u64>,
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
        interface_name: config
            .wireguard
            .interface_name
            .unwrap_or(defaults.interface_name),
        log_max_size_mb: config
            .daemon
            .log_max_size_mb
            .unwrap_or(defaults.log_max_size_mb),
    })
}
