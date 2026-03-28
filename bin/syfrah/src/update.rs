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
    let pid_i32 = i32::try_from(pid).context("daemon PID out of range for signal delivery")?;
    #[cfg(unix)]
    unsafe {
        libc::kill(pid_i32, libc::SIGTERM);
    }

    // Wait up to 10 seconds for the daemon to exit
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if syfrah_fabric::store::daemon_running().is_none() {
            break;
        }
    }

    // Escalate to SIGKILL if SIGTERM didn't work, with retries
    if syfrah_fabric::store::daemon_running().is_some() {
        let mut killed = false;
        for attempt in 0..3 {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid_i32, libc::SIGKILL);
            }
            // Wait up to 1s for SIGKILL to take effect
            for _ in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if syfrah_fabric::store::daemon_running().is_none() {
                    killed = true;
                    break;
                }
            }
            if killed {
                break;
            }

            // Check process state to diagnose why SIGKILL didn't work
            match syfrah_fabric::store::process_state(pid) {
                Some('Z') => {
                    // Zombie — reap it and we're done
                    syfrah_fabric::store::try_reap(pid);
                    syfrah_fabric::store::remove_pid();
                    killed = true;
                    break;
                }
                Some('D') => {
                    ui::warn(&format!(
                        "Daemon (pid {pid}) is in uninterruptible I/O (D state), \
                         retry {}/3...",
                        attempt + 1
                    ));
                }
                _ => {}
            }
        }

        if !killed && syfrah_fabric::store::daemon_running().is_some() {
            // Check one last time for zombie or D state
            match syfrah_fabric::store::process_state(pid) {
                Some('Z') => {
                    syfrah_fabric::store::try_reap(pid);
                    syfrah_fabric::store::remove_pid();
                }
                Some('D') => {
                    ui::warn(&format!(
                        "Daemon (pid {pid}) stuck in uninterruptible I/O. \
                         A reboot may be required to fully reclaim resources."
                    ));
                    ui::warn("Proceeding with update anyway — the old binary will be replaced.");
                }
                _ => {
                    ui::warn(&format!(
                        "Daemon (pid {pid}) did not stop after 3 SIGKILL attempts. \
                         Proceeding with update anyway."
                    ));
                }
            }
        }
    }

    // Only remove PID if no new daemon has started in the meantime (TOCTOU guard).
    if syfrah_fabric::store::daemon_running().is_none() {
        syfrah_fabric::store::remove_pid();
    }
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

/// Signal the running daemon to re-exec itself with SIGUSR1 for zero-downtime update.
///
/// Reads the daemon PID from the PID file, sends SIGUSR1, waits for the daemon
/// to re-exec, and verifies it is still running with the new binary.
fn signal_daemon_reexec() -> Result<()> {
    let pid = match syfrah_fabric::store::daemon_running() {
        Some(pid) => pid,
        None => bail!("daemon not running, no reload needed"),
    };

    if !syfrah_fabric::store::is_syfrah_process(pid) {
        syfrah_fabric::store::remove_pid();
        bail!("PID {pid} is not a syfrah process");
    }

    let sp = ui::spinner("Signaling daemon to reload...");

    let pid_i32 = i32::try_from(pid).context("daemon PID out of range for signal delivery")?;
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid_i32, libc::SIGUSR1) };
        if rc != 0 {
            ui::step_fail(&sp, "Could not signal daemon");
            bail!(
                "failed to send SIGUSR1 to daemon (pid {pid}): {}",
                std::io::Error::last_os_error()
            );
        }
    }
    #[cfg(not(unix))]
    {
        ui::step_fail(&sp, "SIGUSR1 not supported on this platform");
        bail!("SIGUSR1 re-exec is only supported on Unix");
    }

    // Wait for the daemon to re-exec. exec() replaces the process image
    // in-place (PID stays the same on Unix), so we just need to verify the
    // daemon is still alive after the signal. Give it up to 5 seconds.
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if syfrah_fabric::store::daemon_running().is_some() {
            ui::step_ok(&sp, "Daemon reloaded with new binary");
            return Ok(());
        }
    }

    // The daemon may have died during re-exec. Check once more.
    if syfrah_fabric::store::daemon_running().is_some() {
        ui::step_ok(&sp, "Daemon reloaded with new binary");
        Ok(())
    } else {
        ui::step_fail(&sp, "Daemon did not recover after SIGUSR1");
        bail!("daemon did not restart within 5 seconds after SIGUSR1")
    }
}

