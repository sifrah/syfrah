//! Cloud Hypervisor binary resolution and version management.
//!
//! This module provides functions to:
//! - Read the pinned CH version (compile-time constant from `CLOUD_HYPERVISOR_VERSION`)
//! - Resolve the CH binary path (explicit > `/usr/local/lib/syfrah/` > `$PATH`)
//! - Check the version of a CH binary on disk
//! - Verify that the disk version matches the pinned version
//! - Build a version report comparing running VMs against the disk binary

use std::path::{Path, PathBuf};

use crate::error::ProcessError;

/// The Cloud Hypervisor version pinned at compile time.
///
/// This is read from the `CLOUD_HYPERVISOR_VERSION` file at the repo root
/// via `build.rs`, which sets the `CLOUD_HYPERVISOR_VERSION` env var.
const PINNED_VERSION: &str = env!("CLOUD_HYPERVISOR_VERSION");

/// Returns the pinned Cloud Hypervisor version (compile-time constant).
pub fn pinned_version() -> &'static str {
    PINNED_VERSION
}

/// Resolve the cloud-hypervisor binary path.
///
/// Resolution order:
/// 1. `explicit` path (if provided, must exist and be executable)
/// 2. `/usr/local/lib/syfrah/cloud-hypervisor` (standard install location)
/// 3. `cloud-hypervisor` on `$PATH` (via `which`)
///
/// Returns the first match or `ProcessError::SpawnFailed` if none found.
pub fn resolve_binary(explicit: Option<&Path>) -> Result<PathBuf, ProcessError> {
    // 1. Explicit path from config
    if let Some(path) = explicit {
        if is_executable(path) {
            return Ok(path.to_path_buf());
        }
        // Explicit path was given but the file doesn't exist — error immediately.
        if !path.exists() {
            return Err(ProcessError::SpawnFailed {
                reason: format!(
                    "configured cloud-hypervisor binary not found: {}",
                    path.display()
                ),
            });
        }
        // File exists but is not executable — fall through to other locations.
    }

    // 2. Standard installation path
    let installed = PathBuf::from("/usr/local/lib/syfrah/cloud-hypervisor");
    if is_executable(&installed) {
        return Ok(installed);
    }

    // 3. Search $PATH via `which`
    if let Ok(output) = std::process::Command::new("which")
        .arg("cloud-hypervisor")
        .output()
    {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout);
            let path = PathBuf::from(path_str.trim());
            if is_executable(&path) {
                return Ok(path);
            }
        }
    }

    Err(ProcessError::SpawnFailed {
        reason: "cloud-hypervisor not found".to_string(),
    })
}

/// Run `binary --version` and parse the version string from the output.
///
/// Cloud Hypervisor outputs something like: `cloud-hypervisor v43.0`
/// This function extracts and returns the version part (e.g., `v43.0`).
pub fn check_version(binary: &Path) -> Result<String, ProcessError> {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|e| ProcessError::SpawnFailed {
            reason: format!("failed to run {} --version: {e}", binary.display()),
        })?;

    if !output.status.success() {
        return Err(ProcessError::SpawnFailed {
            reason: format!(
                "{} --version exited with status {}",
                binary.display(),
                output.status,
            ),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_version_output(&stdout).ok_or_else(|| ProcessError::SpawnFailed {
        reason: format!(
            "could not parse version from '{}' --version output: {}",
            binary.display(),
            stdout.trim(),
        ),
    })
}

/// Compare the version reported by `binary --version` with the pinned version.
///
/// Returns `Ok(())` if they match, `Err(warning_message)` if they differ.
/// A mismatch is a warning, not a blocking error — old CH still works.
pub fn verify_version(binary: &Path) -> Result<(), String> {
    let disk_version = check_version(binary).map_err(|e| e.to_string())?;
    if disk_version == PINNED_VERSION {
        Ok(())
    } else {
        Err(format!(
            "cloud-hypervisor version mismatch: pinned {PINNED_VERSION}, disk {disk_version}"
        ))
    }
}

// ---------------------------------------------------------------------------
// VersionReport — for startup mismatch reporting (#481)
// ---------------------------------------------------------------------------

/// Report comparing the disk binary version against running VMs.
#[derive(Debug, Clone)]
pub struct VersionReport {
    /// The version pinned at compile time.
    pub pinned: String,
    /// The version of the CH binary currently on disk.
    pub disk: String,
    /// Whether the disk binary matches the pinned version.
    pub disk_matches: bool,
    /// Number of running VMs on the current (disk) version.
    pub vms_current: usize,
    /// Running VMs on an outdated version: `(vm_id, old_version)`.
    pub vms_outdated: Vec<(String, String)>,
}

/// Build a version report from the disk binary and a list of running VMs.
///
/// `vms` is a slice of `(vm_id, ch_version)` pairs gathered from each VM's
/// `meta.json` or runtime state.
pub fn build_version_report(disk_binary: &Path, vms: &[(String, String)]) -> VersionReport {
    let disk = check_version(disk_binary).unwrap_or_else(|_| "unknown".to_string());

    let pinned = PINNED_VERSION.to_string();
    let disk_matches = disk == pinned;

    let mut vms_current = 0usize;
    let mut vms_outdated = Vec::new();

    for (vm_id, vm_version) in vms {
        if vm_version == &disk {
            vms_current += 1;
        } else {
            vms_outdated.push((vm_id.clone(), vm_version.clone()));
        }
    }

    VersionReport {
        pinned,
        disk,
        disk_matches,
        vms_current,
        vms_outdated,
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Check if a path exists and is executable (Unix only).
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            return meta.permissions().mode() & 0o111 != 0;
        }
        false
    }
    #[cfg(not(unix))]
    {
        path.exists()
    }
}

/// Parse a version string from `cloud-hypervisor --version` output.
///
/// Expected format: `cloud-hypervisor vX.Y` or `cloud-hypervisor vX.Y.Z`
/// Returns the version token starting with 'v'.
fn parse_version_output(output: &str) -> Option<String> {
    for token in output.split_whitespace() {
        if token.starts_with('v') && token.len() > 1 && token[1..].chars().next()?.is_ascii_digit()
        {
            return Some(token.to_string());
        }
    }
    None
}
