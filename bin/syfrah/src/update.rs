//! Self-update logic: check for new releases and replace the running binary.

use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
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

/// Determine the expected binary asset name for the current platform.
fn asset_name() -> Result<String> {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        bail!("unsupported OS for self-update");
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        bail!("unsupported architecture for self-update");
    };

    Ok(format!("syfrah-{os}-{arch}"))
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

/// Download and install the latest release, replacing the current binary.
pub fn run() -> Result<()> {
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

    // Step 2: find the right asset
    let target = asset_name()?;
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

    // Step 6: atomic replace
    let sp = ui::spinner("Installing update...");
    let current_exe = std::env::current_exe().context("failed to determine current executable")?;

    // Write to a temp file next to the current binary
    let parent = current_exe
        .parent()
        .context("current exe has no parent directory")?;
    let tmp_path: PathBuf = parent.join(".syfrah-update.tmp");

    if let Err(e) = fs::write(&tmp_path, &binary_data) {
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

    // Atomic rename over the current binary
    if let Err(e) = fs::rename(&tmp_path, &current_exe) {
        let _ = fs::remove_file(&tmp_path);
        ui::step_fail(&sp, "Failed to install update");
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            bail!(
                "permission denied replacing {}. Try: sudo syfrah update",
                current_exe.display()
            );
        }
        bail!("failed to replace binary: {e}");
    }

    ui::step_ok(&sp, &format!("Updated to {latest}"));

    // Step 7: warn about running daemon
    if syfrah_fabric::store::read_pid().is_some() {
        ui::warn("A daemon is running. Restart it to use the new version:");
        ui::warn("  syfrah fabric stop && syfrah fabric start");
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
    fn test_asset_name() {
        let name = asset_name().unwrap();
        assert!(name.starts_with("syfrah-"));
        // Should contain OS and arch
        assert!(name.contains("linux") || name.contains("darwin"));
        assert!(name.contains("amd64") || name.contains("arm64"));
    }
}
