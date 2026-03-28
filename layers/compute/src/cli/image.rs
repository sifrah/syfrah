//! Image management CLI commands.
//!
//! Subcommands: list, inspect, pull, import, delete, catalog.
//! Each handler communicates with the daemon via the control socket.

use std::path::PathBuf;

use clap::Subcommand;

use crate::control::{send_compute_request, ComputeRequest, ComputeResponse};

/// Image management subcommands.
#[derive(Debug, Subcommand)]
pub enum ImageCommand {
    /// List locally available images
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show detailed metadata for an image
    Inspect {
        /// Image name
        name: String,
    },
    /// Download an image from the catalog
    Pull {
        /// Image name
        name: String,
    },
    /// Import a local raw disk image
    Import {
        /// Path to the raw disk image file
        path: PathBuf,
        /// Name to assign to the imported image
        #[arg(long)]
        name: String,
        /// CPU architecture (default: amd64)
        #[arg(long, default_value = "amd64")]
        arch: String,
    },
    /// Delete a locally cached image
    Delete {
        /// Image name
        name: String,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Show remote image catalog
    Catalog {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Execute an image subcommand.
pub async fn run(cmd: ImageCommand) -> anyhow::Result<()> {
    match cmd {
        ImageCommand::List { json } => run_list(json).await,
        ImageCommand::Inspect { name } => run_inspect(name).await,
        ImageCommand::Pull { name } => run_pull(name).await,
        ImageCommand::Import { path, name, arch } => run_import(path, name, arch).await,
        ImageCommand::Delete { name, yes } => run_delete(name, yes).await,
        ImageCommand::Catalog { json } => run_catalog(json).await,
    }
}

// ---------------------------------------------------------------------------
// Control socket path
// ---------------------------------------------------------------------------

fn control_socket_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/root"))
        .join(".syfrah")
        .join("control.sock")
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn run_list(json: bool) -> anyhow::Result<()> {
    let req = ComputeRequest::ImageList;
    let resp = send_compute_request(&control_socket_path(), &req)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to connect to daemon: {e}\n\nIs the daemon running? Try: syfrah start"
            )
        })?;

    match resp {
        ComputeResponse::ImageList(images) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&images)?);
            } else {
                println!(
                    "{:<25} {:<10} {:<10} {:<12} {:<10}",
                    "NAME", "ARCH", "SIZE MB", "CLOUD-INIT", "SOURCE"
                );
                println!("{}", "-".repeat(67));
                if images.is_empty() {
                    println!("(no images)");
                } else {
                    for img in &images {
                        let name = img.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                        let arch = img.get("arch").and_then(|a| a.as_str()).unwrap_or("?");
                        let size = img.get("size_mb").and_then(|s| s.as_u64()).unwrap_or(0);
                        let ci = img
                            .get("cloud_init")
                            .and_then(|c| c.as_bool())
                            .map(|b| if b { "yes" } else { "no" })
                            .unwrap_or("?");
                        let source = img
                            .get("source_kind")
                            .and_then(|s| s.as_str())
                            .unwrap_or("?");
                        println!("{name:<25} {arch:<10} {size:<10} {ci:<12} {source:<10}");
                    }
                }
            }
            Ok(())
        }
        ComputeResponse::Error(msg) => {
            anyhow::bail!("{msg}");
        }
        _ => {
            anyhow::bail!("unexpected response from daemon");
        }
    }
}

async fn run_inspect(name: String) -> anyhow::Result<()> {
    let req = ComputeRequest::ImageInspect { name };
    let resp = send_compute_request(&control_socket_path(), &req)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to connect to daemon: {e}\n\nIs the daemon running? Try: syfrah start"
            )
        })?;

    match resp {
        ComputeResponse::ImageMeta(v) => {
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(())
        }
        ComputeResponse::Error(msg) => {
            anyhow::bail!("{msg}");
        }
        _ => {
            anyhow::bail!("unexpected response from daemon");
        }
    }
}

async fn run_pull(name: String) -> anyhow::Result<()> {
    println!("Downloading {name}...");
    let req = ComputeRequest::ImagePull { name: name.clone() };
    let resp = send_compute_request(&control_socket_path(), &req)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to connect to daemon: {e}\n\nIs the daemon running? Try: syfrah start"
            )
        })?;

    match resp {
        ComputeResponse::ImageMeta(_) => {
            println!("Done.");
            Ok(())
        }
        ComputeResponse::Error(msg) => {
            anyhow::bail!("{msg}");
        }
        _ => {
            anyhow::bail!("unexpected response from daemon");
        }
    }
}

async fn run_import(path: PathBuf, name: String, arch: String) -> anyhow::Result<()> {
    let req = ComputeRequest::ImageImport {
        path: path.clone(),
        name: name.clone(),
        arch,
    };
    let resp = send_compute_request(&control_socket_path(), &req)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to connect to daemon: {e}\n\nIs the daemon running? Try: syfrah start"
            )
        })?;

    match resp {
        ComputeResponse::ImageMeta(v) => {
            let size = v.get("size_mb").and_then(|s| s.as_u64()).unwrap_or(0);
            let arch = v.get("arch").and_then(|a| a.as_str()).unwrap_or("?");
            println!("Imported {name} ({size} MB, {arch})");
            Ok(())
        }
        ComputeResponse::Error(msg) => {
            anyhow::bail!("{msg}");
        }
        _ => {
            anyhow::bail!("unexpected response from daemon");
        }
    }
}

