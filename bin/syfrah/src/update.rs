//! Self-update logic: check for new releases and replace the running binary.

use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use syfrah_fabric::ui;

const GITHUB_API_URL: &str = "https://api.github.com/repos/sifrah/syfrah/releases/latest";

/// Represents a GitHub release.
#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(serde::Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// Parse a version string like "v0.1.0" or "0.1.0" into (major, minor, patch).
fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let v = v.strip_prefix('v').unwrap_or(v);
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

/// Returns true if `latest` is newer than `current`.
fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

/// Return the Rust target triple for the current platform.
fn platform_target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-musl"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        (os, arch) => bail!("unsupported platform for self-update: {os}/{arch}"),
    }
}

/// Determine the expected archive asset name for the current platform and version.
fn asset_name(version: &str) -> Result<String> {
    let target = platform_target()?;
    Ok(format!("syfrah-{version}-{target}.tar.gz"))
}

/// Fetch the latest release metadata from GitHub.
fn fetch_latest_release() -> Result<Release> {
    let resp = ureq::get(GITHUB_API_URL)
        .set("User-Agent", "syfrah-updater")
        .set("Accept", "application/vnd.github+json")
        .call()
        .context("failed to query GitHub releases API")?;

    let release: Release = resp.into_json().context("failed to parse release JSON")?;
    Ok(release)
}

/// Check if an update is available and print the result.
/// Returns `Ok(true)` if an update is available.
pub fn check() -> Result<bool> {
    let current = env!("CARGO_PKG_VERSION");
    let sp = ui::spinner("Checking for updates...");

    let release = match fetch_latest_release() {
        Ok(r) => r,
        Err(e) => {
            ui::step_fail(&sp, "Failed to check for updates");
            bail!("{e}");
        }
    };

    let latest = &release.tag_name;

    if is_newer(current, latest) {
        ui::step_ok(&sp, &format!("Update available: v{current} -> {latest}"));
        Ok(true)
    } else {
        ui::step_ok(&sp, &format!("Already up to date (v{current})"));
        Ok(false)
    }
}

/// Stop the running daemon. Returns the number of known peers if a daemon was stopped.
fn stop_daemon() -> Result<Option<usize>> {
    let pid = match syfrah_fabric::store::daemon_running() {
        Some(pid) => pid,
        None => return Ok(None),
    };

    if !syfrah_fabric::store::is_syfrah_process(pid) {
        syfrah_fabric::store::remove_pid();
        return Ok(None);
    }

    let peer_count = syfrah_fabric::store::peer_count().unwrap_or(0);

    let sp = ui::spinner("Stopping daemon...");
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }

    // Wait up to 10 seconds for the daemon to exit
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if syfrah_fabric::store::daemon_running().is_none() {
            break;
        }
    }

    if syfrah_fabric::store::daemon_running().is_some() {
        ui::step_fail(&sp, &format!("Daemon (pid {pid}) did not stop in time"));
        bail!(
            "daemon did not stop within 10 seconds. \
             Stop it manually with 'kill {pid}' and re-run the update."
        );
    }

    syfrah_fabric::store::remove_pid();
    if peer_count > 0 {
        ui::step_ok(
            &sp,
            &format!("Daemon stopped ({peer_count} peers notified)"),
        );
    } else {
        ui::step_ok(&sp, "Daemon stopped");
    }
    Ok(Some(peer_count))
}

