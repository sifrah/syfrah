//! Container runtime backend (crun + gVisor).
//!
//! `ContainerRuntime` implements [`ComputeRuntime`] by generating OCI bundles
//! and delegating to `crun` with gVisor (`runsc`) as the OCI runtime. This
//! provides a sandboxed container environment on hosts where KVM is not
//! available.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::{debug, info, warn};

use crate::error::{ComputeError, ProcessError};
use crate::phase::VmPhase;
use crate::runtime_backend::{
    ComputeRuntime, RuntimeHandle, RuntimeInfo, RuntimeSpec, RuntimeType,
};

// ---------------------------------------------------------------------------
// ContainerMeta — persisted metadata for reconnect
// ---------------------------------------------------------------------------

/// Metadata written to `meta.json` inside each container's runtime directory.
///
/// This mirrors the shape of `VmMeta` from `process.rs` but carries
/// container-specific fields. The `runtime_type` field allows `reconnect` to
/// distinguish container dirs from VM dirs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerMeta {
    pub container_id: String,
    pub created_at: String,
    pub pid: u32,
    pub runtime_type: String,
    pub vcpus: u32,
    pub memory_mb: u32,
}

// ---------------------------------------------------------------------------
// ContainerRuntime
// ---------------------------------------------------------------------------

/// Container runtime backend using crun + gVisor (runsc).
///
/// Each container is an OCI bundle under `base_dir/{id}/` containing a
/// `config.json` and a `rootfs/` directory. The lifecycle is managed through
/// `crun --runtime=/path/to/runsc` commands.
pub struct ContainerRuntime {
    /// Resolved path to the `crun` binary.
    crun_binary: PathBuf,
    /// Resolved path to the `runsc` (gVisor) binary.
    runsc_binary: PathBuf,
    /// Base directory for per-container runtime dirs (e.g., `/run/syfrah/vms`).
    base_dir: PathBuf,
}

impl ContainerRuntime {
    /// Create a new `ContainerRuntime` by resolving `crun` and `runsc` binaries.
    ///
    /// Returns an error if either binary cannot be found.
    pub fn new(base_dir: PathBuf) -> Result<Self, ComputeError> {
        let crun = resolve_binary("crun")?;
        let runsc = resolve_binary("runsc")?;
        Ok(Self {
            crun_binary: crun,
            runsc_binary: runsc,
            base_dir,
        })
    }

    /// Get the resolved crun binary path.
    pub fn crun_binary(&self) -> &Path {
        &self.crun_binary
    }

    /// Get the resolved runsc binary path.
    pub fn runsc_binary(&self) -> &Path {
        &self.runsc_binary
    }

    /// Get the base directory for runtime dirs.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Execute a crun command with `--runtime=<runsc>` prepended.
    ///
    /// Returns stdout on success; maps failures to `ProcessError::SpawnFailed`.
    async fn crun_exec(&self, args: &[&str]) -> Result<String, ComputeError> {
        let output = Command::new(&self.crun_binary)
            .arg(format!("--runtime={}", self.runsc_binary.display()))
            .args(args)
            .output()
            .await
            .map_err(|e| ProcessError::SpawnFailed {
                reason: format!("crun: {e}"),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ProcessError::SpawnFailed {
                reason: format!("crun failed: {stderr}"),
            }
            .into());
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

// ---------------------------------------------------------------------------
// Binary resolution
// ---------------------------------------------------------------------------

/// Resolve a binary by name, checking well-known paths then `$PATH`.
///
/// Resolution order:
/// 1. `/usr/local/bin/{name}`
/// 2. `/usr/bin/{name}`
/// 3. `which {name}` (searches `$PATH`)
fn resolve_binary(name: &str) -> Result<PathBuf, ComputeError> {
    // 1. /usr/local/bin
    let local = PathBuf::from(format!("/usr/local/bin/{name}"));
    if local.exists() {
        return Ok(local);
    }

    // 2. /usr/bin
    let usr = PathBuf::from(format!("/usr/bin/{name}"));
    if usr.exists() {
        return Ok(usr);
    }

    // 3. Search $PATH via `which`
    if let Ok(output) = std::process::Command::new("which").arg(name).output() {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout);
            let path = PathBuf::from(path_str.trim());
            if path.exists() {
                return Ok(path);
            }
        }
    }

    Err(ProcessError::SpawnFailed {
        reason: format!("{name} binary not found in /usr/local/bin/, /usr/bin/, or $PATH"),
    }
    .into())
}

/// Check whether both `crun` and `runsc` binaries are available on this system.
///
/// Used by `select_runtime` to decide whether the container backend can be
/// offered as a fallback when KVM is absent.
pub fn container_binaries_available() -> bool {
    resolve_binary("crun").is_ok() && resolve_binary("runsc").is_ok()
}

// ---------------------------------------------------------------------------
// Auto-download fallback
// ---------------------------------------------------------------------------

/// crun version used for auto-download.
/// Kept in sync with the repo-root `CRUN_VERSION` file.
const CRUN_VERSION: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../CRUN_VERSION"));

/// runsc (gVisor) release version used for auto-download.
/// Kept in sync with the repo-root `RUNSC_VERSION` file.
const RUNSC_VERSION: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../RUNSC_VERSION"));

/// URL template for downloading crun static binaries.
/// `{version}` and `{arch}` are substituted at runtime.
fn crun_download_url() -> String {
    let version = CRUN_VERSION.trim();
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "amd64"
    };
    format!(
        "https://github.com/containers/crun/releases/download/{}/crun-{}-linux-{}",
        version, version, arch
    )
}