/// Start the daemon using the (new) binary on disk.
/// Returns the PID of the started daemon.
fn start_daemon(exe_path: &std::path::Path) -> Result<Option<u32>> {
    let sp = ui::spinner("Starting daemon...");

    let output = std::process::Command::new(exe_path)
        .args(["fabric", "start"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .context("failed to execute new binary for daemon start")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = if stderr.trim().is_empty() {
            format!("exit code: {:?}", output.status.code())
        } else {
            stderr.trim().to_string()
        };
        ui::step_fail(&sp, &format!("Failed to start daemon: {detail}"));
        bail!("daemon failed to start: {detail}");
    }

    // Wait up to 5 seconds for daemon to register its PID.
    // This matches the timeout used by background_daemon() to avoid racing.
    for _ in 0..50 {
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
                "Daemon process exited successfully but did not register a PID within 5s. \
                 Check: ~/.syfrah/syfrah.log",
            );
            bail!("daemon started but did not register PID within 5 seconds")
        }
    }
}

/// Download and install the latest release, replacing the current binary.
///
/// When a daemon is running and `no_restart` is false, the daemon is
/// signaled via SIGUSR1 to re-exec itself with the new binary (zero-downtime).
/// If SIGUSR1 fails, falls back to stop/start. If the new binary fails to
/// start, the previous binary is restored as a rollback.
pub fn run(no_restart: bool, _force: bool) -> Result<()> {
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

    // Note: daemon-running check is deferred until just before install to avoid TOCTOU.

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

    // Step 6: check daemon state before replacing binary.
    // We no longer stop the daemon here — SIGUSR1 re-exec handles the
    // reload after the binary is replaced on disk.
    let daemon_was_running = syfrah_fabric::store::daemon_running().is_some();

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

    // Step 8: signal daemon to re-exec or print manual instructions
    if daemon_was_running {
        if no_restart {
            ui::warn("A daemon is running. Restart it to use the new version:");
            ui::warn("  syfrah fabric stop && syfrah fabric start");
        } else {
            match signal_daemon_reexec() {
                Ok(()) => {
                    // Daemon re-exec succeeded — backup no longer needed.
                    let _ = fs::remove_file(&backup_path);
                }
                Err(e) => {
                    ui::warn(&format!(
                        "Could not signal daemon for zero-downtime reload: {e}"
                    ));
                    ui::warn("Falling back to stop/start...");
                    let _ = stop_daemon();
                    match start_daemon(&current_exe) {
                        Ok(_) => {
                            let _ = fs::remove_file(&backup_path);
                        }
                        Err(e2) => {
                            rollback_daemon(&backup_path, &current_exe, has_backup, latest);
                            bail!(
                                "daemon failed to start after update to {latest}; \
                                 rolled back to previous version: {e2}"
                            );
                        }
                    }
                }
            }
        }
    } else {
        let _ = fs::remove_file(&backup_path);
    }

    ui::success(&format!(
        "Updated to {latest}. Daemon reloaded — no restart needed."
    ));
    Ok(())
}

