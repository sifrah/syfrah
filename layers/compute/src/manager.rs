use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, Mutex, RwLock};
use tracing::info;

use crate::client::ChClient;
use crate::error::{ComputeError, ProcessError};
use crate::process::{self, ReconnectReport, RuntimeDir};
use crate::runtime::VmRuntimeState;
use crate::types::{VmEvent, VmId, VmSpec, VmStatus};

// ---------------------------------------------------------------------------
// ComputeConfig
// ---------------------------------------------------------------------------

/// Configuration for the compute layer.
///
/// All paths have sensible defaults for standard installations. Override via
/// `ComputeConfig { ch_binary: Some(...), ..Default::default() }`.
pub struct ComputeConfig {
    /// Base directory for per-VM runtime dirs. Default: `/run/syfrah/vms`.
    pub base_dir: PathBuf,
    /// Directory containing VM root filesystem images. Default: `/opt/syfrah/images`.
    pub image_dir: PathBuf,
    /// Path to the shared vmlinux kernel. Default: `/opt/syfrah/vmlinux`.
    pub kernel_path: PathBuf,
    /// Explicit path to the cloud-hypervisor binary. `None` = auto-resolve.
    pub ch_binary: Option<PathBuf>,
    /// Interval between health-check iterations (seconds). Default: 5.
    pub monitor_interval_secs: u64,
    /// Timeout for graceful shutdown before escalating (seconds). Default: 30.
    pub shutdown_timeout_secs: u64,
}

impl Default for ComputeConfig {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::from("/run/syfrah/vms"),
            image_dir: PathBuf::from("/opt/syfrah/images"),
            kernel_path: PathBuf::from("/opt/syfrah/vmlinux"),
            ch_binary: None,
            monitor_interval_secs: 5,
            shutdown_timeout_secs: 30,
        }
    }
}

// ---------------------------------------------------------------------------
// Binary resolution
// ---------------------------------------------------------------------------

/// Resolve the cloud-hypervisor binary path.
///
/// Resolution order (per README):
/// 1. Explicit path from config (if provided and exists)
/// 2. `/usr/local/lib/syfrah/cloud-hypervisor`
/// 3. `cloud-hypervisor` on `$PATH` via `which`
fn resolve_ch_binary(explicit: Option<&Path>) -> Result<PathBuf, ComputeError> {
    // 1. Explicit config value
    if let Some(path) = explicit {
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        return Err(ProcessError::SpawnFailed {
            reason: format!(
                "configured cloud-hypervisor binary not found: {}",
                path.display()
            ),
        }
        .into());
    }

    // 2. Standard installation path
    let installed = PathBuf::from("/usr/local/lib/syfrah/cloud-hypervisor");
    if installed.exists() {
        return Ok(installed);
    }

    // 3. Search $PATH
    if let Ok(output) = std::process::Command::new("which")
        .arg("cloud-hypervisor")
        .output()
    {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout);
            let path = PathBuf::from(path_str.trim());
            if path.exists() {
                return Ok(path);
            }
        }
    }

    Err(ProcessError::SpawnFailed {
        reason: "cloud-hypervisor binary not found in /usr/local/lib/syfrah/ or $PATH".to_string(),
    }
    .into())
}

// ---------------------------------------------------------------------------
// now_unix helper (same as process.rs, kept minimal to avoid pub export)
// ---------------------------------------------------------------------------

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// VmManager
// ---------------------------------------------------------------------------

/// Top-level entry point for the compute layer.
///
/// `VmManager` is the single public interface that forge uses. It wraps
/// `spawn_vm`, `kill_vm`, `delete_vm`, `reconnect`, and `monitor_loop`
/// behind a concurrent `HashMap` with per-VM `Mutex`.
///
/// ## Concurrency model
///
/// - The `vms` map is protected by an `RwLock` (read for list/info, write for
///   create/delete).
/// - Each VM's `VmRuntimeState` is wrapped in `Arc<Mutex<_>>`.
/// - Operations on the **same** VM are serialized via the VM's Mutex.
/// - Operations on **different** VMs run in parallel.
/// - The monitor loop uses `try_lock` to skip busy VMs.
///
/// **MVP limitation:** long operations (e.g., 30s graceful shutdown) block
/// concurrent ops on the same VM. Future: command-in-progress model.
pub struct VmManager {
    config: ComputeConfig,
    /// Resolved cloud-hypervisor binary path (validated at construction).
    ch_binary: PathBuf,
    /// Per-VM runtime state, keyed by VM ID string.
    vms: Arc<RwLock<HashMap<String, Arc<Mutex<VmRuntimeState>>>>>,
    /// Broadcast channel for lifecycle events consumed by forge.
    event_tx: broadcast::Sender<VmEvent>,
}

