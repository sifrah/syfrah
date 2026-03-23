use anyhow::Result;

use crate::events;

pub async fn run(json: bool) -> Result<()> {
    let events =
        events::list_events().map_err(|e| anyhow::anyhow!("failed to load events: {e}"))?;

    if events.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No events recorded yet.");
        }
        return Ok(());
    }

    if json {
        let json_str = serde_json::to_string_pretty(&events)?;
        println!("{json_str}");
        return Ok(());
    }

    let header = "DETAILS";
    println!(
        "{:<20} {:<24} {:<18} {}",
        "TIMESTAMP", "EVENT", "PEER", header
    );
    println!("{}", "-".repeat(90));

    for event in &events {
        let ts = format_timestamp(event.timestamp);
        let peer = event.peer_name.as_deref().unwrap_or("-");
        let details = event.details.as_deref().unwrap_or("");

        println!(
            "{:<20} {:<24} {:<18} {}",
            ts,
            event.event_type.to_string(),
            truncate(peer, 17),
            details,
        );
    }

    Ok(())
}

fn format_timestamp(epoch_secs: u64) -> String {
    // Format as YYYY-MM-DD HH:MM:SS using basic arithmetic (no chrono dep)
    // This is a simplified formatter; for production use chrono would be better.
    let secs_per_day: u64 = 86400;
    let secs_per_hour: u64 = 3600;
    let secs_per_min: u64 = 60;

    let days = epoch_secs / secs_per_day;
    let remaining = epoch_secs % secs_per_day;
    let hours = remaining / secs_per_hour;
    let remaining = remaining % secs_per_hour;
    let minutes = remaining / secs_per_min;
    let seconds = remaining % secs_per_min;

    // Convert days since epoch to Y-M-D (simplified leap year calculation)
    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}")
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Days since 1970-01-01
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

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}