/// Attempt to restore the previous binary and restart the daemon after a failed start.
fn rollback_daemon(
    backup_path: &std::path::Path,
    current_exe: &std::path::Path,
    has_backup: bool,
    latest: &str,
) {
    if !has_backup {
        ui::warn(
            "Daemon failed to start and no backup available. \
             Try: syfrah fabric start",
        );
        ui::warn(&format!(
            "Binary was replaced with {latest} but the daemon could not be started."
        ));
        return;
    }

    ui::warn("New daemon failed to start. Rolling back to previous version...");
    let sp = ui::spinner("Restoring previous binary...");

    if fs::rename(backup_path, current_exe).is_err() {
        ui::step_fail(&sp, "Failed to restore previous binary");
        // Do NOT delete the backup — it's the user's only recovery option.
        ui::warn(
            "The backup is preserved at the .bak path. \
             Try restoring it manually and run: syfrah fabric start",
        );
        ui::warn(&format!(
            "Update to {latest} failed: could not restore previous binary."
        ));
        return;
    }

    ui::step_ok(&sp, "Previous binary restored");
    // Backup has been consumed by the rename — no file to clean up.

    if start_daemon(current_exe).is_ok() {
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

    ui::warn(&format!(
        "Update to {latest} was rolled back. The previous version is still running."
    ));
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
        // NOTE: This test assumes no syfrah daemon is running in the test environment.
        // If a daemon happens to be running, the test will fail because stop_daemon()
        // will attempt to kill it. This IS environment-dependent — skip in CI if
        // a syfrah daemon may be running alongside tests.
        if syfrah_fabric::store::daemon_running().is_some() {
            eprintln!("skipping stop_daemon_returns_none_when_no_daemon: daemon is running");
            return;
        }
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
    fn rollback_preserves_backup_on_rename_failure() {
        // Simulate: backup exists but rename to a non-writable target fails.
        // The backup file must NOT be deleted.
        let dir = std::env::temp_dir().join("syfrah-test-rollback-preserve");
        let _ = fs::create_dir_all(&dir);

        let backup = dir.join("syfrah.bak");
        fs::write(&backup, b"old-binary").unwrap();

        // Point current_exe at a path we can't write to (nonexistent deep dir)
        let impossible_target = dir
            .join("no-such-dir")
            .join("deeply")
            .join("nested")
            .join("syfrah");

        rollback_daemon(&backup, &impossible_target, true, "v0.0.0-test");

        // The backup should still exist because the rename failed
        assert!(backup.exists(), "backup was deleted after failed rollback");

        // Clean up
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rollback_without_backup_does_not_panic() {
        let dir = std::env::temp_dir().join("syfrah-test-rollback-no-backup");
        let _ = fs::create_dir_all(&dir);

        let backup = dir.join("syfrah.bak");
        let exe = dir.join("syfrah");

        // has_backup = false — should just warn, not panic or try to rename
        rollback_daemon(&backup, &exe, false, "v0.0.0-test");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rollback_successful_rename_consumes_backup() {
        // When rollback rename succeeds, the backup file is consumed (moved to current_exe).
        let dir = std::env::temp_dir().join("syfrah-test-rollback-rename-ok");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);

        let backup = dir.join("syfrah.bak");
        let exe = dir.join("syfrah");

        fs::write(&backup, b"old-binary-content").unwrap();

        rollback_daemon(&backup, &exe, true, "v0.0.0-test");

        // The backup should have been renamed to exe, so backup is gone
        assert!(!backup.exists(), "backup should be consumed by rename");
        // The exe should now have the old binary content
        assert!(exe.exists(), "exe should exist after rollback");
        assert_eq!(
            fs::read(&exe).unwrap(),
            b"old-binary-content",
            "exe should contain the old binary"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn start_daemon_with_nonexistent_binary_returns_err() {
        // Verify start_daemon returns Err (not Ok(None)) for a missing binary.
        // This confirms the Ok(None) -> Err refactor is correct.
        let fake_path = std::path::Path::new("/tmp/nonexistent-syfrah-binary-err-check");
        let result = start_daemon(fake_path);
        assert!(
            result.is_err(),
            "start_daemon should return Err for missing binary"
        );
    }

    #[test]
    fn is_newer_edge_cases() {
        // Additional edge cases for version comparison
        assert!(
            !is_newer("invalid", "0.1.0"),
            "invalid current should not be newer"
        );
        assert!(
            !is_newer("0.1.0", "invalid"),
            "invalid latest should not be newer"
        );
        assert!(
            !is_newer("invalid", "invalid"),
            "both invalid should not be newer"
        );
        assert!(is_newer("0.0.1", "0.0.2"), "patch bump should be newer");
        assert!(
            !is_newer("0.0.2", "0.0.1"),
            "patch downgrade should not be newer"
        );
    }

    #[test]
    fn signal_reexec_returns_err_when_no_daemon() {
        // When no daemon is running, signal_daemon_reexec should return an error.
        if syfrah_fabric::store::daemon_running().is_some() {
            eprintln!("skipping signal_reexec_returns_err_when_no_daemon: daemon is running");
            return;
        }
        let result = signal_daemon_reexec();
        assert!(result.is_err(), "should fail when no daemon is running");
        assert!(
            result.unwrap_err().to_string().contains("not running"),
            "error should mention daemon not running"
        );
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
