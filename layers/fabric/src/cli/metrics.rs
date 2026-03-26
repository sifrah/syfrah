use anyhow::Result;

use crate::metrics;

/// Output all fabric metrics in Prometheus text exposition format.
///
/// This is designed to be consumed by `node_exporter` textfile collector,
/// a cron job piping to a file, or directly by any tool that speaks the
/// Prometheus text format.
pub async fn run() -> Result<()> {
    print!("{}", metrics::render_prometheus());
    Ok(())
}
