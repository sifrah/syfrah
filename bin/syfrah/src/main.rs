use std::net::SocketAddr;

use anyhow::Result;
use clap::{Parser, Subcommand};

use syfrah_fabric::cli;
use syfrah_state::cli::StateCommand;

#[derive(Parser)]
#[command(
    name = "syfrah",
    about = "Syfrah — turn dedicated servers into a programmable cloud",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage the WireGuard fabric mesh
    Fabric {
        #[command(subcommand)]
        command: FabricCommand,
    },
    /// Inspect and manage layer state databases
    State {
        #[command(subcommand)]
        command: StateCommand,
    },
}

#[derive(Subcommand)]
enum FabricCommand {
    /// Create a new mesh network
    Init {
        #[arg(long)]
        name: String,
        #[arg(long)]
        node_name: Option<String>,
        #[arg(long, default_value = "51820")]
        port: u16,
        #[arg(long)]
        endpoint: Option<SocketAddr>,
        #[arg(long)]
        peering_port: Option<u16>,
        /// Region label for this node
        #[arg(long)]
        region: Option<String>,
        /// Zone label for this node (auto-incremented if not set)
        #[arg(long)]
        zone: Option<String>,
        #[arg(long, short)]
        daemon: bool,
    },
    /// Join an existing mesh
    Join {
        /// IP or IP:port of an existing node (default port: 51821)
        target: String,
        #[arg(long)]
        node_name: Option<String>,
        #[arg(long, default_value = "51820")]
        port: u16,
        #[arg(long)]
        endpoint: Option<SocketAddr>,
        /// PIN for auto-accept (skip manual approval)
        #[arg(long)]
        pin: Option<String>,
        /// Region label for this node
        #[arg(long)]
        region: Option<String>,
        /// Zone label for this node (auto-incremented if not set)
        #[arg(long)]
        zone: Option<String>,
        #[arg(long, short)]
        daemon: bool,
    },
    /// Restart the daemon from saved state
    Start {
        #[arg(long, short)]
        daemon: bool,
    },
    /// Stop the running daemon
    Stop,
    /// Show mesh and daemon status
    Status,
    /// List all peers
    Peers,
    /// Show the mesh secret
    Token,
    /// Rotate the mesh secret
    Rotate,
    /// Leave the mesh, tear down interface, clear state
    Leave,
    /// Run diagnostic checks on the fabric
    Diagnose,
    /// Manage peering — accept/reject join requests
    Peering {
        /// PIN for auto-accept mode
        #[arg(long)]
        pin: Option<String>,
        #[command(subcommand)]
        action: Option<PeeringAction>,
    },
}

#[derive(Subcommand)]
enum PeeringAction {
    /// Start accepting join requests (non-interactive)
    Start {
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        pin: Option<String>,
    },
    /// Stop accepting join requests
    Stop,
    /// List pending join requests
    List,
    /// Accept a join request
    Accept { request_id: String },
    /// Reject a join request
    Reject {
        request_id: String,
        #[arg(long)]
        reason: Option<String>,
    },
}

fn default_node_name() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "syfrah-node".into())
}

fn setup_logging(daemon_mode: bool) {
    let json_mode = std::env::var("SYFRAH_LOG_FORMAT")
        .map(|v| v == "json")
        .unwrap_or(false);

    if daemon_mode {
        let log_path = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".syfrah")
            .join("syfrah.log");
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(meta) = std::fs::metadata(&log_path) {
            if meta.len() > 10 * 1024 * 1024 {
                let old = log_path.with_extension("log.old");
                let _ = std::fs::rename(&log_path, &old);
            }
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .expect("failed to open log file");
        if json_mode {
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .with_writer(file)
                .with_ansi(false)
                .json()
                .init();
        } else {
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .with_writer(file)
                .with_ansi(false)
                .init();
        }
    } else if json_mode {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .json()
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .init();
    }
}

