use crate::daemon;
use anyhow::{Context, Result};

pub async fn run() -> Result<()> {
    daemon::run_start()
        .await
        .context("Failed to start daemon. Is the mesh configured? Try: syfrah fabric status")
}
