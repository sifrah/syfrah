use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::sanitize::sanitize;
use crate::{no_mesh_error, store, ui, wg};
use anyhow::Result;
use syfrah_core::mesh::{PeerRecord, PeerStatus};

pub async fn run() -> Result<()> {
    let state = store::load().map_err(|_| no_mesh_error())?;

    if state.peers.is_empty() {
        ui::info_line("Peers", "No peers discovered yet.");
        return Ok(());
    }

    // Dedup peers by WG public key, keeping the entry with the latest last_seen
    let peers = dedup_peers(&state.peers);

    // Try to get live WG stats
    let wg_summary = wg::interface_summary().ok();

    ui::heading(&format!(
        "{:<18} {:<12} {:<14} {:<24} {:>8} {:<14} {:>12} {:>14}",
        "NAME", "REGION", "ZONE", "ENDPOINT", "STATUS", "SINCE", "HANDSHAKE", "TRAFFIC"
    ));

    for peer in &peers {
        let status = match peer.status {
            PeerStatus::Active => "active",
            PeerStatus::Unreachable => "unreach",
            PeerStatus::Removed => "removed",
        };

        // Find live WG stats for this peer
        let (handshake_str, traffic_str) = if let Some(ref summary) = wg_summary {
            match summary
                .peers
                .iter()
                .find(|p| p.public_key == peer.wg_public_key)
            {
                Some(wg_peer) => {
                    let hs = wg_peer
                        .last_handshake
                        .map(format_ago)
                        .unwrap_or_else(|| "never".into());
                    let traffic = format_traffic(wg_peer.rx_bytes, wg_peer.tx_bytes);
                    (hs, traffic)
                }
                None => ("n/a".into(), "-".into()),
            }
        } else {
            ("-".into(), "-".into())
        };

        let region = peer
            .region
            .as_deref()
            .map(sanitize)
            .unwrap_or_else(|| "-".into());
        let zone = peer
            .zone
            .as_deref()
            .map(sanitize)
            .unwrap_or_else(|| "-".into());

        let since_str = format_since(peer.last_seen);

        println!(
            "{:<18} {:<12} {:<14} {:<24} {:>8} {:<14} {:>12} {:>14}",
            truncate(&sanitize(&peer.name), 17),
            truncate(&region, 11),
            truncate(&zone, 13),
            peer.endpoint,
            status,
            since_str,
            handshake_str,
            traffic_str,
        );
    }

    Ok(())
}

/// Deduplicate peers by WireGuard public key, keeping the entry with the
/// latest `last_seen` timestamp. Returns a sorted vec (by name then key).
fn dedup_peers(peers: &[PeerRecord]) -> Vec<PeerRecord> {
    let mut by_key: HashMap<&str, &PeerRecord> = HashMap::new();
    for peer in peers {
        by_key
            .entry(peer.wg_public_key.as_str())
            .and_modify(|existing| {
                if peer.last_seen > existing.last_seen {
                    *existing = peer;
                }
            })
            .or_insert(peer);
    }
    let mut result: Vec<PeerRecord> = by_key.into_values().cloned().collect();
    result.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.wg_public_key.cmp(&b.wg_public_key))
    });
    result
}

/// Format a `last_seen` epoch-seconds timestamp as a human-readable duration
/// (e.g. "2h 15m ago"). Returns "-" if the timestamp is zero.
fn format_since(epoch_secs: u64) -> String {
    if epoch_secs == 0 {
        return "-".into();
    }

    let seen_time = UNIX_EPOCH + Duration::from_secs(epoch_secs);
    let elapsed = SystemTime::now()
        .duration_since(seen_time)
        .unwrap_or_default()
        .as_secs();

    format_duration(elapsed)
}

fn format_ago(time: SystemTime) -> String {
    if time == UNIX_EPOCH {
        return "never".into();
    }

    let elapsed = SystemTime::now()
        .duration_since(time)
        .unwrap_or_default()
        .as_secs();

    format_duration(elapsed)
}

/// Format seconds into a human-readable relative duration string.
fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        let m = secs / 60;
        let s = secs % 60;
        if s == 0 {
            format!("{m}m ago")
        } else {
            format!("{m}m {s}s ago")
        }
    } else if secs < 86400 {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        if m == 0 {
            format!("{h}h ago")
        } else {
            format!("{h}h {m}m ago")
        }
    } else {
        let d = secs / 86400;
        let h = (secs % 86400) / 3600;
        if h == 0 {
            format!("{d}d ago")
        } else {
            format!("{d}d {h}h ago")
        }
    }
}

fn format_traffic(rx: u64, tx: u64) -> String {
    if rx == 0 && tx == 0 {
        return "-".into();
    }
    format!("{}↓ {}↑", format_short(rx), format_short(tx))
}

fn format_short(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{}K", bytes / 1024)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{}M", bytes / (1024 * 1024))
    } else {
        format!("{}G", bytes / (1024 * 1024 * 1024))
    }
}

use super::ui::truncate;

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv6Addr;

    fn make_peer(name: &str, key: &str, last_seen: u64) -> PeerRecord {
        PeerRecord {
            name: name.into(),
            wg_public_key: key.into(),
            endpoint: "127.0.0.1:51820".parse().unwrap(),
            mesh_ipv6: Ipv6Addr::new(0xfd12, 0, 0, 0, 0, 0, 0, 1),
            last_seen,
            status: PeerStatus::Active,
            region: None,
            zone: None,
        }
    }

    #[test]
    fn dedup_keeps_latest_last_seen() {
        let peers = vec![
            make_peer("node-a", "key-1", 100),
            make_peer("node-a", "key-1", 200),
            make_peer("node-a", "key-1", 150),
        ];
        let result = dedup_peers(&peers);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].last_seen, 200);
    }

    #[test]
    fn dedup_preserves_distinct_keys() {
        let peers = vec![
            make_peer("node-a", "key-1", 100),
            make_peer("node-b", "key-2", 200),
        ];
        let result = dedup_peers(&peers);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn dedup_empty() {
        let result = dedup_peers(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(0), "0s ago");
        assert_eq!(format_duration(45), "45s ago");
        assert_eq!(format_duration(59), "59s ago");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(60), "1m ago");
        assert_eq!(format_duration(90), "1m 30s ago");
        assert_eq!(format_duration(3599), "59m 59s ago");
    }

    #[test]
    fn format_duration_hours() {
        assert_eq!(format_duration(3600), "1h ago");
        assert_eq!(format_duration(8100), "2h 15m ago");
        assert_eq!(format_duration(86399), "23h 59m ago");
    }

    #[test]
    fn format_duration_days() {
        assert_eq!(format_duration(86400), "1d ago");
        assert_eq!(format_duration(90000), "1d 1h ago");
    }

    #[test]
    fn format_since_zero_returns_dash() {
        assert_eq!(format_since(0), "-");
    }

    #[test]
    fn format_short_bytes() {
        assert_eq!(format_short(0), "0B");
        assert_eq!(format_short(512), "512B");
        assert_eq!(format_short(1024), "1K");
        assert_eq!(format_short(1048576), "1M");
        assert_eq!(format_short(1073741824), "1G");
    }

    #[test]
    fn format_traffic_zero() {
        assert_eq!(format_traffic(0, 0), "-");
    }

    #[test]
    fn format_traffic_nonzero() {
        assert_eq!(format_traffic(1200, 3400), "1K↓ 3K↑");
    }
}
