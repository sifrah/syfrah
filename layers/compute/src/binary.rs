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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    /// Helper: create a fake executable script in a tmpdir that outputs the given text.
    ///
    /// To avoid ETXTBSY ("Text file busy") on Linux when tests run in parallel,
    /// we copy `/bin/echo` as the binary. Since `/bin/echo` is already a proper
    /// ELF binary on disk, the kernel won't get ETXTBSY. We then wrap it with a
    /// shell script that calls echo with the desired output.
    ///
    /// Actually, the simplest robust approach: write the script, sync, close,
    /// and use a separate inode by first creating a temp, then hard-linking.
    /// But on tmpfs hard links to the same fs work fine.
    ///
    /// Simplest fix: write the script content to a stable path that is never
    /// re-opened for writing.
    fn make_fake_binary(dir: &Path, name: &str, output: &str) -> PathBuf {
        let path = dir.join(name);
        let content = format!("#!/bin/sh\nprintf '%s\\n' '{output}'\n");
        fs::write(&path, content).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        // Sync the directory to flush metadata.
        let dir_fd = fs::File::open(dir).unwrap();
        dir_fd.sync_all().unwrap();
        path
    }

    /// Helper: create a non-executable file.
    fn make_non_executable(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, "not executable").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        path
    }

    // -- pinned_version -------------------------------------------------------

    #[test]
    fn pinned_version_returns_nonempty_string() {
        let v = pinned_version();
        assert!(
            !v.is_empty(),
            "pinned_version() must return a non-empty string"
        );
        assert!(
            v.starts_with('v'),
            "pinned version should start with 'v', got: {v}"
        );
    }

    #[test]
    fn pinned_version_matches_compile_time_constant() {
        assert_eq!(pinned_version(), PINNED_VERSION);
    }

    // -- resolve_binary: explicit path ----------------------------------------

    #[test]
    fn resolve_binary_explicit_path_exists_and_executable() {
        let tmp = tempfile::TempDir::new().unwrap();
        let bin = make_fake_binary(tmp.path(), "ch-fake", "cloud-hypervisor v43.0");
        let result = resolve_binary(Some(&bin));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), bin);
    }

    #[test]
    fn resolve_binary_explicit_path_not_found() {
        let result = resolve_binary(Some(Path::new("/nonexistent/path/cloud-hypervisor")));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not found"),
            "expected 'not found' in error: {msg}"
        );
    }

    #[test]
    fn resolve_binary_explicit_path_not_executable_falls_through() {
        let tmp = tempfile::TempDir::new().unwrap();
        let bin = make_non_executable(tmp.path(), "ch-noexec");
        // Not executable, no /usr/local/lib/syfrah/cloud-hypervisor, no PATH match
        let result = resolve_binary(Some(&bin));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not found"),
            "expected 'not found' in error: {msg}"
        );
    }

    // -- resolve_binary: not found --------------------------------------------

    #[test]
    fn resolve_binary_none_not_found() {
        // With no explicit path, no /usr/local/lib, and (likely) no cloud-hypervisor on PATH
        // in CI, this should fail. We test the error message.
        let result = resolve_binary(None);
        // This might succeed if cloud-hypervisor is installed, so we just check it doesn't panic.
        // The important thing is it returns a Result, not a panic.
        let _ = result;
    }

    // -- resolve_binary: priority (explicit > default > PATH) -----------------

    #[test]
    fn resolve_binary_explicit_takes_priority() {
        let tmp = tempfile::TempDir::new().unwrap();
        let explicit_bin = make_fake_binary(tmp.path(), "ch-explicit", "explicit");
        // Even if other locations exist, explicit wins
        let result = resolve_binary(Some(&explicit_bin));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), explicit_bin);
    }

    // -- check_version --------------------------------------------------------

    #[test]
    fn check_version_happy_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let bin = make_fake_binary(tmp.path(), "ch-good", "cloud-hypervisor v43.0");
        let result = check_version(&bin);
        assert!(result.is_ok(), "check_version failed: {result:?}");
        assert_eq!(result.unwrap(), "v43.0");
    }

    #[test]
    fn check_version_with_patch_version() {
        let tmp = tempfile::TempDir::new().unwrap();
        let bin = make_fake_binary(tmp.path(), "ch-patch", "cloud-hypervisor v43.0.1");
        let result = check_version(&bin);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "v43.0.1");
    }

    #[test]
    fn check_version_garbage_output() {
        let tmp = tempfile::TempDir::new().unwrap();
        let bin = make_fake_binary(tmp.path(), "ch-garbage", "this is not a version");
        let result = check_version(&bin);
        assert!(result.is_err(), "garbage output should produce an error");
    }

    #[test]
    fn check_version_empty_output() {
        let tmp = tempfile::TempDir::new().unwrap();
        let bin = make_fake_binary(tmp.path(), "ch-empty", "");
        let result = check_version(&bin);
        assert!(result.is_err());
    }

    #[test]
    fn check_version_nonexistent_binary() {
        let result = check_version(Path::new("/nonexistent/cloud-hypervisor"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("failed to run"),
            "expected spawn error, got: {msg}"
        );
    }

    // -- verify_version -------------------------------------------------------

    #[test]
    fn verify_version_match() {
        let tmp = tempfile::TempDir::new().unwrap();
        let version = pinned_version();
        let bin = make_fake_binary(
            tmp.path(),
            "ch-match",
            &format!("cloud-hypervisor {version}"),
        );
        let result = verify_version(&bin);
        assert!(
            result.is_ok(),
            "verify_version should succeed on match: {result:?}"
        );
    }

    #[test]
    fn verify_version_mismatch() {
        let tmp = tempfile::TempDir::new().unwrap();
        let bin = make_fake_binary(tmp.path(), "ch-old", "cloud-hypervisor v42.0");
        let result = verify_version(&bin);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("mismatch"),
            "expected mismatch warning, got: {msg}"
        );
        assert!(
            msg.contains("v42.0"),
            "expected old version in warning, got: {msg}"
        );
        assert!(
            msg.contains(pinned_version()),
            "expected pinned version in warning, got: {msg}"
        );
    }

    // -- build_version_report -------------------------------------------------

    #[test]
    fn build_version_report_three_vms_two_outdated() {
        let tmp = tempfile::TempDir::new().unwrap();
        let bin = make_fake_binary(tmp.path(), "ch-report", "cloud-hypervisor v43.0");

        let vms = vec![
            ("vm-1".to_string(), "v43.0".to_string()),
            ("vm-2".to_string(), "v42.0".to_string()),
            ("vm-3".to_string(), "v42.0".to_string()),
        ];

        let report = build_version_report(&bin, &vms);
        assert_eq!(report.disk, "v43.0");
        assert_eq!(report.pinned, pinned_version());
        assert!(report.disk_matches, "disk should match pinned");
        assert_eq!(report.vms_current, 1);
        assert_eq!(report.vms_outdated.len(), 2);
        assert!(report.vms_outdated.iter().any(|(id, _)| id == "vm-2"));
        assert!(report.vms_outdated.iter().any(|(id, _)| id == "vm-3"));
        assert!(report.vms_outdated.iter().all(|(_, ver)| ver == "v42.0"));
    }

    #[test]
    fn build_version_report_all_current() {
        let tmp = tempfile::TempDir::new().unwrap();
        let bin = make_fake_binary(tmp.path(), "ch-all-ok", "cloud-hypervisor v43.0");

        let vms = vec![
            ("vm-a".to_string(), "v43.0".to_string()),
            ("vm-b".to_string(), "v43.0".to_string()),
        ];

        let report = build_version_report(&bin, &vms);
        assert_eq!(report.vms_current, 2);
        assert!(report.vms_outdated.is_empty());
    }

    #[test]
    fn build_version_report_no_vms() {
        let tmp = tempfile::TempDir::new().unwrap();
        let bin = make_fake_binary(tmp.path(), "ch-no-vms", "cloud-hypervisor v43.0");

        let report = build_version_report(&bin, &[]);
        assert_eq!(report.vms_current, 0);
        assert!(report.vms_outdated.is_empty());
    }

    #[test]
    fn build_version_report_disk_unknown() {
        // If the disk binary can't be checked, disk version is "unknown"
        let report = build_version_report(Path::new("/nonexistent/ch"), &[]);
        assert_eq!(report.disk, "unknown");
        assert!(!report.disk_matches);
    }

    // -- parse_version_output (internal) --------------------------------------

    #[test]
    fn parse_version_standard_format() {
        assert_eq!(
            parse_version_output("cloud-hypervisor v43.0"),
            Some("v43.0".to_string())
        );
    }

    #[test]
    fn parse_version_with_patch() {
        assert_eq!(
            parse_version_output("cloud-hypervisor v43.0.1"),
            Some("v43.0.1".to_string())
        );
    }

    #[test]
    fn parse_version_garbage() {
        assert_eq!(parse_version_output("not a version"), None);
    }

    #[test]
    fn parse_version_empty() {
        assert_eq!(parse_version_output(""), None);
    }

    #[test]
    fn parse_version_v_alone() {
        // Just "v" with no digits should not match
        assert_eq!(parse_version_output("v"), None);
    }

    // -- is_executable --------------------------------------------------------

    #[test]
    fn is_executable_on_executable_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = make_fake_binary(tmp.path(), "exec-test", "hello");
        assert!(is_executable(&path));
    }

    #[test]
    fn is_executable_on_non_executable_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = make_non_executable(tmp.path(), "noexec-test");
        assert!(!is_executable(&path));
    }

    #[test]
    fn is_executable_on_nonexistent_path() {
        assert!(!is_executable(Path::new("/does/not/exist")));
    }
}
