mod commands;

use std::net::SocketAddr;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "syfrah", about = "Syfrah mesh network CLI", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
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
        #[arg(long, short)]
        daemon: bool,
    },
    /// Join an existing mesh (just pass the IP of an existing node)
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
    /// Show the mesh secret for sharing
    Token,
    /// Rotate the mesh secret
    Rotate,
    /// Leave the mesh, tear down interface, clear state
    Leave,
    /// Manage peering — accept/reject join requests
    Peering {
        /// PIN for auto-accept mode (no manual approval needed)
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
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(file)
            .with_ansi(false)
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
    if pid < 0 { anyhow::bail!("fork failed"); }
    if pid > 0 { return Ok(true); }
    if unsafe { libc::setsid() } < 0 { anyhow::bail!("setsid failed"); }
    let pid2 = unsafe { libc::fork() };
    if pid2 < 0 { anyhow::bail!("second fork failed"); }
    if pid2 > 0 { std::process::exit(0); }
    unsafe {
        let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        if devnull >= 0 {
            libc::dup2(devnull, 0);
            libc::dup2(devnull, 1);
            libc::dup2(devnull, 2);
            if devnull > 2 { libc::close(devnull); }
        }
    }
    Ok(false)
}

#[cfg(not(unix))]
fn daemonize() -> Result<bool> {
    anyhow::bail!("--daemon is only supported on Unix");
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { name, node_name, port, endpoint, peering_port, daemon } => {
            let peering_port = peering_port.unwrap_or(port + 1);
            if daemon {
                println!("Starting daemon in background...");
                if daemonize()? { println!("Use 'syfrah status' to check."); return Ok(()); }
            }
            setup_logging(daemon);
            commands::init::run(&name, &node_name.unwrap_or_else(default_node_name), port, endpoint, peering_port).await
        }
        Commands::Join { target, node_name, port, endpoint, pin, daemon } => {
            if daemon {
                println!("Starting daemon in background...");
                if daemonize()? { println!("Use 'syfrah status' to check."); return Ok(()); }
            }
            setup_logging(daemon);
            commands::join::run(&target, &node_name.unwrap_or_else(default_node_name), port, endpoint, pin).await
        }
        Commands::Start { daemon } => {
            if daemon {
                println!("Starting daemon in background...");
                if daemonize()? { println!("Use 'syfrah status' to check."); return Ok(()); }
            }
            setup_logging(daemon);
            commands::start::run().await
        }
        Commands::Stop => { setup_logging(false); commands::stop::run().await }
        Commands::Status => { setup_logging(false); commands::status::run().await }
        Commands::Peers => { setup_logging(false); commands::peers::run().await }
        Commands::Token => { setup_logging(false); commands::token::run().await }
        Commands::Rotate => { setup_logging(false); commands::rotate::run().await }
        Commands::Leave => { setup_logging(false); commands::leave::run().await }
        Commands::Peering { pin, action } => {
            setup_logging(false);
            match action {
                None => {
                    // Interactive mode (default when no subcommand)
                    commands::peering::watch(pin).await
                }
                Some(PeeringAction::Start { port, pin: start_pin }) => {
                    let port = port.unwrap_or(51821);
                    commands::peering::start(port, pin.or(start_pin)).await
                }
                Some(PeeringAction::Stop) => commands::peering::stop().await,
                Some(PeeringAction::List) => commands::peering::list().await,
                Some(PeeringAction::Accept { request_id }) => commands::peering::accept(&request_id).await,
                Some(PeeringAction::Reject { request_id, reason }) => commands::peering::reject(&request_id, reason).await,
            }
        }
    }
}