#[cfg(unix)]
fn daemonize() -> Result<bool> {
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        anyhow::bail!("fork failed");
    }
    if pid > 0 {
        return Ok(true);
    }
    if unsafe { libc::setsid() } < 0 {
        anyhow::bail!("setsid failed");
    }
    let pid2 = unsafe { libc::fork() };
    if pid2 < 0 {
        anyhow::bail!("second fork failed");
    }
    if pid2 > 0 {
        std::process::exit(0);
    }
    unsafe {
        let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        if devnull >= 0 {
            libc::dup2(devnull, 0);
            libc::dup2(devnull, 1);
            libc::dup2(devnull, 2);
            if devnull > 2 {
                libc::close(devnull);
            }
        }
    }
    Ok(false)
}

#[cfg(not(unix))]
fn daemonize() -> Result<bool> {
    anyhow::bail!("--daemon is only supported on Unix");
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let args = Cli::parse();

    match args.command {
        Commands::Fabric { command } => match command {
            FabricCommand::Init {
                name,
                node_name,
                port,
                endpoint,
                peering_port,
                region,
                zone,
                daemon,
            } => {
                let peering_port = peering_port.unwrap_or(port + 1);
                if daemon {
                    println!("Starting daemon in background...");
                    if daemonize()? {
                        println!("Use 'syfrah fabric status' to check.");
                        return Ok(());
                    }
                }
                setup_logging(daemon);
                cli::init::run(
                    &name,
                    &node_name.unwrap_or_else(default_node_name),
                    port,
                    endpoint,
                    peering_port,
                    region,
                    zone,
                )
                .await
            }
            FabricCommand::Join {
                target,
                node_name,
                port,
                endpoint,
                pin,
                region,
                zone,
                daemon,
            } => {
                if daemon {
                    println!("Starting daemon in background...");
                    if daemonize()? {
                        println!("Use 'syfrah fabric status' to check.");
                        return Ok(());
                    }
                }
                setup_logging(daemon);
                cli::join::run(
                    &target,
                    &node_name.unwrap_or_else(default_node_name),
                    port,
                    endpoint,
                    pin,
                    region,
                    zone,
                )
                .await
            }
            FabricCommand::Start { daemon } => {
                if daemon {
                    println!("Starting daemon in background...");
                    if daemonize()? {
                        println!("Use 'syfrah fabric status' to check.");
                        return Ok(());
                    }
                }
                setup_logging(daemon);
                cli::start::run().await
            }
            FabricCommand::Stop => {
                setup_logging(false);
                cli::stop::run().await
            }
            FabricCommand::Status => {
                setup_logging(false);
                cli::status::run().await
            }
            FabricCommand::Peers => {
                setup_logging(false);
                cli::peers::run().await
            }
            FabricCommand::Token => {
                setup_logging(false);
                cli::token::run().await
            }
            FabricCommand::Rotate => {
                setup_logging(false);
                cli::rotate::run().await
            }
            FabricCommand::Leave => {
                setup_logging(false);
                cli::leave::run().await
            }
            FabricCommand::Diagnose => {
                setup_logging(false);
                cli::diagnose::run().await
            }
            FabricCommand::Peering { pin, action } => {
                setup_logging(false);
                match action {
                    None => cli::peering::watch(pin).await,
                    Some(PeeringAction::Start {
                        port,
                        pin: start_pin,
                    }) => {
                        let port = port.unwrap_or(51821);
                        cli::peering::start(port, pin.or(start_pin)).await
                    }
                    Some(PeeringAction::Stop) => cli::peering::stop().await,
                    Some(PeeringAction::List) => cli::peering::list().await,
                    Some(PeeringAction::Accept { request_id }) => {
                        cli::peering::accept(&request_id).await
                    }
                    Some(PeeringAction::Reject { request_id, reason }) => {
                        cli::peering::reject(&request_id, reason).await
                    }
                }
            }
        },
        Commands::State { command } => syfrah_state::cli::run(command).await,
    }
}
