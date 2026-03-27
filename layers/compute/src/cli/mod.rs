//! CLI commands for `syfrah compute ...`.
//!
//! Provides subcommands for VM lifecycle management and compute layer
//! status queries. Each handler communicates with the daemon via the
//! control socket (stubbed for now — prints a placeholder message until
//! the daemon integration is complete).

pub mod vm;

use clap::Subcommand;

/// Top-level compute CLI command.
#[derive(Debug, Subcommand)]
pub enum ComputeCommand {
    /// Manage virtual machines
    Vm {
        #[command(subcommand)]
        command: vm::VmCommand,
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
        ComputeCommand::Status { json } => run_status(json).await,
    }
}

async fn run_status(json: bool) -> anyhow::Result<()> {
    if json {
        let status = serde_json::json!({
            "status": "not yet connected to daemon",
            "total_vms": 0,
            "running_vms": 0,
        });
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        println!("Compute Status");
        println!("  Status:      not yet connected to daemon");
        println!("  Total VMs:   0");
        println!("  Running VMs: 0");
    }
    Ok(())
}
