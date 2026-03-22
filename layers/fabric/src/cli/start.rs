use crate::daemon;
use anyhow::Result;

pub async fn run() -> Result<()> {
    daemon::run_start().await
}
