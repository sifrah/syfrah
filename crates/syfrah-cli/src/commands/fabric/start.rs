use anyhow::Result;
use syfrah_net::daemon;

pub async fn run() -> Result<()> {
    daemon::run_start().await
}
