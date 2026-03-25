use std::net::SocketAddr;

use anyhow::Result;
use clap::{Parser, Subcommand};

use syfrah_fabric::cli;
use syfrah_fabric::daemon::{self, DaemonConfig};
use syfrah_state::cli::StateCommand;

mod update;

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
    /// Update syfrah to the latest release
    Update {
        /// Only check if an update is available, don't install
        #[arg(long)]
        check: bool,
        /// Skip automatic daemon restart (print manual instructions instead)
        #[arg(long)]
        no_restart: bool,
        /// Skip the confirmation prompt when the node has active peer connections
        #[arg(long)]
        force: bool,
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
        /// Region label for this node [default: "default"]
        #[arg(long)]
        region: Option<String>,
        /// Zone label for this node (auto-incremented if not set)
        #[arg(long)]
        zone: Option<String>,
        /// Run daemon in foreground instead of backgrounding
        #[arg(long, short)]
        foreground: bool,
        /// Start peering with auto-accept PIN after init
        #[arg(long)]
        peering: bool,
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
        /// Region label for this node [default: "default"]
        #[arg(long)]
        region: Option<String>,
        /// Zone label for this node (auto-incremented if not set)
        #[arg(long)]
        zone: Option<String>,
        /// Run daemon in foreground instead of backgrounding
        #[arg(long, short)]
        foreground: bool,
    },
    /// Start the daemon from saved state
    Start {
        /// Run daemon in foreground instead of backgrounding
        #[arg(long, short)]
        foreground: bool,
    },
    /// Stop the running daemon
    Stop,
    /// Show mesh and daemon status
    Status {
        /// Show config and metrics sections
        #[arg(long)]
        verbose: bool,
        /// Show the full mesh secret (masked by default)
        #[arg(long)]
        show_secret: bool,
    },
    /// Show the event log
    Events {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
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
    /// Manage the systemd service
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Manage peering — accept/reject join requests
    Peering {
        #[command(subcommand)]
        action: PeeringAction,
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
    /// Watch for join requests interactively
    Watch {
        /// PIN for auto-accept mode
        #[arg(long)]
        pin: Option<String>,
        /// Stay open to accept multiple join requests
        #[arg(long, name = "continuous")]
        continuous: bool,
    },
    /// Accept a join request
    Accept { request_id: String },
    /// Reject a join request
    Reject {
        request_id: String,
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Install and enable the systemd service
    Install,
    /// Disable and remove the systemd service
    Uninstall,
    /// Show systemd service status
    Status,
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
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }
        if let Ok(meta) = std::fs::metadata(&log_path) {
            if meta.len() > 10 * 1024 * 1024 {
                let old = log_path.with_extension("log.old");
                let _ = std::fs::rename(&log_path, &old);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o600));
                }
            }
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .expect("failed to open log file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&log_path, std::fs::Permissions::from_mode(0o600));
        }
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

/// Spawn the daemon in background via double-fork + re-exec with `start --foreground`.
/// State must already be saved before calling this.
///
/// Uses the classic double-fork pattern so that:
/// 1. Parent forks -> intermediate child
/// 2. Intermediate forks -> grandchild (the daemon), then exits
/// 3. Parent reaps intermediate (no zombie)
/// 4. Grandchild calls setsid + exec, is reparented to init
#[cfg(unix)]
fn background_daemon() -> Result<()> {
    use std::io::Write;
    let _ = std::io::stdout().flush();

    let exe = std::env::current_exe()?;

    // Open the log file for stderr so we can capture startup errors
    let log_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".syfrah");
    let _ = std::fs::create_dir_all(&log_dir);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&log_dir, std::fs::Permissions::from_mode(0o700));
    }
    let log_path = log_dir.join("syfrah.log");

    // First fork
    let pid1 = unsafe { libc::fork() };
    if pid1 < 0 {
        anyhow::bail!("fork failed");
    }
    if pid1 == 0 {
        // Intermediate child: setsid, fork again, then exit
        unsafe { libc::setsid() };

        let pid2 = unsafe { libc::fork() };
        if pid2 < 0 {
            std::process::exit(1);
        }
        if pid2 > 0 {
            // Intermediate exits immediately — grandchild becomes orphan
            std::process::exit(0);
        }

        // Grandchild: redirect stdio and exec the daemon
        unsafe {
            let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
            if devnull >= 0 {
                libc::dup2(devnull, 0); // stdin
                libc::dup2(devnull, 1); // stdout
                if devnull > 2 {
                    libc::close(devnull);
                }
            }
            // Redirect stderr to log file
            if let Ok(log_cstr) = std::ffi::CString::new(log_path.to_string_lossy().as_bytes()) {
                let log_fd = libc::open(
                    log_cstr.as_ptr(),
                    libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND,
                    0o600,
                );
                if log_fd >= 0 {
                    libc::dup2(log_fd, 2); // stderr
                    if log_fd > 2 {
                        libc::close(log_fd);
                    }
                }
            }
        }

        // Exec the daemon — replaces current process
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(&exe)
            .args(["fabric", "start", "--foreground"])
            .exec();
        // If exec fails, write error and exit
        eprintln!("exec failed: {err}");
        std::process::exit(1);
    }

    // Parent: wait for intermediate child to exit (reap it, no zombie)
    unsafe {
        let mut status: libc::c_int = 0;
        libc::waitpid(pid1, &mut status, 0);
    }

    // Wait for the grandchild daemon to start and write its PID file.
    // Poll up to 5s — the daemon needs time to open redb, set up WG, etc.
    let mut daemon_pid = None;
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Some(pid) = syfrah_fabric::store::daemon_running() {
            daemon_pid = Some(pid);
            break;
        }
    }
    match daemon_pid {
        Some(pid) => {
            println!("Daemon started (pid {pid}).");
        }
        None => {
            eprintln!("Warning: daemon may have failed to start. Check logs: ~/.syfrah/syfrah.log");
        }
    }
    println!("Run 'syfrah fabric status' to check.");
    Ok(())
}

