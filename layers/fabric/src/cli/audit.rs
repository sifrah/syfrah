use anyhow::Result;

use crate::audit::{self, AuditEventType};
use crate::sanitize::sanitize;
use crate::ui;

pub async fn run(
    json: bool,
    limit: Option<usize>,
    since: Option<u64>,
    event_type: Option<String>,
) -> Result<()> {
    let mut entries =
        audit::read_entries().map_err(|e| anyhow::anyhow!("failed to read audit log: {e}"))?;

    // Filter by --type
    if let Some(ref t) = event_type {
        // Validate the type string
        if AuditEventType::from_dotted(t).is_none() {
            anyhow::bail!(
                "unknown event type '{t}'. Valid types: peer.join.requested, \
                 peer.join.accepted, peer.join.rejected, peer.removed, \
                 peering.started, peering.stopped, secret.rotated, \
                 daemon.started, daemon.stopped"
            );
        }
        entries.retain(|e| e.event_type == *t);
    }

    // Filter by --since (keep only events at or after the given timestamp)
    if let Some(since_ts) = since {
        entries.retain(|e| e.timestamp >= since_ts);
    }

    // Apply --limit (show the N most recent entries; default 20)
    let n = limit.unwrap_or(20);
    let len = entries.len();
    if n < len {
        entries = entries.split_off(len - n);
    }

    if entries.is_empty() {
        if json {
            println!("[]");
        } else {
            ui::info_line("Audit", "No audit entries found.");
        }
        return Ok(());
    }

    if json {
        let json_str = serde_json::to_string_pretty(&entries)?;
        println!("{json_str}");
        return Ok(());
    }

    ui::heading(&format!(
        "{:<20} {:<24} {:<18} {}",
        "TIMESTAMP", "EVENT", "PEER", "DETAILS"
    ));

    for entry in &entries {
        let ts = format_timestamp(entry.timestamp);
        let peer = entry
            .peer_name
            .as_deref()
            .map(sanitize)
            .unwrap_or_else(|| "-".into());
        let details = entry.details.as_deref().map(sanitize).unwrap_or_default();

        println!(
            "{:<20} {:<24} {:<18} {}",
            ts,
            entry.event_type,
            truncate(&peer, 17),
            details,
        );
    }

    Ok(())
}

fn format_timestamp(epoch_secs: u64) -> String {
    let secs_per_day: u64 = 86400;
    let secs_per_hour: u64 = 3600;
    let secs_per_min: u64 = 60;

    let days = epoch_secs / secs_per_day;
    let remaining = epoch_secs % secs_per_day;
    let hours = remaining / secs_per_hour;
    let remaining = remaining % secs_per_hour;
    let minutes = remaining / secs_per_min;
    let seconds = remaining % secs_per_min;

    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}")
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970;

    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let month_days: [u64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }

    (year, month, days + 1)
}

fn is_leap(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

use super::ui::truncate;
