use std::collections::HashMap;
use std::time::SystemTime;

use crate::sanitize::sanitize;
use crate::{store, wg};
use anyhow::Result;
use syfrah_core::mesh::PeerRecord;
use syfrah_core::mesh::PeerStatus;

pub async fn run() -> Result<()> {
    let state = store::load().map_err(|_| {
        anyhow::anyhow!(
            "no mesh configured. Run 'syfrah fabric init' or 'syfrah fabric join' first."
        )
    })?;

    if state.peers.is_empty() {
        println!("No peers discovered yet.");
        return Ok(());
    }

    // Deduplicate peers by WireGuard public key, keeping the most recent entry
    let peers = dedup_peers(&state.peers);

    if peers.is_empty() {
        println!("No peers discovered yet.");
        return Ok(());
    }

    // Try to get live WG stats
    let wg_summary = wg::interface_summary().ok();

    println!(
        "{:<18} {:<10} {:<12} {:<24} {:>8} {:>12} {:>12} {:>12}",
        "NAME", "REGION", "ZONE", "ENDPOINT", "STATUS", "SINCE", "HANDSHAKE", "TRAFFIC"
    );
    println!("{}", "-".repeat(120));

    for peer in &peers {
        let status = match peer.status {
            PeerStatus::Active => "active",
            PeerStatus::Unreachable => "unreach",
            PeerStatus::Removed => "removed",
        };

        // Compute SINCE column from last_seen epoch timestamp
        let since_str = format_since(peer.last_seen);

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

        println!(
            "{:<18} {:<10} {:<12} {:<24} {:>8} {:>12} {:>12} {:>12}",
            truncate(&sanitize(&peer.name), 17),
            truncate(&region, 9),
            truncate(&zone, 11),
            peer.endpoint,
            status,
            since_str,
            handshake_str,
            traffic_str,
        );
    }

    Ok(())
}

/// Deduplicate peers by WireGuard public key.
/// When duplicates exist, keep the entry with the highest `last_seen` timestamp.
fn dedup_peers(peers: &[PeerRecord]) -> Vec<PeerRecord> {
    let mut best: HashMap<&str, &PeerRecord> = HashMap::new();
    for peer in peers {
        let entry = best.entry(peer.wg_public_key.as_str()).or_insert(peer);
        if peer.last_seen > entry.last_seen {
            *entry = peer;
        }
    }
    let mut result: Vec<PeerRecord> = best.into_values().cloned().collect();
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

/// Format a `last_seen` epoch timestamp as a human-readable relative duration.
/// Returns "-" if the timestamp is 0 (never seen).
fn format_since(epoch_secs: u64) -> String {
    if epoch_secs == 0 {
        return "-".into();
    }
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if epoch_secs > now {
        return "just now".into();
    }
    let elapsed = now - epoch_secs;
    format_duration(elapsed)
}

fn format_ago(time: SystemTime) -> String {
    if time == std::time::UNIX_EPOCH {
        return "never".into();
    }

    let elapsed = SystemTime::now()
        .duration_since(time)
        .unwrap_or_default()
        .as_secs();

    format_duration(elapsed)
}

/// Format a duration in seconds as a human-readable string.
/// Examples: "12s ago", "5m ago", "2h 15m ago", "3d 4h ago".
fn format_duration(elapsed: u64) -> String {
    if elapsed < 60 {
        format!("{elapsed}s ago")
    } else if elapsed < 3600 {
        let m = elapsed / 60;
        let s = elapsed % 60;
        if s == 0 {
            format!("{m}m ago")
        } else {
            format!("{m}m {s}s ago")
        }
    } else if elapsed < 86400 {
        let h = elapsed / 3600;
        let m = (elapsed % 3600) / 60;
        if m == 0 {
            format!("{h}h ago")
        } else {
            format!("{h}h {m}m ago")
        }
    } else {
        let d = elapsed / 86400;
        let h = (elapsed % 86400) / 3600;
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

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv6Addr;

    fn make_peer(name: &str, key: &str, last_seen: u64, status: PeerStatus) -> PeerRecord {
        PeerRecord {
            name: name.into(),
            wg_public_key: key.into(),
            endpoint: "203.0.113.1:51820".parse().unwrap(),
            mesh_ipv6: Ipv6Addr::new(0xfd12, 0x3456, 0x7800, 0, 0, 0, 0, 1),
            last_seen,
            status,
            region: Some("eu-north".into()),
            zone: Some("zone-1".into()),
        }
    }

    #[test]
    fn dedup_keeps_latest_by_last_seen() {
        let peers = vec![
            make_peer("node-a", "key-1", 100, PeerStatus::Unreachable),
            make_peer("node-a", "key-1", 200, PeerStatus::Active),
            make_peer("node-a", "key-1", 150, PeerStatus::Unreachable),
        ];
        let result = dedup_peers(&peers);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].last_seen, 200);
        assert_eq!(result[0].status, PeerStatus::Active);
    }

    #[test]
    fn dedup_distinct_keys_kept() {
        let peers = vec![
            make_peer("node-a", "key-1", 100, PeerStatus::Active),
            make_peer("node-b", "key-2", 200, PeerStatus::Active),
        ];
        let result = dedup_peers(&peers);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn dedup_empty_input() {
        let result = dedup_peers(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn dedup_sorted_by_name() {
        let peers = vec![
            make_peer("node-c", "key-3", 100, PeerStatus::Active),
            make_peer("node-a", "key-1", 100, PeerStatus::Active),
            make_peer("node-b", "key-2", 100, PeerStatus::Active),
        ];
        let result = dedup_peers(&peers);
        assert_eq!(result[0].name, "node-a");
        assert_eq!(result[1].name, "node-b");
        assert_eq!(result[2].name, "node-c");
    }

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(0), "0s ago");
        assert_eq!(format_duration(30), "30s ago");
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
        assert_eq!(format_duration(100800), "1d 4h ago");
    }

    #[test]
    fn format_since_zero_returns_dash() {
        assert_eq!(format_since(0), "-");
    }

    #[test]
    fn format_since_future_returns_just_now() {
        let future = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 9999;
        assert_eq!(format_since(future), "just now");
    }

    #[test]
    fn format_traffic_zero() {
        assert_eq!(format_traffic(0, 0), "-");
    }

    #[test]
    fn format_traffic_bytes() {
        assert_eq!(format_traffic(512, 1024), "512B\u{2193} 1K\u{2191}");
    }

    #[test]
    fn format_traffic_megabytes() {
        let rx = 1_500_000;
        let tx = 3_500_000;
        assert_eq!(
            format_traffic(rx, tx),
            format!(
                "{}M\u{2193} {}M\u{2191}",
                rx / (1024 * 1024),
                tx / (1024 * 1024)
            )
        );
    }

    #[test]
    fn format_short_units() {
        assert_eq!(format_short(0), "0B");
        assert_eq!(format_short(500), "500B");
        assert_eq!(format_short(1024), "1K");
        assert_eq!(format_short(1024 * 1024), "1M");
        assert_eq!(format_short(1024 * 1024 * 1024), "1G");
    }

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_string() {
        assert_eq!(truncate("hello world", 8), "hello...");
    }
}