/// URL for downloading runsc (gVisor) static binary pinned to a specific release.
fn runsc_download_url() -> String {
    let version = RUNSC_VERSION.trim();
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    format!(
        "https://storage.googleapis.com/gvisor/releases/release/{}/{}/runsc",
        version, arch
    )
}

/// Ensure crun and runsc are available, downloading them if necessary.
///
/// This mirrors the `ensure_kernel` pattern in `boot.rs`. If a binary is
/// missing from the standard paths, it is downloaded to `/usr/local/bin/`.
pub async fn ensure_container_binaries() -> Result<(), ComputeError> {
    for (name, url_fn) in [
        ("crun", crun_download_url as fn() -> String),
        ("runsc", runsc_download_url),
    ] {
        if resolve_binary(name).is_ok() {
            continue;
        }

        let url = url_fn();
        let dest = PathBuf::from(format!("/usr/local/bin/{name}"));
        warn!("{name} not found, downloading from {url}...");

        let dest_clone = dest.clone();
        let name_owned = name.to_string();
        tokio::task::spawn_blocking(move || download_binary(&url, &dest_clone, &name_owned))
            .await
            .map_err(|e| ProcessError::SpawnFailed {
                reason: format!("{name} download task panicked: {e}"),
            })??;

        info!("{name} downloaded and installed to {}", dest.display());
    }
    Ok(())
}

