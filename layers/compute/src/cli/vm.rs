//! VM lifecycle CLI commands.
//!
//! Subcommands: create, list, get, start, stop, delete, reboot, resize.
//! Each handler currently prints a stub message until daemon control
//! socket integration is complete.

use clap::Subcommand;

/// VM management subcommands.
#[derive(Debug, Subcommand)]
pub enum VmCommand {
    /// Create a new virtual machine
    Create {
        /// Human-readable name for the VM
        #[arg(long)]
        name: String,
        /// Number of virtual CPUs
        #[arg(long, default_value = "2")]
        vcpus: u32,
        /// Memory in megabytes
        #[arg(long, default_value = "2048")]
        memory: u32,
        /// Root filesystem image name (e.g. "ubuntu-24.04")
        #[arg(long)]
        image: String,
        /// Optional GPU PCI BDF address for VFIO passthrough
        #[arg(long)]
        gpu: Option<String>,
        /// TAP device name for networking
        #[arg(long)]
        tap: Option<String>,
    },
    /// List all virtual machines
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get details of a virtual machine
    Get {
        /// VM identifier
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Start (boot) a stopped virtual machine
    Start {
        /// VM identifier
        id: String,
    },
    /// Stop a running virtual machine
    Stop {
        /// VM identifier
        id: String,
        /// Force shutdown (kill) instead of graceful ACPI
        #[arg(long, short)]
        force: bool,
    },
    /// Delete a virtual machine and clean up all artifacts
    Delete {
        /// VM identifier
        id: String,
        /// Skip confirmation prompt
        #[arg(long, short)]
        yes: bool,
    },
    /// Reboot a running virtual machine
    Reboot {
        /// VM identifier
        id: String,
    },
    /// Hot-resize CPU and memory of a running virtual machine
    Resize {
        /// VM identifier
        id: String,
        /// New number of virtual CPUs
        #[arg(long)]
        vcpus: Option<u32>,
        /// New memory in megabytes
        #[arg(long)]
        memory: Option<u32>,
    },
}

/// Execute a VM subcommand.
pub async fn run(cmd: VmCommand) -> anyhow::Result<()> {
    match cmd {
        VmCommand::Create {
            name,
            vcpus,
            memory,
            image,
            gpu,
            tap,
        } => run_create(name, vcpus, memory, image, gpu, tap).await,
        VmCommand::List { json } => run_list(json).await,
        VmCommand::Get { id, json } => run_get(id, json).await,
        VmCommand::Start { id } => run_start(id).await,
        VmCommand::Stop { id, force } => run_stop(id, force).await,
        VmCommand::Delete { id, yes } => run_delete(id, yes).await,
        VmCommand::Reboot { id } => run_reboot(id).await,
        VmCommand::Resize { id, vcpus, memory } => run_resize(id, vcpus, memory).await,
    }
}

// ---------------------------------------------------------------------------
// Handlers (stubs until daemon control socket integration)
// ---------------------------------------------------------------------------

async fn run_create(
    name: String,
    vcpus: u32,
    memory: u32,
    image: String,
    gpu: Option<String>,
    tap: Option<String>,
) -> anyhow::Result<()> {
    eprintln!("not yet connected to daemon");
    println!("Would create VM:");
    println!("  Name:   {name}");
    println!("  vCPUs:  {vcpus}");
    println!("  Memory: {memory} MB");
    println!("  Image:  {image}");
    if let Some(ref g) = gpu {
        println!("  GPU:    {g}");
    }
    if let Some(ref t) = tap {
        println!("  TAP:    {t}");
    }
    Ok(())
}

async fn run_list(json: bool) -> anyhow::Result<()> {
    eprintln!("not yet connected to daemon");
    if json {
        println!("{}", serde_json::json!({"vms": []}));
    } else {
        println!(
            "{:<20} {:<12} {:<6} {:<10} {:<20} {:<10}",
            "ID", "PHASE", "vCPUs", "MEMORY", "IMAGE", "UPTIME"
        );
        println!("{}", "-".repeat(78));
        println!("(no VMs)");
    }
    Ok(())
}

async fn run_get(id: String, json: bool) -> anyhow::Result<()> {
    eprintln!("not yet connected to daemon");
    if json {
        let vm = serde_json::json!({
            "id": id,
            "phase": "Unknown",
            "vcpus": 0,
            "memory_mb": 0,
        });
        println!("{}", serde_json::to_string_pretty(&vm)?);
    } else {
        println!("VM Details");
        println!("  ID:        {id}");
        println!("  Phase:     Unknown");
        println!("  vCPUs:     -");
        println!("  Memory:    -");
        println!("  Image:     -");
        println!("  Created:   -");
        println!("  Uptime:    -");
    }
    Ok(())
}

async fn run_start(id: String) -> anyhow::Result<()> {
    eprintln!("not yet connected to daemon");
    println!("Would start VM: {id}");
    Ok(())
}

async fn run_stop(id: String, force: bool) -> anyhow::Result<()> {
    eprintln!("not yet connected to daemon");
    if force {
        println!("Would force-stop VM: {id}");
    } else {
        println!("Would gracefully stop VM: {id}");
    }
    Ok(())
}

async fn run_delete(id: String, yes: bool) -> anyhow::Result<()> {
    eprintln!("not yet connected to daemon");
    if !yes {
        println!("Would prompt for confirmation before deleting VM: {id}");
    }
    println!("Would delete VM: {id}");
    Ok(())
}

async fn run_reboot(id: String) -> anyhow::Result<()> {
    eprintln!("not yet connected to daemon");
    println!("Would reboot VM: {id}");
    Ok(())
}

async fn run_resize(id: String, vcpus: Option<u32>, memory: Option<u32>) -> anyhow::Result<()> {
    eprintln!("not yet connected to daemon");
    println!("Would resize VM: {id}");
    if let Some(v) = vcpus {
        println!("  New vCPUs:  {v}");
    }
    if let Some(m) = memory {
        println!("  New Memory: {m} MB");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    /// Helper to parse VM commands from an arg list.
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: VmCommand,
    }

    fn parse(args: &[&str]) -> VmCommand {
        let full_args = std::iter::once("test").chain(args.iter().copied());
        TestCli::parse_from(full_args).cmd
    }

    #[test]
    fn parse_create_minimal() {
        let cmd = parse(&["create", "--name", "test-vm", "--image", "ubuntu-24.04"]);
        match cmd {
            VmCommand::Create {
                name,
                vcpus,
                memory,
                image,
                gpu,
                tap,
            } => {
                assert_eq!(name, "test-vm");
                assert_eq!(vcpus, 2); // default
                assert_eq!(memory, 2048); // default
                assert_eq!(image, "ubuntu-24.04");
                assert!(gpu.is_none());
                assert!(tap.is_none());
            }
            other => panic!("expected Create, got {other:?}"),
        }
    }

    #[test]
    fn parse_create_full() {
        let cmd = parse(&[
            "create",
            "--name",
            "gpu-vm",
            "--vcpus",
            "8",
            "--memory",
            "16384",
            "--image",
            "ubuntu-24.04",
            "--gpu",
            "0000:01:00.0",
            "--tap",
            "tap0",
        ]);
        match cmd {
            VmCommand::Create {
                name,
                vcpus,
                memory,
                image,
                gpu,
                tap,
            } => {
                assert_eq!(name, "gpu-vm");
                assert_eq!(vcpus, 8);
                assert_eq!(memory, 16384);
                assert_eq!(image, "ubuntu-24.04");
                assert_eq!(gpu.as_deref(), Some("0000:01:00.0"));
                assert_eq!(tap.as_deref(), Some("tap0"));
            }
            other => panic!("expected Create, got {other:?}"),
        }
    }

    #[test]
    fn parse_list() {
        let cmd = parse(&["list"]);
        assert!(matches!(cmd, VmCommand::List { json: false }));
    }

    #[test]
    fn parse_list_json() {
        let cmd = parse(&["list", "--json"]);
        assert!(matches!(cmd, VmCommand::List { json: true }));
    }

    #[test]
    fn parse_get() {
        let cmd = parse(&["get", "vm-123"]);
        match cmd {
            VmCommand::Get { id, json } => {
                assert_eq!(id, "vm-123");
                assert!(!json);
            }
            other => panic!("expected Get, got {other:?}"),
        }
    }

    #[test]
    fn parse_get_json() {
        let cmd = parse(&["get", "vm-456", "--json"]);
        match cmd {
            VmCommand::Get { id, json } => {
                assert_eq!(id, "vm-456");
                assert!(json);
            }
            other => panic!("expected Get, got {other:?}"),
        }
    }

    #[test]
    fn parse_start() {
        let cmd = parse(&["start", "vm-789"]);
        assert!(matches!(cmd, VmCommand::Start { id } if id == "vm-789"));
    }

    #[test]
    fn parse_stop() {
        let cmd = parse(&["stop", "vm-abc"]);
        match cmd {
            VmCommand::Stop { id, force } => {
                assert_eq!(id, "vm-abc");
                assert!(!force);
            }
            other => panic!("expected Stop, got {other:?}"),
        }
    }

    #[test]
    fn parse_stop_force() {
        let cmd = parse(&["stop", "--force", "vm-abc"]);
        match cmd {
            VmCommand::Stop { id, force } => {
                assert_eq!(id, "vm-abc");
                assert!(force);
            }
            other => panic!("expected Stop, got {other:?}"),
        }
    }

    #[test]
    fn parse_delete() {
        let cmd = parse(&["delete", "vm-del"]);
        match cmd {
            VmCommand::Delete { id, yes } => {
                assert_eq!(id, "vm-del");
                assert!(!yes);
            }
            other => panic!("expected Delete, got {other:?}"),
        }
    }

    #[test]
    fn parse_delete_yes() {
        let cmd = parse(&["delete", "--yes", "vm-del"]);
        match cmd {
            VmCommand::Delete { id, yes } => {
                assert_eq!(id, "vm-del");
                assert!(yes);
            }
            other => panic!("expected Delete, got {other:?}"),
        }
    }

    #[test]
    fn parse_reboot() {
        let cmd = parse(&["reboot", "vm-reboot"]);
        assert!(matches!(cmd, VmCommand::Reboot { id } if id == "vm-reboot"));
    }

    #[test]
    fn parse_resize_vcpus_only() {
        let cmd = parse(&["resize", "vm-resize", "--vcpus", "4"]);
        match cmd {
            VmCommand::Resize { id, vcpus, memory } => {
                assert_eq!(id, "vm-resize");
                assert_eq!(vcpus, Some(4));
                assert!(memory.is_none());
            }
            other => panic!("expected Resize, got {other:?}"),
        }
    }

    #[test]
    fn parse_resize_memory_only() {
        let cmd = parse(&["resize", "vm-resize", "--memory", "8192"]);
        match cmd {
            VmCommand::Resize { id, vcpus, memory } => {
                assert_eq!(id, "vm-resize");
                assert!(vcpus.is_none());
                assert_eq!(memory, Some(8192));
            }
            other => panic!("expected Resize, got {other:?}"),
        }
    }

    #[test]
    fn parse_resize_both() {
        let cmd = parse(&["resize", "vm-resize", "--vcpus", "8", "--memory", "16384"]);
        match cmd {
            VmCommand::Resize { id, vcpus, memory } => {
                assert_eq!(id, "vm-resize");
                assert_eq!(vcpus, Some(8));
                assert_eq!(memory, Some(16384));
            }
            other => panic!("expected Resize, got {other:?}"),
        }
    }
}
