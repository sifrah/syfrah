//! Prometheus text format export for fabric metrics.
//!
//! Reads counters and gauges from the store and formats them according to
//! the [Prometheus exposition format](https://prometheus.io/docs/instrumenting/exposition_formats/).

use crate::store;

/// All metric definitions. Each entry maps a store key to its Prometheus
/// name, HELP string, and type (counter or gauge).
const METRIC_DEFS: &[MetricDef] = &[
    MetricDef {
        store_key: "peers_discovered",
        prom_name: "syfrah_peers_discovered_total",
        help: "Total number of peers discovered since daemon start",
        kind: MetricKind::Counter,
    },
    MetricDef {
        store_key: "wg_reconciliations",
        prom_name: "syfrah_wg_reconciliations_total",
        help: "Total WireGuard reconciliation cycles executed",
        kind: MetricKind::Counter,
    },
    MetricDef {
        store_key: "peers_marked_unreachable",
        prom_name: "syfrah_peers_marked_unreachable_total",
        help: "Total number of times a peer was marked unreachable",
        kind: MetricKind::Counter,
    },
    MetricDef {
        store_key: "announcements_failed",
        prom_name: "syfrah_announcements_failed_total",
        help: "Total peer announcement failures",
        kind: MetricKind::Counter,
    },
    MetricDef {
        store_key: "daemon_started_at",
        prom_name: "syfrah_daemon_started_at",
        help: "Unix timestamp when the daemon was started",
        kind: MetricKind::Gauge,
    },
    MetricDef {
        store_key: "announces_dropped",
        prom_name: "syfrah_announces_dropped_total",
        help: "Total announce messages dropped due to back-pressure",
        kind: MetricKind::Counter,
    },
    MetricDef {
        store_key: "announces_queued",
        prom_name: "syfrah_announces_queued_total",
        help: "Total announce messages queued for retry",
        kind: MetricKind::Counter,
    },
    MetricDef {
        store_key: "announces_queue_full",
        prom_name: "syfrah_announces_queue_full_total",
        help: "Total announce messages dropped because the retry queue was full",
        kind: MetricKind::Counter,
    },
    MetricDef {
        store_key: "peer_limit_reached",
        prom_name: "syfrah_peer_limit_reached_total",
        help: "Total times a new peer was rejected because the peer limit was reached",
        kind: MetricKind::Counter,
    },
    MetricDef {
        store_key: "connections_rejected",
        prom_name: "syfrah_connections_rejected_total",
        help: "Total peering connections rejected",
        kind: MetricKind::Counter,
    },
    MetricDef {
        store_key: "connections_active",
        prom_name: "syfrah_connections_active",
        help: "Number of currently active peering connections",
        kind: MetricKind::Gauge,
    },
    MetricDef {
        store_key: "health_check_failures",
        prom_name: "syfrah_health_check_failures_total",
        help: "Total health check failures",
        kind: MetricKind::Counter,
    },
    MetricDef {
        store_key: "reconcile_failures",
        prom_name: "syfrah_reconcile_failures_total",
        help: "Total WireGuard reconcile failures",
        kind: MetricKind::Counter,
    },
    MetricDef {
        store_key: "store_failures",
        prom_name: "syfrah_store_failures_total",
        help: "Total state store persistence failures",
        kind: MetricKind::Counter,
    },
];

struct MetricDef {
    store_key: &'static str,
    prom_name: &'static str,
    help: &'static str,
    kind: MetricKind,
}

#[derive(Clone, Copy)]
enum MetricKind {
    Counter,
    Gauge,
}

impl MetricKind {
    fn as_str(self) -> &'static str {
        match self {
            MetricKind::Counter => "counter",
            MetricKind::Gauge => "gauge",
        }
    }
}

/// Render all known metrics in Prometheus text exposition format.
///
/// Reads each metric from the store; metrics that fail to load are silently
/// skipped (the store may not exist yet if the daemon has never run).
pub fn render_prometheus() -> String {
    let mut out = String::with_capacity(2048);

    // Peer count from store (derived metric)
    let peer_count = store::peer_count().unwrap_or(0) as u64;
    out.push_str("# HELP syfrah_peers_count Current number of peers in the mesh\n");
    out.push_str("# TYPE syfrah_peers_count gauge\n");
    out.push_str(&format!("syfrah_peers_count {peer_count}\n"));

    // All store-backed metrics
    for def in METRIC_DEFS {
        // Try loading from redb via the store helpers. If the store doesn't
        // exist yet we get an error — just skip the metric.
        let value = match load_metric(def.store_key) {
            Some(v) => v,
            None => continue,
        };

        out.push_str(&format!("# HELP {} {}\n", def.prom_name, def.help));
        out.push_str(&format!("# TYPE {} {}\n", def.prom_name, def.kind.as_str()));
        out.push_str(&format!("{} {value}\n", def.prom_name));
    }

    out
}

/// Load a single metric value from the store. Returns `None` when the store
/// is unavailable (e.g. no mesh configured yet).
fn load_metric(key: &str) -> Option<u64> {
    // `store::load()` gives us the full `NodeState` which contains `Metrics`,
    // but those only cover a subset. Instead we read directly from redb via
    // `inc_metric` with delta 0 — this returns the current value without
    // mutating it.
    store::inc_metric(key, 0).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_defs_have_unique_prom_names() {
        let mut names: Vec<&str> = METRIC_DEFS.iter().map(|d| d.prom_name).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(
            before,
            names.len(),
            "duplicate Prometheus metric names found"
        );
    }

    #[test]
    fn metric_defs_have_unique_store_keys() {
        let mut keys: Vec<&str> = METRIC_DEFS.iter().map(|d| d.store_key).collect();
        keys.sort();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "duplicate store keys found");
    }

    #[test]
    fn counter_names_end_with_total() {
        for def in METRIC_DEFS {
            if matches!(def.kind, MetricKind::Counter) {
                assert!(
                    def.prom_name.ends_with("_total"),
                    "counter {} should end with _total",
                    def.prom_name
                );
            }
        }
    }

    #[test]
    fn gauge_names_do_not_end_with_total() {
        for def in METRIC_DEFS {
            if matches!(def.kind, MetricKind::Gauge) {
                assert!(
                    !def.prom_name.ends_with("_total"),
                    "gauge {} should not end with _total",
                    def.prom_name
                );
            }
        }
    }

    #[test]
    fn render_prometheus_does_not_panic_without_store() {
        // When no store exists, render should return at least the peer count
        // line (with 0) and not panic.
        let output = render_prometheus();
        assert!(output.contains("syfrah_peers_count"));
    }
}
