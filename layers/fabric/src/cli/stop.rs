use crate::store;
use anyhow::Result;

pub async fn run() -> Result<()> {
    match store::daemon_running() {
        Some(pid) => {
            println!("Stopping daemon (pid {pid})...");
            #[cfg(unix)]
            {
                // Send SIGTERM
                unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            }
            // Wait a moment for the daemon to clean up
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            if store::daemon_running().is_some() {
                eprintln!("Warning: daemon still running. Try 'kill {pid}'.");
            } else {
                store::remove_pid();
                println!("Daemon stopped.");
            }
        }
        None => {
            println!("No daemon running.");
            store::remove_pid(); // Clean up stale PID file
        }
    }
    Ok(())
}