#[cfg(not(unix))]
fn background_daemon() -> Result<()> {
    anyhow::bail!("background daemon is only supported on Unix. Use --foreground instead.");
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
                foreground,
                peering,
            } => {
                let peering_port = peering_port.unwrap_or(port + 1);
                let config = DaemonConfig {
                    mesh_name: name,
                    node_name: node_name.unwrap_or_else(default_node_name),
                    wg_listen_port: port,
                    public_endpoint: endpoint,
                    peering_port,
                    region,
                    zone,
                };
                if foreground {
                    setup_logging(true);
                    daemon::run_init(config).await
                } else {
                    daemon::setup_init(&config)?;
                    background_daemon()?;
                    if peering {
                        cli::init::wait_and_start_peering(endpoint, peering_port).await
                    } else {
                        Ok(())
                    }
                }
            }
            FabricCommand::Join {
                target,
                node_name,
                port,
                endpoint,
                pin,
                region,
                zone,
                foreground,
            } => {
                let config = DaemonConfig {
                    mesh_name: String::new(),
                    node_name: node_name.unwrap_or_else(default_node_name),
                    wg_listen_port: port,
                    public_endpoint: endpoint,
                    peering_port: port + 1,
                    region,
                    zone,
                };
                // Parse target: "1.2.3.4" -> "1.2.3.4:51821", or "1.2.3.4:9999" as-is
                let target_addr: SocketAddr = if target.contains(':') {
                    target
                        .parse()
                        .map_err(|e| anyhow::anyhow!("invalid target address '{target}': {e}"))?
                } else {
                    format!("{target}:51821")
                        .parse()
                        .map_err(|e| anyhow::anyhow!("invalid target address '{target}': {e}"))?
                };
                if foreground {
                    setup_logging(true);
                    daemon::run_join(target_addr, config, pin).await
                } else {
                    daemon::setup_join(target_addr, &config, pin).await?;
                    background_daemon()
                }
            }
            FabricCommand::Start { foreground } => {
                if !foreground {
                    if let Some(pid) = syfrah_fabric::store::daemon_running() {
                        eprintln!(
                            "Daemon is already running (pid {pid}). Use 'syfrah fabric stop' first."
                        );
                        std::process::exit(1);
                    }
                }
                if foreground {
                    // Log to file when running as daemon (foreground or background)
                    setup_logging(true);
                    cli::start::run().await
                } else {
                    // Validate state can be loaded before spawning background daemon
                    syfrah_fabric::store::load().map_err(|_| syfrah_fabric::no_mesh_error())?;
                    background_daemon()
                }
            }
            FabricCommand::Stop => {
                setup_logging(false);
                cli::stop::run().await
            }
            FabricCommand::Status {
                verbose,
                show_secret,
            } => {
                setup_logging(false);
                cli::status::run(cli::status::StatusOpts {
                    verbose,
                    show_secret,
                })
                .await
            }
            FabricCommand::Events { json } => {
                setup_logging(false);
                cli::events::run(json).await
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
            FabricCommand::Service { action } => {
                setup_logging(false);
                match action {
                    ServiceAction::Install => cli::service::install().await,
                    ServiceAction::Uninstall => cli::service::uninstall().await,
                    ServiceAction::Status => cli::service::status().await,
                }
            }
            FabricCommand::Peering { action } => {
                setup_logging(false);
                match action {
                    PeeringAction::Start { port, pin } => {
                        let port = port.unwrap_or(51821);
                        cli::peering::start(port, pin).await
                    }
                    PeeringAction::Stop => cli::peering::stop().await,
                    PeeringAction::List => cli::peering::list().await,
                    PeeringAction::Watch { pin, continuous } => {
                        cli::peering::watch(pin, continuous).await
                    }
                    PeeringAction::Accept { request_id } => cli::peering::accept(&request_id).await,
                    PeeringAction::Reject { request_id, reason } => {
                        cli::peering::reject(&request_id, reason).await
                    }
                }
            }
        },
        Commands::State { command } => syfrah_state::cli::run(command).await,
        Commands::Update {
            check,
            no_restart,
            force,
        } => {
            if check {
                update::check()?;
                Ok(())
            } else {
                update::run(no_restart, force)
            }
        }
    }
}