impl VmManager {
    /// Create a new `VmManager` with the given configuration.
    ///
    /// Resolves the cloud-hypervisor binary at construction time so that
    /// misconfiguration is caught early (before any VM operations).
    pub fn new(config: ComputeConfig) -> Result<Self, ComputeError> {
        let ch_binary = resolve_ch_binary(config.ch_binary.as_deref())?;
        info!(ch_binary = %ch_binary.display(), "VmManager: resolved cloud-hypervisor binary");

        let (event_tx, _) = broadcast::channel(256);

        Ok(Self {
            config,
            ch_binary,
            vms: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
        })
    }

    // -- Lifecycle operations -------------------------------------------------

    /// Create and boot a new VM.
    ///
    /// 1. Checks that no VM with the same ID already exists.
    /// 2. Calls `process::spawn_vm` (validate, resolve, preflight, spawn, boot).
    /// 3. Inserts the runtime state into the map with `Arc<Mutex<_>>`.
    /// 4. Emits `Created` and `Booted` events.
    pub async fn create_vm(&self, spec: VmSpec) -> Result<VmStatus, ComputeError> {
        let vm_id_str = spec.id.0.clone();

        // Check for duplicates under a brief read lock.
        {
            let map = self.vms.read().await;
            if map.contains_key(&vm_id_str) {
                return Err(ProcessError::SpawnFailed {
                    reason: format!("VM {vm_id_str} already exists"),
                }
                .into());
            }
        }

        // Spawn (this is the heavy part — runs outside any lock on the map).
        let state = process::spawn_vm(
            &spec,
            &self.ch_binary,
            &self.config.base_dir,
            &self.config.image_dir,
            &self.config.kernel_path,
        )
        .await?;

        let now = now_unix();
        let status = state.to_status(now);

        // Insert into the map under a write lock.
        {
            let mut map = self.vms.write().await;
            map.insert(vm_id_str.clone(), Arc::new(Mutex::new(state)));
        }

        // Emit events (best-effort — receivers may lag).
        let _ = self.event_tx.send(VmEvent::Created {
            vm_id: VmId(vm_id_str.clone()),
        });
        let _ = self.event_tx.send(VmEvent::Booted {
            vm_id: VmId(vm_id_str),
        });

        Ok(status)
    }

    /// Shut down a running VM via the 4-level kill chain.
    ///
    /// Acquires the VM's mutex, calls `process::kill_vm`, and emits a
    /// `Stopped` event on success.
    pub async fn shutdown_vm(&self, id: &str) -> Result<(), ComputeError> {
        let vm_arc = self.get_vm(id).await?;
        let mut guard = vm_arc.lock().await;

        let runtime_dir = RuntimeDir::from_existing(self.config.base_dir.join(id));
        let client = ChClient::new(guard.socket_path.clone());

        process::kill_vm(&mut guard, &client, &runtime_dir).await?;

        let _ = self.event_tx.send(VmEvent::Stopped {
            vm_id: VmId(id.to_string()),
        });

        Ok(())
    }

    /// Delete a VM: stop if running, clean up all artifacts, remove from map.
    ///
    /// Acquires the VM's mutex, calls `process::delete_vm`, removes the entry
    /// from the map, and emits a `Deleted` event.
    pub async fn delete_vm(&self, id: &str) -> Result<(), ComputeError> {
        let vm_arc = self.get_vm(id).await?;
        let mut guard = vm_arc.lock().await;

        let runtime_dir = RuntimeDir::from_existing(self.config.base_dir.join(id));
        let client = ChClient::new(guard.socket_path.clone());

        process::delete_vm(&mut guard, &client, &runtime_dir).await?;

        // Drop the guard before acquiring the write lock on the map.
        drop(guard);

        {
            let mut map = self.vms.write().await;
            map.remove(id);
        }

        let _ = self.event_tx.send(VmEvent::Deleted {
            vm_id: VmId(id.to_string()),
        });

        Ok(())
    }

