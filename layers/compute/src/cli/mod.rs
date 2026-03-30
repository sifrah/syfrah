//! CLI commands for `syfrah compute ...`.
//!
//! Provides subcommands for VM lifecycle management and compute layer
//! status queries. Each handler communicates with the daemon via the
//! control socket.

pub mod image;
pub mod vm;

use std::path::PathBuf;

use clap::Subcommand;

use crate::control::{send_compute_request, ComputeRequest, ComputeResponse};

/// Print a JSON error object to stdout and exit with code 1.
///
/// Used when `--json` is active so that callers parsing JSON output always
/// receive structured data, even on failure.
pub(crate) fn json_error_exit(msg: &str) -> ! {
    println!("{}", serde_json::json!({"error": msg}));
    std::process::exit(1)
}

/// Return the current terminal width, falling back to 120 columns.
pub(crate) fn term_width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(120)
}

/// Truncate a string to `max` characters, appending "..." if it exceeds the limit.
pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 3 {
        s[..max].to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

/// Top-level compute CLI command.
#[derive(Debug, Subcommand)]
pub enum ComputeCommand {
    /// Manage virtual machines
    Vm {
        #[command(subcommand)]
        command: vm::VmCommand,
    },
    /// Manage images
    Image {
        #[command(subcommand)]
        command: image::ImageCommand,
    },
    /// Show compute layer status
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Execute a compute CLI command.
pub async fn run(cmd: ComputeCommand) -> anyhow::Result<()> {
    match cmd {
        ComputeCommand::Vm { command } => vm::run(command).await,
        ComputeCommand::Image { command } => image::run(command).await,
        ComputeCommand::Status { json } => run_status(json).await,
    }
}

fn control_socket_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/root"))
        .join(".syfrah")
        .join("control.sock")
}

async fn run_status(json: bool) -> anyhow::Result<()> {
    let req = ComputeRequest::Status;
    let resp = match send_compute_request(&control_socket_path(), &req).await {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("failed to connect to daemon: {e}\n\nIs the daemon running? Initialize with: syfrah fabric init --name <mesh-name>");
            if json {
                json_error_exit(&msg);
            }
            anyhow::bail!("{msg}");
        }
    };

    match resp {
        ComputeResponse::Status(v) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("?");
                let runtime = v.get("runtime").and_then(|r| r.as_str()).unwrap_or("?");
                let total = v.get("total_vms").and_then(|t| t.as_u64()).unwrap_or(0);
                let running = v.get("running_vms").and_then(|r| r.as_u64()).unwrap_or(0);
                println!("Compute Status");
                println!("  Status:      {status}");
                println!("  Runtime:     {runtime}");
                println!("  Total VMs:   {total}");
                println!("  Running VMs: {running}");
                if let Some(warnings) = v.get("warnings").and_then(|w| w.as_array()) {
                    if !warnings.is_empty() {
                        println!("  Warnings:");
                        for w in warnings {
                            if let Some(msg) = w.as_str() {
                                println!("    - {msg}");
                            }
                        }
                    }
                }
            }
            Ok(())
        }
        ComputeResponse::Error(msg) => {
            if json {
                json_error_exit(&msg);
            }
            anyhow::bail!("{msg}");
        }
        _ => {
            if json {
                json_error_exit("unexpected response from daemon");
            }
            anyhow::bail!("unexpected response from daemon");
        }
    }
}
