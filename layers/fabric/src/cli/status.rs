use crate::sanitize::sanitize;
use crate::{config, no_mesh_error, store, ui, wg};
use anyhow::Result;

/// Options for the status command.
pub struct StatusOpts {
    /// Show config and metrics sections.
    pub verbose: bool,
    /// Show the full mesh secret instead of masking it.
    pub show_secret: bool,
}

pub async fn run(opts: StatusOpts) -> Result<()> {
    let state = store::load().map_err(|_| no_mesh_error())?;

    // ── Uptime (computed early for the Mesh box) ────────────────────
    let m = &state.metrics;
    let uptime_str = if m.daemon_started_at > 0 {
        let uptime = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(m.daemon_started_at);
        Some(fmt_duration(uptime))
    } else {
        None
    };

    // ── Mesh section ────────────────────────────────────────────────
    let region = state
        .region
        .as_deref()
        .map(sanitize)
        .unwrap_or_else(|| "(not set)".into());
    let zone = state
        .zone
        .as_deref()
        .map(sanitize)
        .unwrap_or_else(|| "(not set)".into());

    ui::box_top("Mesh");
    ui::box_row(&format!("Name:     {}", sanitize(&state.mesh_name)));
    ui::box_row(&format!("Node:     {}", sanitize(&state.node_name)));
    ui::box_row(&format!("Region:   {} / zone: {}", region, zone));
    ui::box_row(&format!("Prefix:   {}/48", state.mesh_prefix));
    if let Some(ref up) = uptime_str {
        ui::box_row(&format!("Uptime:   {up}"));
    }
    ui::box_bottom();

    // ── Health status (prominent, outside a box) ────────────────────
    println!();
    let daemon_running = store::daemon_running();
    match daemon_running {
        Some(pid) => ui::health_ok(&format!("Daemon running (pid {pid})")),
        None => ui::health_bad("Daemon stopped"),
    }

    let iface_up = match wg::interface_summary() {
        Ok(summary) => summary.public_key.is_some(),
        Err(_) => false,
    };
    if iface_up {
        ui::health_ok("Interface syfrah0 is up");
    } else {
        ui::health_bad("Interface syfrah0 is down");
    }
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
    if ui::is_tty() {
        let green = console::Style::new().green();
        let red = console::Style::new().red();
        ui::box_row(&format!("{} {} active", green.apply_to("\u{25cf}"), active));
        if unreachable > 0 {
            ui::box_row(&format!(
                "{} {} unreachable",
                red.apply_to("\u{2717}"),
                unreachable
            ));
        } else {
            ui::box_row(&format!(
                "{} {} unreachable",
                green.apply_to("\u{25cf}"),
                unreachable
            ));
        }
    } else {
        ui::box_row(&format!("{active} active"));
        ui::box_row(&format!("{unreachable} unreachable"));
    }
    ui::box_bottom();
    println!();

    // ── Network section ─────────────────────────────────────────────
    let secret_display = if opts.show_secret {
        state.mesh_secret.clone()
    } else {
        format!(
            "{} (use --show-secret)",
            ui::mask_secret(&state.mesh_secret)
        )
    };

    ui::box_top("Network");
    ui::box_row(&format!("WireGuard:  port {}", state.wg_listen_port));
    if daemon_running.is_some() {
        ui::box_row(&format!("Peering:    active (port {})", state.peering_port));
    } else {
        ui::box_row(&format!(
            "Peering:    inactive (port {} configured)",
            state.peering_port
        ));
    }
    ui::box_row(&format!("Mesh IPv6:  {}", state.mesh_ipv6));
    ui::box_row(&format!("Secret:     {secret_display}"));

    // WG traffic summary if interface is up
    if let Ok(summary) = wg::interface_summary() {
        let (rx, tx) = summary.peers.iter().fold((0u64, 0u64), |(rx, tx), p| {
            (rx + p.rx_bytes, tx + p.tx_bytes)
        });
        ui::box_row(&format!(
            "Traffic:    rx {} / tx {}",
            fmt_bytes(rx),
            fmt_bytes(tx)
        ));
    }
    ui::box_bottom();

    // ── Verbose: Metrics ────────────────────────────────────────────
    if opts.verbose && m.daemon_started_at > 0 {
        println!();
        ui::box_top("Metrics");
        ui::box_row(&format!("Peers discovered:  {}", m.peers_discovered));
        ui::box_row(&format!("WG reconciles:     {}", m.wg_reconciliations));
        ui::box_row(&format!(
            "Peers unreached:   {}",
            m.peers_marked_unreachable
        ));
        ui::box_row(&format!("Announce fails:    {}", m.announcements_failed));
        ui::box_bottom();
    }

    // ── Verbose: Config ─────────────────────────────────────────────
    if opts.verbose {
        let tuning = config::load_tuning().unwrap_or_default();
        println!();
        ui::box_top("Config");
        ui::box_row(&format!(
            "health_check_interval:  {}s",
            tuning.health_check_interval.as_secs()
        ));
        ui::box_row(&format!(
            "reconcile_interval:     {}s",
            tuning.reconcile_interval.as_secs()
        ));
        ui::box_row(&format!(
            "persist_interval:       {}s",
            tuning.persist_interval.as_secs()
        ));
        ui::box_row(&format!(
            "unreachable_timeout:    {}s",
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
    fn fmt_duration_cases() {
        assert_eq!(fmt_duration(30), "30s");
        assert_eq!(fmt_duration(90), "1m 30s");
        assert_eq!(fmt_duration(3661), "1h 1m");
        assert_eq!(fmt_duration(90061), "1d 1h");
    }

    #[test]
    fn fmt_bytes_cases() {
        assert_eq!(fmt_bytes(500), "500 B");
        assert_eq!(fmt_bytes(2048), "2.0 KiB");
        assert_eq!(fmt_bytes(2 * 1024 * 1024), "2.0 MiB");
        assert_eq!(fmt_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }
}