/// Start the daemon using the (new) binary on disk.
/// Returns the PID of the started daemon.
fn start_daemon(exe_path: &std::path::Path) -> Result<Option<u32>> {
    let sp = ui::spinner("Starting daemon...");

    let status = std::process::Command::new(exe_path)
        .args(["fabric", "start"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("failed to execute new binary for daemon start")?;

    if !status.success() {
        ui::step_fail(&sp, "Failed to start daemon");
        return Ok(None);
    }

    // Wait briefly for daemon to register its PID
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if syfrah_fabric::store::daemon_running().is_some() {
            break;
        }
    }

    match syfrah_fabric::store::daemon_running() {
        Some(pid) => {
            let peer_count = syfrah_fabric::store::peer_count().unwrap_or(0);
            if peer_count > 0 {
                ui::step_ok(
                    &sp,
                    &format!("Daemon started, reconnecting to mesh ({peer_count} peers)"),
                );
            } else {
                ui::step_ok(&sp, "Daemon started");
            }
            Ok(Some(pid))
        }
        None => {
            ui::step_fail(
                &sp,
                "Daemon may not have started. Check: ~/.syfrah/syfrah.log",
            );
            Ok(None)
        }
    }
}

/// Download and install the latest release, replacing the current binary.
///
/// When a daemon is running and `no_restart` is false, the daemon is
/// automatically stopped before the binary is replaced and restarted
/// afterward. If the new binary fails to start, the previous binary is
/// restored as a rollback.
pub fn run(no_restart: bool, force: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");

    // Step 1: check for update
    let sp = ui::spinner("Checking for updates...");
    let release = match fetch_latest_release() {
        Ok(r) => r,
        Err(e) => {
            ui::step_fail(&sp, "Failed to check for updates");
            bail!("{e}");
        }
    };

    let latest = &release.tag_name;
    if !is_newer(current, latest) {
        ui::step_ok(&sp, &format!("Already up to date (v{current})"));
        return Ok(());
    }
    ui::step_ok(&sp, &format!("Update available: v{current} -> {latest}"));

    // Check if daemon is running — we'll need to restart it later
    let daemon_was_running = syfrah_fabric::store::daemon_running().is_some();

    if daemon_was_running && !no_restart && !force {
        let peer_count = syfrah_fabric::store::peer_count().unwrap_or(0);
        if peer_count > 0 {
            ui::warn(&format!(
                "This will briefly interrupt connectivity with {peer_count} peer(s)."
            ));
        }
    }

    // Step 2: find the right asset
    let target = asset_name(latest)?;
    let binary_asset = release
        .assets
        .iter()
        .find(|a| a.name == target)
        .with_context(|| format!("no release asset found for {target}"))?;

    let checksum_asset = release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS.txt")
        .context("no SHA256SUMS.txt in release assets")?;

    // Step 3: download checksum file
    let sp = ui::spinner("Downloading checksums...");
    let checksums_text = ureq::get(&checksum_asset.browser_download_url)
        .set("User-Agent", "syfrah-updater")
        .call()
        .context("failed to download SHA256SUMS.txt")?
        .into_string()
        .context("failed to read SHA256SUMS.txt")?;
    ui::step_ok(&sp, "Downloaded checksums");

    // Parse expected checksum for our binary
    let expected_hash = checksums_text
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?;
            if name == target || name.trim_start_matches("*") == target {
                Some(hash.to_string())
            } else {
                None
            }
        })
        .with_context(|| format!("no checksum found for {target} in SHA256SUMS.txt"))?;

    // Step 4: download the binary
    let sp = ui::spinner(&format!("Downloading {target}..."));
    let resp = ureq::get(&binary_asset.browser_download_url)
        .set("User-Agent", "syfrah-updater")
        .call()
        .context("failed to download release binary")?;

    let mut binary_data = Vec::new();
    resp.into_reader()
        .take(256 * 1024 * 1024) // 256 MB limit
        .read_to_end(&mut binary_data)
        .context("failed to read binary data")?;
    ui::step_ok(
        &sp,
        &format!("Downloaded {target} ({} bytes)", binary_data.len()),
    );

    // Step 5: verify checksum
    let sp = ui::spinner("Verifying checksum...");
    let mut hasher = Sha256::new();
    hasher.update(&binary_data);
    let actual_hash = hex::encode(hasher.finalize());

    if actual_hash != expected_hash {
        ui::step_fail(&sp, "Checksum mismatch");
        bail!("checksum mismatch: expected {expected_hash}, got {actual_hash}");
    }
    ui::step_ok(&sp, "Checksum verified");

    // Step 6: stop daemon before replacing binary (unless --no-restart)
    if daemon_was_running && !no_restart {
        stop_daemon()?;
    }

    // Step 7: extract binary from tar.gz and atomic replace
    let sp = ui::spinner("Installing update...");
    let current_exe = std::env::current_exe().context("failed to determine current executable")?;

    // Extract the "syfrah" binary from the tar.gz archive
    let decoder = GzDecoder::new(std::io::Cursor::new(&binary_data));
    let mut archive = tar::Archive::new(decoder);
    let mut extracted_binary = None;
    for entry in archive.entries().context("failed to read tar entries")? {
        let mut entry = entry.context("failed to read tar entry")?;
        let path = entry.path().context("failed to read entry path")?;
        if path.file_name().and_then(|n| n.to_str()) == Some("syfrah") {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .context("failed to read syfrah binary from archive")?;
            extracted_binary = Some(buf);
            break;
        }
    }
    let new_binary_data = extracted_binary.context("archive does not contain a 'syfrah' binary")?;

    // Write to a temp file next to the current binary
    let parent = current_exe
        .parent()
        .context("current exe has no parent directory")?;
    let tmp_path: PathBuf = parent.join(".syfrah-update.tmp");
    let backup_path: PathBuf = parent.join(".syfrah-update.bak");

    if let Err(e) = fs::write(&tmp_path, &new_binary_data) {
        ui::step_fail(&sp, "Failed to write update");
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            bail!(
                "permission denied writing to {}. Try: sudo syfrah update",
                parent.display()
            );
        }
        bail!("failed to write temp file: {e}");
    }

    // chmod +x
    let perms = fs::Permissions::from_mode(0o755);
    fs::set_permissions(&tmp_path, perms).context("failed to set executable permissions")?;

    // Back up the current binary for rollback
    let has_backup = fs::copy(&current_exe, &backup_path).is_ok();
    if has_backup {
        let _ = fs::set_permissions(&backup_path, fs::Permissions::from_mode(0o755));
    }

    // Atomic rename over the current binary
    if let Err(e) = fs::rename(&tmp_path, &current_exe) {
        let _ = fs::remove_file(&tmp_path);
        let _ = fs::remove_file(&backup_path);
        ui::step_fail(&sp, "Failed to install update");
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            bail!(
                "permission denied replacing {}. Try: sudo syfrah update",
                current_exe.display()
            );
        }
        bail!("failed to replace binary: {e}");
    }

    ui::step_ok(&sp, &format!("Updated binary to {latest}"));

    // Step 8: restart daemon or print manual instructions
    if daemon_was_running {
        if no_restart {
            ui::warn("A daemon is running. Restart it to use the new version:");
            ui::warn("  syfrah fabric stop && syfrah fabric start");
        } else {
            // Try to start the daemon with the new binary
            match start_daemon(&current_exe) {
                Ok(Some(_)) => {
                    // Success — clean up backup
                    let _ = fs::remove_file(&backup_path);
                }
                Ok(None) | Err(_) => {
                    // Daemon failed to start — attempt rollback
                    if has_backup {
                        ui::warn("New daemon failed to start. Rolling back to previous version...");
                        let sp = ui::spinner("Restoring previous binary...");
                        if fs::rename(&backup_path, &current_exe).is_ok() {
                            ui::step_ok(&sp, "Previous binary restored");
                            // Try to start the old daemon
                            if start_daemon(&current_exe).is_ok() {
                                ui::warn(
                                    "Rolled back and restarted with previous version. \
                                     Check release notes for compatibility issues.",
                                );
                            } else {
                                ui::warn(
                                    "Previous binary restored but daemon failed to start. \
                                     Try: syfrah fabric start",
                                );
                            }
                        } else {
                            ui::step_fail(&sp, "Failed to restore previous binary");
                            ui::warn("Try starting the daemon manually: syfrah fabric start");
                        }
                    } else {
                        ui::warn(
                            "Daemon failed to start and no backup available. \
                             Try: syfrah fabric start",
                        );
                    }
                    let _ = fs::remove_file(&backup_path);
                    ui::success(&format!(
                        "syfrah updated to {latest} (daemon restart failed)."
                    ));
                    return Ok(());
                }
            }
        }
    } else {
        // Clean up backup — no daemon to worry about
        let _ = fs::remove_file(&backup_path);
    }

    ui::success(&format!("syfrah updated to {latest} successfully."));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("invalid"), None);
    }

    #[test]
    fn test_is_newer() {
        assert!(is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.1.0", "v0.2.0"));
        assert!(!is_newer("0.2.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(is_newer("0.1.0", "1.0.0"));
        assert!(is_newer("0.9.9", "0.10.0"));
    }

    #[test]
    fn test_platform_target() {
        let target = platform_target().unwrap();
        // Must be one of the four supported targets
        let valid = [
            "x86_64-unknown-linux-musl",
            "aarch64-unknown-linux-musl",
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
        ];
        assert!(valid.contains(&target), "unexpected target: {target}");
    }

    #[test]
    fn test_asset_name() {
        let name = asset_name("v0.3.0").unwrap();
        assert!(name.starts_with("syfrah-v0.3.0-"));
        assert!(name.ends_with(".tar.gz"));
        // Should contain a valid Rust target triple
        assert!(
            name.contains("x86_64") || name.contains("aarch64"),
            "missing arch in {name}"
        );
        assert!(
            name.contains("linux-musl") || name.contains("apple-darwin"),
            "missing OS in {name}"
        );
    }

    #[test]
    fn stop_daemon_returns_none_when_no_daemon() {
        // With no daemon running, stop_daemon should return Ok(None)
        let result = stop_daemon();
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn start_daemon_with_nonexistent_binary_fails() {
        let fake_path = std::path::Path::new("/tmp/nonexistent-syfrah-binary");
        let result = start_daemon(fake_path);
        // Should return an error because the binary doesn't exist
        assert!(result.is_err());
    }

    #[test]
    fn cli_parses_no_restart_flag() {
        use clap::Parser;

        // Verify --no-restart is recognized
        let cli = crate::Cli::parse_from(["syfrah", "update", "--no-restart"]);
        match cli.command {
            crate::Commands::Update {
                no_restart, force, ..
            } => {
                assert!(no_restart);
                assert!(!force);
            }
            _ => panic!("expected Update command"),
        }
    }

    #[test]
    fn cli_parses_force_flag() {
        use clap::Parser;

        let cli = crate::Cli::parse_from(["syfrah", "update", "--force"]);
        match cli.command {
            crate::Commands::Update { force, .. } => {
                assert!(force);
            }
            _ => panic!("expected Update command"),
        }
    }

    #[test]
    fn cli_parses_no_restart_and_force_together() {
        use clap::Parser;

        let cli = crate::Cli::parse_from(["syfrah", "update", "--no-restart", "--force"]);
        match cli.command {
            crate::Commands::Update {
                no_restart,
                force,
                check,
            } => {
                assert!(no_restart);
                assert!(force);
                assert!(!check);
            }
            _ => panic!("expected Update command"),
        }
    }
}
