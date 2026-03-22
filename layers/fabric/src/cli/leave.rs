use anyhow::Result;
use crate::daemon;

pub async fn run() -> Result<()> {
    daemon::run_leave().await
}