    /// Get the external status of a single VM.
    pub async fn info(&self, id: &str) -> Result<VmStatus, ComputeError> {
        let vm_arc = self.get_vm(id).await?;
        let guard = vm_arc.lock().await;
        Ok(guard.to_status(now_unix()))
    }

    /// List the status of all tracked VMs.
    ///
    /// Takes a read lock on the map, then acquires each VM's mutex in turn
    /// to produce its `VmStatus`.
    pub async fn list(&self) -> Vec<VmStatus> {
        let snapshot: Vec<(String, Arc<Mutex<VmRuntimeState>>)> = {
            let map = self.vms.read().await;
            map.iter()
                .map(|(k, v)| (k.clone(), Arc::clone(v)))
                .collect()
        };

        let now = now_unix();
        let mut results = Vec::with_capacity(snapshot.len());
        for (_id, vm_arc) in snapshot {
            let guard = vm_arc.lock().await;
            results.push(guard.to_status(now));
        }
        results
    }

    // -- Reconnect ------------------------------------------------------------

    /// Scan runtime dirs and recover VMs that survived a daemon restart.
    ///
    /// Calls `process::reconnect`, inserts recovered VMs into the map, and
    /// returns the reconnect report (recovered / failed / orphans cleaned).
    pub async fn reconnect(&self) -> Result<ReconnectReport, ComputeError> {
        let report = process::reconnect(&self.config.base_dir, self.event_tx.clone()).await;

        // Insert recovered VMs into the map.
        if !report.recovered.is_empty() {
            let mut map = self.vms.write().await;
            for state in &report.recovered {
                let id = state.vm_id.0.clone();
                map.insert(id, Arc::new(Mutex::new(state.clone())));
            }
        }

        info!(
            recovered = report.recovered.len(),
            failed = report.failed.len(),
            orphans = report.orphans_cleaned.len(),
            "VmManager: reconnect complete"
        );

        Ok(report)
    }

    // -- Events ---------------------------------------------------------------

    /// Subscribe to the lifecycle event broadcast channel.
    ///
    /// Returns a `Receiver` that will get all events emitted after this call.
    /// Slow consumers may miss events (broadcast channel drops old messages
    /// when the buffer is full).
    pub fn subscribe(&self) -> broadcast::Receiver<VmEvent> {
        self.event_tx.subscribe()
    }

    // -- Monitor --------------------------------------------------------------

    /// Start the background health-check loop.
    ///
    /// Spawns `process::monitor_loop` as a detached tokio task. The loop runs
    /// until the runtime shuts down.
    pub fn start_monitor(&self) {
        let vms = Arc::clone(&self.vms);
        let event_tx = self.event_tx.clone();
        let interval = Duration::from_secs(self.config.monitor_interval_secs);

        tokio::spawn(async move {
            process::monitor_loop(vms, event_tx, interval).await;
        });

        info!(
            interval_secs = self.config.monitor_interval_secs,
            "VmManager: started monitor loop"
        );
    }

    // -- Internal helpers -----------------------------------------------------

    /// Look up a VM by ID, returning its `Arc<Mutex<VmRuntimeState>>`.
    ///
    /// Returns `ProcessError::PidNotFound`-style error if the VM is unknown.
    async fn get_vm(&self, id: &str) -> Result<Arc<Mutex<VmRuntimeState>>, ComputeError> {
        let map = self.vms.read().await;
        map.get(id).cloned().ok_or_else(|| {
            ProcessError::SpawnFailed {
                reason: format!("VM {id} not found"),
            }
            .into()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sensible_paths() {
        let cfg = ComputeConfig::default();
        assert_eq!(cfg.base_dir, PathBuf::from("/run/syfrah/vms"));
        assert_eq!(cfg.image_dir, PathBuf::from("/opt/syfrah/images"));
        assert_eq!(cfg.kernel_path, PathBuf::from("/opt/syfrah/vmlinux"));
        assert!(cfg.ch_binary.is_none());
        assert_eq!(cfg.monitor_interval_secs, 5);
        assert_eq!(cfg.shutdown_timeout_secs, 30);
    }

    #[test]
    fn resolve_ch_binary_fails_on_missing_explicit_path() {
        let result = resolve_ch_binary(Some(Path::new("/nonexistent/ch-binary")));
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("not found"));
    }
}
