use crate::sanitize::sanitize;
use crate::{config, store, ui, wg};
use anyhow::Result;

pub async fn run(verbose: bool, show_secret: bool) -> Result<()> {
    let state = store::load().map_err(|_| {
        anyhow::anyhow!(
            "no mesh configured. Run 'syfrah fabric init' or 'syfrah fabric join' first."
        )
    })?;

    // ── Mesh section ────────────────────────────────────────────────
    let region_zone = match (state.region.as_deref(), state.zone.as_deref()) {
        (Some(r), Some(z)) => format!("{} / zone: {}", sanitize(r), sanitize(z)),
        (Some(r), None) => sanitize(r).to_string(),
        _ => "(not set)".into(),
    };

    let uptime_str = {
        let m = &state.metrics;
        if m.daemon_started_at > 0 {
            let uptime = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .saturating_sub(m.daemon_started_at);
            fmt_duration(uptime)
        } else {
            "-".into()
        }
    };

    ui::box_top("Mesh");
    ui::box_line(&format!(" Name:     {}", sanitize(&state.mesh_name)));
    ui::box_line(&format!(" Node:     {}", sanitize(&state.node_name)));
    ui::box_line(&format!(" Region:   {region_zone}"));
    ui::box_line(&format!(" Prefix:   {}/48", state.mesh_prefix));
    ui::box_line(&format!(" Uptime:   {uptime_str}"));
    ui::box_bottom();
    println!();

    // ── Health status (prominent, outside box) ──────────────────────
    let daemon_running = store::daemon_running();
    match &daemon_running {
        Some(pid) => ui::health_line(true, &format!("Daemon running (pid {pid})")),
        None => ui::health_line(false, "Daemon stopped"),
    }

    let iface_up = match wg::interface_summary() {
        Ok(summary) => {
            let up = summary.public_key.is_some();
            if up {
                ui::health_line(true, &format!("Interface {} is up", summary.name));
            } else {
                ui::health_line(false, &format!("Interface {} is down", summary.name));
            }
            Some((up, summary))
        }
        Err(_) => {
            ui::health_line(false, "Interface syfrah0 is down");
            None
        }
    };
    println!();

    // ── Peers section ───────────────────────────────────────────────
    let total = state.peers.len();
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

    ui::box_top(&format!("Peers ({total})"));
    ui::peer_line("\u{25cf}", active, "active");
    if unreachable > 0 {
        ui::peer_line("\u{2717}", unreachable, "unreachable");
    }
    ui::box_bottom();
    println!();

    // ── WireGuard info (if interface is up) ─────────────────────────
    if let Some((true, ref summary)) = iface_up {
        let with_handshake = summary
            .peers
            .iter()
            .filter(|p| p.last_handshake.is_some())
            .count();
        let (rx, tx) = summary.peers.iter().fold((0u64, 0u64), |(rx, tx), p| {
            (rx + p.rx_bytes, tx + p.tx_bytes)
        });
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

        ui::box_top("WireGuard");
        ui::box_line(&format!(
            " Peers:     {} configured, {} with handshake",
            summary.peer_count, with_handshake
        ));
        if summary.peer_count > 0 {
            ui::box_line(&format!(
                " Health:    {}/{} recent handshake (<3min)",
                healthy, summary.peer_count
            ));
        }
        ui::box_line(&format!(
            " Traffic:   rx {} / tx {}",
            fmt_bytes(rx),
            fmt_bytes(tx)
        ));
        ui::box_bottom();
        println!();
    }

    // ── Network section ─────────────────────────────────────────────
    let secret_display = if show_secret {
        state.mesh_secret.clone()
    } else {
        format!(
            "{} (use --show-secret)",
            ui::mask_secret(&state.mesh_secret)
        )
    };

    ui::box_top("Network");
    ui::box_line(&format!(" WireGuard:  port {}", state.wg_listen_port));
    ui::box_line(&format!(" Peering:    port {}", state.peering_port));
    ui::box_line(&format!(" Mesh IPv6:  {}", state.mesh_ipv6));
    ui::box_line(&format!(" Secret:     {secret_display}"));
    ui::box_bottom();

    // ── Verbose-only: Metrics and Config ────────────────────────────
    if verbose {
        let m = &state.metrics;
        if m.daemon_started_at > 0 {
            println!();
            ui::box_top("Metrics");
            ui::box_line(&format!(" Peers discovered:  {}", m.peers_discovered));
            ui::box_line(&format!(" WG reconciles:     {}", m.wg_reconciliations));
            ui::box_line(&format!(
                " Peers unreached:   {}",
                m.peers_marked_unreachable
            ));
            ui::box_line(&format!(" Announce fails:    {}", m.announcements_failed));
            ui::box_bottom();
        }

        let tuning = config::load_tuning().unwrap_or_default();
        println!();
        ui::box_top("Config");
        ui::box_line(&format!(
            " health_check_interval:  {}s",
            tuning.health_check_interval.as_secs()
        ));
        ui::box_line(&format!(
            " reconcile_interval:     {}s",
            tuning.reconcile_interval.as_secs()
        ));
        ui::box_line(&format!(
            " persist_interval:       {}s",
            tuning.persist_interval.as_secs()
        ));
        ui::box_line(&format!(
            " unreachable_timeout:    {}s",
            tuning.unreachable_timeout.as_secs()
        ));
        ui::box_bottom();
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_duration_formats_correctly() {
        assert_eq!(fmt_duration(30), "30s");
        assert_eq!(fmt_duration(90), "1m 30s");
        assert_eq!(fmt_duration(3661), "1h 1m");
        assert_eq!(fmt_duration(90061), "1d 1h");
    }

    #[test]
    fn fmt_bytes_formats_correctly() {
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1536), "1.5 KiB");
        assert_eq!(fmt_bytes(1_572_864), "1.5 MiB");
        assert_eq!(fmt_bytes(1_610_612_736), "1.5 GiB");
    }
}
