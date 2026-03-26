use crate::control::{send_control_request, ControlRequest, ControlResponse};
use crate::store;
use anyhow::Result;

pub async fn run() -> Result<()> {
    let socket = store::control_socket_path();
    if !socket.exists() {
        anyhow::bail!("Daemon is not running. Start it with 'syfrah fabric start' first.");
    }

    let resp = send_control_request(&socket, &ControlRequest::Reload)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to daemon: {e}. Is the daemon running?"))?;

    match resp {
        ControlResponse::ConfigReloaded { changes, skipped } => {
            if changes.is_empty() && skipped.is_empty() {
                println!("OK: Configuration reloaded. No changes detected.");
            } else {
                let mut parts = Vec::new();
                for c in &changes {
                    parts.push(format!("  {c}"));
                }
                for s in &skipped {
                    parts.push(format!("  {s}"));
                }
                println!("OK: Configuration reloaded. Changed:");
                for p in &parts {
                    println!("{p}");
                }
            }
        }
        ControlResponse::Error { message } => {
            eprintln!("{message}");
            std::process::exit(1);
        }
        other => {
            anyhow::bail!("Unexpected response from daemon: {other:?}");
        }
    }

    Ok(())
}