/// Download a binary from `url` to `dest` and make it executable.
fn download_binary(url: &str, dest: &Path, name: &str) -> Result<(), ComputeError> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| ProcessError::SpawnFailed {
            reason: format!("failed to build HTTP client for {name}: {e}"),
        })?;

    let response = client
        .get(url)
        .send()
        .map_err(|e| ProcessError::SpawnFailed {
            reason: format!("failed to download {name} from {url}: {e}"),
        })?;

    if !response.status().is_success() {
        return Err(ProcessError::SpawnFailed {
            reason: format!(
                "failed to download {name} from {url}: HTTP {}",
                response.status()
            ),
        }
        .into());
    }

    let bytes = response.bytes().map_err(|e| ProcessError::SpawnFailed {
        reason: format!("failed to read {name} response body: {e}"),
    })?;

    // Guard against empty or truncated responses
    if bytes.len() < 1024 {
        return Err(ProcessError::SpawnFailed {
            reason: format!(
                "downloaded {name} binary is too small ({} bytes) — possible empty or truncated response",
                bytes.len()
            ),
        }
        .into());
    }

    // Validate ELF magic bytes (0x7f 'E' 'L' 'F')
    if bytes.len() >= 4 && &bytes[..4] != b"\x7fELF" {
        return Err(ProcessError::SpawnFailed {
            reason: format!(
                "downloaded {name} binary is not a valid ELF executable (bad magic bytes)"
            ),
        }
        .into());
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ProcessError::SpawnFailed {
            reason: format!("failed to create directory {}: {e}", parent.display()),
        })?;
    }

    std::fs::write(dest, &bytes).map_err(|e| ProcessError::SpawnFailed {
        reason: format!("failed to write {name} to {}: {e}", dest.display()),
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(dest, perms).map_err(|e| ProcessError::SpawnFailed {
            reason: format!("failed to set permissions on {}: {e}", dest.display()),
        })?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// OCI config.json generation
// ---------------------------------------------------------------------------

/// Generate a minimal OCI runtime spec (`config.json`) for the given workload.
fn generate_oci_config(id: &str, spec: &RuntimeSpec) -> serde_json::Value {
    serde_json::json!({
        "ociVersion": "1.0.0",
        "process": {
            "terminal": false,
            "user": { "uid": 0, "gid": 0 },
            "args": ["/bin/sh", "-c", "exec /sbin/init || exec sleep infinity"],
            "env": [
                "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
            ],
            "cwd": "/"
        },
        "root": {
            "path": "rootfs",
            "readonly": false
        },
        "hostname": id,
        "linux": {
            "resources": {
                "memory": { "limit": (spec.memory_mb as u64) * 1024 * 1024 },
                "cpu": { "shares": spec.vcpus * 1024 }
            },
            "namespaces": [
                { "type": "pid" },
                { "type": "mount" },
                { "type": "ipc" },
                { "type": "uts" },
                { "type": "network" }
            ]
        }
    })
}

// ---------------------------------------------------------------------------
// Helper: extract rootfs from archive
// ---------------------------------------------------------------------------

/// Prepare the rootfs directory for the container.
///
/// - If `rootfs_path` is a directory, use it directly (symlink/bind).
/// - If it is a `.tar.gz` / `.tgz`, extract into `{runtime_dir}/rootfs/`.
/// - Otherwise, return an error (raw disk images are not supported for
///   container mode).
async fn prepare_rootfs(rootfs_path: &Path, runtime_dir: &Path) -> Result<PathBuf, ComputeError> {
    let rootfs_dest = runtime_dir.join("rootfs");

    if rootfs_path.is_dir() {
        // Use the directory directly — symlink so crun sees "rootfs" in the bundle.
        #[cfg(unix)]
        tokio::fs::symlink(rootfs_path, &rootfs_dest)
            .await
            .map_err(|e| ProcessError::SpawnFailed {
                reason: format!(
                    "failed to symlink rootfs {} -> {}: {e}",
                    rootfs_path.display(),
                    rootfs_dest.display()
                ),
            })?;
        return Ok(rootfs_dest);
    }

    let name = rootfs_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        tokio::fs::create_dir_all(&rootfs_dest)
            .await
            .map_err(|e| ProcessError::SpawnFailed {
                reason: format!("failed to create rootfs dir: {e}"),
            })?;

        // Extract using tar (async spawn).
        let output = Command::new("tar")
            .args(["xzf", &rootfs_path.to_string_lossy(), "-C"])
            .arg(&rootfs_dest)
            .output()
            .await
            .map_err(|e| ProcessError::SpawnFailed {
                reason: format!("failed to extract rootfs tar: {e}"),
            })?;

        if !output.status.success() {
            // Wrap raw tar stderr into a user-friendly message instead of
            // leaking internal paths like /opt/syfrah/images/foo-oci.tar.gz.
            let image_name = rootfs_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .trim_end_matches("-oci.tar")
                .trim_end_matches("-oci")
                .to_string();
            return Err(ProcessError::SpawnFailed {
                reason: format!(
                    "Container image format not available for '{image_name}'. \
                     The OCI archive may be corrupt or missing. \
                     Try re-pulling with: syfrah compute image pull {image_name}"
                ),
            }
            .into());
        }

        return Ok(rootfs_dest);
    }

    // .raw or unsupported format — show image name instead of full path.
    let image_name = rootfs_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    Err(ProcessError::SpawnFailed {
        reason: format!(
            "Container image format not available for '{image_name}'. \
             Raw disk images are not supported in container mode — use a .tar.gz OCI archive."
        ),
    }
    .into())
}

// ---------------------------------------------------------------------------
// ComputeRuntime implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl ComputeRuntime for ContainerRuntime {
    async fn create(&self, id: &str, spec: &RuntimeSpec) -> Result<RuntimeHandle, ComputeError> {
        let runtime_dir = self.base_dir.join(id);
        info!(
            container_id = %id,
            runtime_dir = %runtime_dir.display(),
            "ContainerRuntime::create: setting up OCI bundle"
        );

        // 1. Create runtime directory.
        tokio::fs::create_dir_all(&runtime_dir)
            .await
            .map_err(|e| ProcessError::SpawnFailed {
                reason: format!(
                    "failed to create runtime dir {}: {e}",
                    runtime_dir.display()
                ),
            })?;

        // 2. Prepare rootfs.
        prepare_rootfs(&spec.rootfs_path, &runtime_dir).await?;

        // 3. Write config.json.
        let config = generate_oci_config(id, spec);
        let config_path = runtime_dir.join("config.json");
        let config_json =
            serde_json::to_string_pretty(&config).map_err(|e| ProcessError::SpawnFailed {
                reason: format!("failed to serialize OCI config: {e}"),
            })?;
        tokio::fs::write(&config_path, config_json)
            .await
            .map_err(|e| ProcessError::SpawnFailed {
                reason: format!("failed to write config.json: {e}"),
            })?;

        // 4. crun create --bundle {runtime_dir} {id}
        let runtime_dir_str = runtime_dir.to_string_lossy().to_string();
        self.crun_exec(&["create", "--bundle", &runtime_dir_str, id])
            .await?;

        // 5. crun start {id}
        self.crun_exec(&["start", id]).await?;

        // 6. Get PID from crun state.
        let state_json = self.crun_exec(&["state", id]).await?;
        let state: serde_json::Value =
            serde_json::from_str(&state_json).map_err(|e| ProcessError::SpawnFailed {
                reason: format!("failed to parse crun state JSON: {e}"),
            })?;
        let pid = state["pid"].as_u64().unwrap_or(0) as u32;

        // 7. Write meta.json for reconnect.
        let now = chrono_now_iso8601();
        let meta = ContainerMeta {
            container_id: id.to_string(),
            created_at: now,
            pid,
            runtime_type: "container".to_string(),
            vcpus: spec.vcpus,
            memory_mb: spec.memory_mb,
        };
        let meta_json =
            serde_json::to_string_pretty(&meta).map_err(|e| ProcessError::SpawnFailed {
                reason: format!("failed to serialize meta.json: {e}"),
            })?;
        let meta_path = runtime_dir.join("meta.json");
        tokio::fs::write(&meta_path, meta_json)
            .await
            .map_err(|e| ProcessError::SpawnFailed {
                reason: format!("failed to write meta.json: {e}"),
            })?;

        info!(
            container_id = %id,
            pid = pid,
            "ContainerRuntime::create: container started"
        );

        // 8. Return handle.
        Ok(RuntimeHandle {
            id: id.to_string(),
            pid,
            runtime_type: RuntimeType::Container,
            runtime_dir,
        })
    }

    async fn stop(&self, handle: &RuntimeHandle, force: bool) -> Result<(), ComputeError> {
        let signal = if force { "SIGKILL" } else { "SIGTERM" };
        debug!(
            container_id = %handle.id,
            signal = signal,
            "ContainerRuntime::stop"
        );

        self.crun_exec(&["kill", &handle.id, signal]).await?;

        if !force {
            // Poll for the container to exit (up to 30 seconds).
            for _ in 0..30 {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if !self.is_alive(handle).await {
                    return Ok(());
                }
            }
            // Escalate to SIGKILL after timeout.
            warn!(
                container_id = %handle.id,
                "ContainerRuntime::stop: graceful shutdown timed out, sending SIGKILL"
            );
            self.crun_exec(&["kill", &handle.id, "SIGKILL"]).await?;
        }

        Ok(())
    }

    async fn delete(&self, handle: &RuntimeHandle) -> Result<(), ComputeError> {
        debug!(
            container_id = %handle.id,
            "ContainerRuntime::delete"
        );

        // crun delete (force to handle stopped containers).
        // Ignore errors from delete — the container may already be gone.
        let _ = self.crun_exec(&["delete", "--force", &handle.id]).await;

        // Clean up the runtime directory.
        if handle.runtime_dir.exists() {
            tokio::fs::remove_dir_all(&handle.runtime_dir)
                .await
                .map_err(|e| ProcessError::SpawnFailed {
                    reason: format!(
                        "failed to remove runtime dir {}: {e}",
                        handle.runtime_dir.display()
                    ),
                })?;
        }

        info!(
            container_id = %handle.id,
            "ContainerRuntime::delete: container deleted and cleaned up"
        );
        Ok(())
    }

    async fn info(&self, handle: &RuntimeHandle) -> Result<RuntimeInfo, ComputeError> {
        let state_json = self.crun_exec(&["state", &handle.id]).await?;
        let state: serde_json::Value =
            serde_json::from_str(&state_json).map_err(|e| ProcessError::SpawnFailed {
                reason: format!("failed to parse crun state JSON: {e}"),
            })?;

        let status = state["status"].as_str().unwrap_or("unknown");
        let pid = state["pid"].as_u64().unwrap_or(0) as u32;

        let phase = match status {
            "creating" => VmPhase::Pending,
            "created" => VmPhase::Pending,
            "running" => VmPhase::Running,
            "stopped" => VmPhase::Stopped,
            _ => VmPhase::Failed,
        };

        // Best-effort uptime from meta.json created_at timestamp.
        let uptime_secs = if phase == VmPhase::Running {
            read_container_meta(&handle.runtime_dir).ok().and_then(|m| {
                parse_iso8601_to_unix(&m.created_at).map(|created| {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    now.saturating_sub(created)
                })
            })
        } else {
            None
        };

        Ok(RuntimeInfo {
            phase,
            pid,
            uptime_secs,
            runtime_type: RuntimeType::Container,
        })
    }

    async fn is_alive(&self, handle: &RuntimeHandle) -> bool {
        // Try crun state first; fall back to kill(pid, 0).
        if let Ok(state_json) = self.crun_exec(&["state", &handle.id]).await {
            if let Ok(state) = serde_json::from_str::<serde_json::Value>(&state_json) {
                return state["status"].as_str() == Some("running");
            }
        }
        // Fallback: signal check.
        unsafe { libc::kill(handle.pid as i32, 0) == 0 }
    }

    async fn reconnect(&self, runtime_dir_base: &Path) -> Vec<RuntimeHandle> {
        let entries = match std::fs::read_dir(runtime_dir_base) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };

        let mut handles = Vec::new();

        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }

            let meta = match read_container_meta(&dir) {
                Ok(m) => m,
                Err(_) => continue,
            };

            // Only recover containers (skip VM meta.json files).
            if meta.runtime_type != "container" {
                continue;
            }

            // Verify the container is still running.
            let id = &meta.container_id;
            let alive = if let Ok(state_json) = self.crun_exec(&["state", id]).await {
                serde_json::from_str::<serde_json::Value>(&state_json)
                    .map(|s| s["status"].as_str() == Some("running"))
                    .unwrap_or(false)
            } else {
                false
            };

            if alive {
                info!(
                    container_id = %id,
                    pid = meta.pid,
                    "ContainerRuntime::reconnect: found live container"
                );
                handles.push(RuntimeHandle {
                    id: id.clone(),
                    pid: meta.pid,
                    runtime_type: RuntimeType::Container,
                    runtime_dir: dir,
                });
            }
        }

        handles
    }

    fn name(&self) -> &str {
        "container (gVisor)"
    }

    fn health_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if !self.crun_binary.exists() {
            warnings.push("crun binary not found".to_string());
        }
        if !self.runsc_binary.exists() {
            warnings.push("runsc (gVisor) binary not found".to_string());
        }
        warnings
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read `meta.json` from a container runtime directory.
fn read_container_meta(runtime_dir: &Path) -> Result<ContainerMeta, ComputeError> {
    let path = runtime_dir.join("meta.json");
    let data = std::fs::read_to_string(&path).map_err(|e| ProcessError::SpawnFailed {
        reason: format!("failed to read {}: {e}", path.display()),
    })?;
    Ok(
        serde_json::from_str(&data).map_err(|e| ProcessError::SpawnFailed {
            reason: format!("failed to parse {}: {e}", path.display()),
        })?,
    )
}

