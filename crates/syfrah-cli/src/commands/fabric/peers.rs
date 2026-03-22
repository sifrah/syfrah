use std::time::SystemTime;

use anyhow::Result;
use syfrah_core::mesh::PeerStatus;
use syfrah_net::{store, wg};

pub async fn run() -> Result<()> {
    let state = store::load().map_err(|_| {
        anyhow::anyhow!("no mesh configured. Run 'syfrah init' or 'syfrah join' first.")
    })?;

    if state.peers.is_empty() {
        println!("No peers discovered yet.");
        return Ok(());
    }

    // Try to get live WG stats
    let wg_summary = wg::interface_summary().ok();

    println!(
        "{:<18} {:<40} {:<22} {:>8} {:>10} {:>10}",
        "NAME", "MESH IP", "ENDPOINT", "STATUS", "HANDSHAKE", "TRAFFIC"
    );
    println!("{}", "-".repeat(112));

    for peer in &state.peers {
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

        println!(
            "{:<18} {:<40} {:<22} {:>8} {:>10} {:>10}",
            truncate(&peer.name, 17),
            peer.mesh_ipv6,
            peer.endpoint,
            status,
            handshake_str,
            traffic_str,
        );
    }

    Ok(())
}

fn format_ago(time: SystemTime) -> String {
    let elapsed = SystemTime::now()
        .duration_since(time)
        .unwrap_or_default()
        .as_secs();

    if elapsed < 60 {
        format!("{elapsed}s ago")
    } else if elapsed < 3600 {
        format!("{}m ago", elapsed / 60)
    } else if elapsed < 86400 {
        format!("{}h ago", elapsed / 3600)
    } else {
        format!("{}d ago", elapsed / 86400)
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
