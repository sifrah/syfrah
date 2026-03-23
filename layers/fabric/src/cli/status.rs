use crate::{config, store, wg};
use anyhow::Result;

pub async fn run() -> Result<()> {
    let state = store::load().map_err(|_| {
        anyhow::anyhow!("no mesh configured. Run 'syfrah init' or 'syfrah join' first.")
    })?;

    println!("Mesh:      {}", state.mesh_name);
    println!("Node:      {}", state.node_name);
    println!("Mesh IPv6: {}", state.mesh_ipv6);
    println!("Prefix:    {}/48", state.mesh_prefix);
    println!("WG port:   {}", state.wg_listen_port);
    println!(
        "Region:    {}",
        state.region.as_deref().unwrap_or("(not set)")
    );
    println!(
        "Zone:      {}",
        state.zone.as_deref().unwrap_or("(not set)")
    );
    println!("Secret:    {}", state.mesh_secret);
    println!("Peering:   port {}", state.peering_port);

    match store::daemon_running() {
        Some(pid) => println!("Daemon:    running (pid {pid})"),
        None => println!("Daemon:    stopped"),
    }
    println!();

    match wg::interface_summary() {
        Ok(summary) => {
            println!(
                "Interface: {} ({})",
                summary.name,
                if summary.public_key.is_some() {
                    "up"
                } else {
                    "down"
                }
            );
            if let Some(port) = summary.listen_port {
                println!("Listen:    :{port}");
            }
            let with_handshake = summary
                .peers
                .iter()
                .filter(|p| p.last_handshake.is_some())
                .count();
            println!(
                "WG peers:  {} configured, {} with handshake",
                summary.peer_count, with_handshake
            );
            let (rx, tx) = summary.peers.iter().fold((0u64, 0u64), |(rx, tx), p| {
                (rx + p.rx_bytes, tx + p.tx_bytes)
            });
            println!("Traffic:   rx {} / tx {}", fmt_bytes(rx), fmt_bytes(tx));

            // Handshake health: count peers with recent handshake (<3min)
            let now_ts = std::time::SystemTime::now();
            let healthy = summary
                .peers
                .iter()
                .filter(|p| {
                    p.last_handshake
                        .map(|h| now_ts.duration_since(h).unwrap_or_default().as_secs() < 180)
                        .unwrap_or(false)
                })
                .count();
            if summary.peer_count > 0 {
                println!(
                    "Health:    {}/{} peers with recent handshake (<3min)",
                    healthy, summary.peer_count
                );
            }
        }
        Err(_) => println!("Interface: syfrah0 (down)"),
    }

    // Peer status breakdown
    let active = state
        .peers
        .iter()
        .filter(|p| p.status == syfrah_core::mesh::PeerStatus::Active)
        .count();
    let unreachable = state
        .peers
        .iter()
        .filter(|p| p.status == syfrah_core::mesh::PeerStatus::Unreachable)
        .count();
    println!();
    println!(
        "Peers:     {} total ({} active, {} unreachable)",
        state.peers.len(),
        active,
        unreachable
    );

    let m = &state.metrics;
    if m.daemon_started_at > 0 {
        let uptime = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(m.daemon_started_at);
        println!();
        println!("Metrics:");
        println!("  Uptime:           {}", fmt_duration(uptime));
        println!("  Peers discovered: {}", m.peers_discovered);
        println!("  WG reconciles:    {}", m.wg_reconciliations);
        println!("  Peers unreached:  {}", m.peers_marked_unreachable);
        println!("  Announce fails:   {}", m.announcements_failed);
    }

    let tuning = config::load_tuning().unwrap_or_default();
    println!();
    println!("Config:");
    println!(
        "  health_check_interval: {}s",
        tuning.health_check_interval.as_secs()
    );
    println!(
        "  reconcile_interval:    {}s",
        tuning.reconcile_interval.as_secs()
    );
    println!(
        "  persist_interval:      {}s",
        tuning.persist_interval.as_secs()
    );
    println!(
        "  unreachable_timeout:   {}s",
        tuning.unreachable_timeout.as_secs()
    );

    Ok(())
}

fn fmt_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs < 86400 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d {}h", secs / 86400, (secs % 86400) / 3600)
    }
}

fn fmt_bytes(b: u64) -> String {
    if b < 1024 {
        format!("{b} B")
    } else if b < 1024 * 1024 {
        format!("{:.1} KiB", b as f64 / 1024.0)
    } else if b < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", b as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GiB", b as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