/// Return the current time as an ISO 8601 string (UTC).
fn chrono_now_iso8601() -> String {
    // Use std time to avoid adding chrono dependency.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Simple ISO 8601-ish format: "1970-01-01T00:00:00Z"
    let secs_per_day: u64 = 86400;
    let days = now / secs_per_day;
    let rem = now % secs_per_day;
    let hours = rem / 3600;
    let minutes = (rem % 3600) / 60;
    let seconds = rem % 60;

    // Approximate date from days since epoch (good enough for metadata).
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from Howard Hinnant's `civil_from_days`.
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u64, m, d)
}

/// Parse a simple ISO 8601 timestamp to Unix epoch seconds (best-effort).
fn parse_iso8601_to_unix(s: &str) -> Option<u64> {
    // Expected: "YYYY-MM-DDThh:mm:ssZ"
    let parts: Vec<&str> = s.split('T').collect();
    if parts.len() != 2 {
        return None;
    }
    let date_parts: Vec<u64> = parts[0].split('-').filter_map(|p| p.parse().ok()).collect();
    let time_str = parts[1].trim_end_matches('Z');
    let time_parts: Vec<u64> = time_str.split(':').filter_map(|p| p.parse().ok()).collect();
    if date_parts.len() != 3 || time_parts.len() != 3 {
        return None;
    }
    // Rough conversion — good enough for uptime calculation.
    let (y, m, d) = (date_parts[0], date_parts[1], date_parts[2]);
    let days = ymd_to_days(y, m, d);
    Some(days * 86400 + time_parts[0] * 3600 + time_parts[1] * 60 + time_parts[2])
}

