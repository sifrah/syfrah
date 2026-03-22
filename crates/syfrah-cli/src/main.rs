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
        /// IPFS API endpoint (default: http://127.0.0.1:5001)
        #[arg(long)]
        ipfs_api: Option<String>,
        #[arg(long, short)]
        daemon: bool,
    },
    /// Join an existing mesh network
    Join {
        /// Mesh secret (syf_sk_...)
        secret: String,
        #[arg(long)]
        node_name: Option<String>,
        #[arg(long, default_value = "51820")]
        port: u16,
        #[arg(long)]
        endpoint: Option<SocketAddr>,
        #[arg(long)]
        ipfs_api: Option<String>,
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
    /// Rotate the mesh secret (all peers must rejoin)
    Rotate,
    /// Leave the mesh, tear down interface, clear state
    Leave,
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
        Commands::Init { name, node_name, port, endpoint, ipfs_api, daemon } => {
            if daemon {
                println!("Starting daemon in background...");
                if daemonize()? { println!("Use 'syfrah status' to check."); return Ok(()); }
            }
            setup_logging(daemon);
            commands::init::run(&name, &node_name.unwrap_or_else(default_node_name), port, endpoint, ipfs_api).await
        }
        Commands::Join { secret, node_name, port, endpoint, ipfs_api, daemon } => {
            if daemon {
                println!("Starting daemon in background...");
                if daemonize()? { println!("Use 'syfrah status' to check."); return Ok(()); }
            }
            setup_logging(daemon);
            commands::join::run(&secret, &node_name.unwrap_or_else(default_node_name), port, endpoint, ipfs_api).await
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
    }
}