async fn run_delete(name: String, yes: bool) -> anyhow::Result<()> {
    if !yes {
        eprint!("Delete image {name}? This cannot be undone. [y/N] ");
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        let answer = answer.trim();
        if answer != "y" && answer != "Y" {
            println!("Aborted.");
            return Ok(());
        }
    }

    let req = ComputeRequest::ImageDelete { name: name.clone() };
    let resp = send_compute_request(&control_socket_path(), &req)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to connect to daemon: {e}\n\nIs the daemon running? Try: syfrah start"
            )
        })?;

    match resp {
        ComputeResponse::Ok => {
            println!("Deleted {name}");
            Ok(())
        }
        ComputeResponse::Error(msg) => {
            anyhow::bail!("{msg}");
        }
        _ => {
            anyhow::bail!("unexpected response from daemon");
        }
    }
}

async fn run_catalog(json: bool) -> anyhow::Result<()> {
    let req = ComputeRequest::ImageCatalog;
    let resp = send_compute_request(&control_socket_path(), &req)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to connect to daemon: {e}\n\nIs the daemon running? Try: syfrah start"
            )
        })?;

    match resp {
        ComputeResponse::ImageCatalog(v) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&v)?);
                return Ok(());
            }
            let images = v.get("images").and_then(|i| i.as_array());
            match images {
                Some(images) => {
                    println!(
                        "{:<25} {:<10} {:<10} {:<12}",
                        "NAME", "ARCH", "SIZE MB", "CLOUD-INIT"
                    );
                    println!("{}", "-".repeat(57));
                    if images.is_empty() {
                        println!("(no images in catalog)");
                    } else {
                        for img in images {
                            let name = img.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                            let arch = img.get("arch").and_then(|a| a.as_str()).unwrap_or("?");
                            let size = img.get("size_mb").and_then(|s| s.as_u64()).unwrap_or(0);
                            let ci = img
                                .get("cloud_init")
                                .and_then(|c| c.as_bool())
                                .map(|b| if b { "yes" } else { "no" })
                                .unwrap_or("?");
                            println!("{name:<25} {arch:<10} {size:<10} {ci:<12}");
                        }
                    }
                }
                None => {
                    println!("{}", serde_json::to_string_pretty(&v)?);
                }
            }
            Ok(())
        }
        ComputeResponse::Error(msg) => {
            anyhow::bail!("{msg}");
        }
        _ => {
            anyhow::bail!("unexpected response from daemon");
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    /// Helper to parse image commands from an arg list.
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: ImageCommand,
    }

    fn parse(args: &[&str]) -> ImageCommand {
        let full_args = std::iter::once("test").chain(args.iter().copied());
        TestCli::parse_from(full_args).cmd
    }

    #[test]
    fn parse_list() {
        let cmd = parse(&["list"]);
        assert!(matches!(cmd, ImageCommand::List { json: false }));
    }

    #[test]
    fn parse_list_json() {
        let cmd = parse(&["list", "--json"]);
        assert!(matches!(cmd, ImageCommand::List { json: true }));
    }

    #[test]
    fn parse_inspect() {
        let cmd = parse(&["inspect", "ubuntu-24.04"]);
        match cmd {
            ImageCommand::Inspect { name } => assert_eq!(name, "ubuntu-24.04"),
            other => panic!("expected Inspect, got {other:?}"),
        }
    }

    #[test]
    fn parse_pull() {
        let cmd = parse(&["pull", "alpine-3.20"]);
        match cmd {
            ImageCommand::Pull { name } => assert_eq!(name, "alpine-3.20"),
            other => panic!("expected Pull, got {other:?}"),
        }
    }

    #[test]
    fn parse_import() {
        let cmd = parse(&["import", "/tmp/disk.raw", "--name", "custom-os"]);
        match cmd {
            ImageCommand::Import { path, name, arch } => {
                assert_eq!(path, PathBuf::from("/tmp/disk.raw"));
                assert_eq!(name, "custom-os");
                assert_eq!(arch, "amd64"); // default
            }
            other => panic!("expected Import, got {other:?}"),
        }
    }

    #[test]
    fn parse_import_with_arch() {
        let cmd = parse(&[
            "import",
            "/tmp/disk.raw",
            "--name",
            "custom-os",
            "--arch",
            "aarch64",
        ]);
        match cmd {
            ImageCommand::Import { path, name, arch } => {
                assert_eq!(path, PathBuf::from("/tmp/disk.raw"));
                assert_eq!(name, "custom-os");
                assert_eq!(arch, "aarch64");
            }
            other => panic!("expected Import, got {other:?}"),
        }
    }

    #[test]
    fn parse_delete() {
        let cmd = parse(&["delete", "old-image"]);
        match cmd {
            ImageCommand::Delete { name, yes } => {
                assert_eq!(name, "old-image");
                assert!(!yes);
            }
            other => panic!("expected Delete, got {other:?}"),
        }
    }

    #[test]
    fn parse_delete_yes() {
        let cmd = parse(&["delete", "--yes", "old-image"]);
        match cmd {
            ImageCommand::Delete { name, yes } => {
                assert_eq!(name, "old-image");
                assert!(yes);
            }
            other => panic!("expected Delete, got {other:?}"),
        }
    }

    #[test]
    fn parse_catalog() {
        let cmd = parse(&["catalog"]);
        assert!(matches!(cmd, ImageCommand::Catalog { json: false }));
    }

    #[test]
    fn parse_catalog_json() {
        let cmd = parse(&["catalog", "--json"]);
        assert!(matches!(cmd, ImageCommand::Catalog { json: true }));
    }
}