/// Convert (year, month, day) to days since Unix epoch.
fn ymd_to_days(year: u64, month: u64, day: u64) -> u64 {
    // Inverse of days_to_ymd (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m = month;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe as i64 - 719468;
    days as u64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GpuMode;

    #[test]
    fn generate_oci_config_sets_hostname() {
        let spec = RuntimeSpec {
            vcpus: 2,
            memory_mb: 512,
            rootfs_path: PathBuf::from("/tmp/rootfs"),
            cloud_init_path: None,
            network: None,
            gpu: GpuMode::None,
        };
        let config = generate_oci_config("test-vm-1", &spec);
        assert_eq!(config["hostname"], "test-vm-1");
    }

    #[test]
    fn generate_oci_config_sets_memory_limit() {
        let spec = RuntimeSpec {
            vcpus: 4,
            memory_mb: 2048,
            rootfs_path: PathBuf::from("/tmp/rootfs"),
            cloud_init_path: None,
            network: None,
            gpu: GpuMode::None,
        };
        let config = generate_oci_config("mem-test", &spec);
        let limit = config["linux"]["resources"]["memory"]["limit"]
            .as_u64()
            .unwrap();
        assert_eq!(limit, 2048 * 1024 * 1024);
    }

    #[test]
    fn generate_oci_config_sets_cpu_shares() {
        let spec = RuntimeSpec {
            vcpus: 4,
            memory_mb: 1024,
            rootfs_path: PathBuf::from("/tmp/rootfs"),
            cloud_init_path: None,
            network: None,
            gpu: GpuMode::None,
        };
        let config = generate_oci_config("cpu-test", &spec);
        let shares = config["linux"]["resources"]["cpu"]["shares"]
            .as_u64()
            .unwrap();
        assert_eq!(shares, 4 * 1024);
    }

    #[test]
    fn generate_oci_config_has_namespaces() {
        let spec = RuntimeSpec {
            vcpus: 1,
            memory_mb: 256,
            rootfs_path: PathBuf::from("/tmp/rootfs"),
            cloud_init_path: None,
            network: None,
            gpu: GpuMode::None,
        };
        let config = generate_oci_config("ns-test", &spec);
        let namespaces = config["linux"]["namespaces"].as_array().unwrap();
        assert_eq!(namespaces.len(), 5);
        let ns_types: Vec<&str> = namespaces
            .iter()
            .filter_map(|n| n["type"].as_str())
            .collect();
        assert!(ns_types.contains(&"pid"));
        assert!(ns_types.contains(&"mount"));
        assert!(ns_types.contains(&"network"));
    }

    #[test]
    fn generate_oci_config_oci_version() {
        let spec = RuntimeSpec {
            vcpus: 1,
            memory_mb: 256,
            rootfs_path: PathBuf::from("/tmp/rootfs"),
            cloud_init_path: None,
            network: None,
            gpu: GpuMode::None,
        };
        let config = generate_oci_config("ver-test", &spec);
        assert_eq!(config["ociVersion"], "1.0.0");
    }

    #[test]
    fn container_meta_serde_roundtrip() {
        let meta = ContainerMeta {
            container_id: "ctr-1".to_string(),
            created_at: "2026-01-15T10:30:00Z".to_string(),
            pid: 42,
            runtime_type: "container".to_string(),
            vcpus: 2,
            memory_mb: 512,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let back: ContainerMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(back.container_id, "ctr-1");
        assert_eq!(back.pid, 42);
        assert_eq!(back.runtime_type, "container");
    }

    #[test]
    fn container_runtime_name() {
        // We can't construct ContainerRuntime without binaries on the host,
        // but we can test the name method via a direct struct construction.
        let rt = ContainerRuntime {
            crun_binary: PathBuf::from("/bin/true"),
            runsc_binary: PathBuf::from("/bin/true"),
            base_dir: PathBuf::from("/tmp/vms"),
        };
        assert_eq!(rt.name(), "container (gVisor)");
    }

    #[test]
    fn container_runtime_accessors() {
        let rt = ContainerRuntime {
            crun_binary: PathBuf::from("/usr/local/bin/crun"),
            runsc_binary: PathBuf::from("/usr/local/bin/runsc"),
            base_dir: PathBuf::from("/run/syfrah/vms"),
        };
        assert_eq!(rt.crun_binary(), Path::new("/usr/local/bin/crun"));
        assert_eq!(rt.runsc_binary(), Path::new("/usr/local/bin/runsc"));
        assert_eq!(rt.base_dir(), Path::new("/run/syfrah/vms"));
    }

    #[test]
    fn days_to_ymd_epoch() {
        let (y, m, d) = days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2024-01-01 is day 19723
        let (y, m, d) = days_to_ymd(19723);
        assert_eq!((y, m, d), (2024, 1, 1));
    }

    #[test]
    fn chrono_now_iso8601_format() {
        let ts = chrono_now_iso8601();
        // Should be roughly "YYYY-MM-DDThh:mm:ssZ"
        assert!(ts.ends_with('Z'));
        assert!(ts.contains('T'));
        assert_eq!(ts.len(), 20);
    }

    #[test]
    fn parse_iso8601_roundtrip() {
        let ts = "2026-03-28T12:00:00Z";
        let secs = parse_iso8601_to_unix(ts).unwrap();
        assert!(secs > 0);
    }

    #[test]
    fn parse_iso8601_invalid() {
        assert!(parse_iso8601_to_unix("not-a-date").is_none());
        assert!(parse_iso8601_to_unix("").is_none());
    }

    #[tokio::test]
    async fn container_runtime_reconnect_empty_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rt = ContainerRuntime {
            crun_binary: PathBuf::from("/bin/true"),
            runsc_binary: PathBuf::from("/bin/true"),
            base_dir: tmp.path().to_path_buf(),
        };
        let handles = rt.reconnect(tmp.path()).await;
        assert!(handles.is_empty());
    }

    #[tokio::test]
    async fn container_runtime_is_alive_dead_pid() {
        let rt = ContainerRuntime {
            crun_binary: PathBuf::from("/bin/false"),
            runsc_binary: PathBuf::from("/bin/false"),
            base_dir: PathBuf::from("/tmp/vms"),
        };
        let handle = RuntimeHandle {
            id: "ctr-dead".to_string(),
            pid: 4_000_000, // nonexistent PID
            runtime_type: RuntimeType::Container,
            runtime_dir: PathBuf::from("/tmp/nonexistent"),
        };
        assert!(!rt.is_alive(&handle).await);
    }

    #[test]
    fn container_binaries_not_available_on_ci() {
        // On most CI hosts, crun and runsc are not installed.
        // This test verifies the function doesn't panic.
        let _ = container_binaries_available();
    }

    #[test]
    fn read_container_meta_missing_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = read_container_meta(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn read_container_meta_valid() {
        let tmp = tempfile::TempDir::new().unwrap();
        let meta = ContainerMeta {
            container_id: "ctr-2".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            pid: 100,
            runtime_type: "container".to_string(),
            vcpus: 1,
            memory_mb: 256,
        };
        let json = serde_json::to_string_pretty(&meta).unwrap();
        std::fs::write(tmp.path().join("meta.json"), json).unwrap();

        let loaded = read_container_meta(tmp.path()).unwrap();
        assert_eq!(loaded.container_id, "ctr-2");
        assert_eq!(loaded.pid, 100);
    }
}
