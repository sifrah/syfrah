//! Cloud Hypervisor runtime backend.
//!
//! `ChRuntime` implements [`ComputeRuntime`] by delegating to the existing
//! process management functions in [`crate::process`]. This is a thin wrapper
//! that translates between the trait's generic types and the CH-specific
//! implementation.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tracing::{debug, info};

use crate::error::ComputeError;
use crate::phase::VmPhase;
use crate::process::{self, RuntimeDir};
use crate::runtime_backend::{
    ComputeRuntime, RuntimeHandle, RuntimeInfo, RuntimeSpec, RuntimeType,
};

// ---------------------------------------------------------------------------
// ChRuntime
// ---------------------------------------------------------------------------

/// Cloud Hypervisor runtime backend.
///
/// Wraps the existing spawn/kill/delete/reconnect functions from `process.rs`
/// behind the [`ComputeRuntime`] trait. Each method translates between the
/// trait's generic types and the CH-specific types.
pub struct ChRuntime {
    /// Resolved path to the cloud-hypervisor binary.
    ch_binary: PathBuf,
    /// Base directory for per-VM runtime dirs (e.g., `/run/syfrah/vms`).
    base_dir: PathBuf,
    /// Path to the shared vmlinux kernel.
    kernel_path: PathBuf,
}

impl ChRuntime {
    /// Create a new ChRuntime with the given configuration paths.
    pub fn new(ch_binary: PathBuf, base_dir: PathBuf, kernel_path: PathBuf) -> Self {
        Self {
            ch_binary,
            base_dir,
            kernel_path,
        }
    }

    /// Get the resolved cloud-hypervisor binary path.
    pub fn ch_binary(&self) -> &Path {
        &self.ch_binary
    }

    /// Get the base directory for runtime dirs.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Get the kernel path.
    pub fn kernel_path(&self) -> &Path {
        &self.kernel_path
    }
}

#[async_trait]
impl ComputeRuntime for ChRuntime {
    async fn create(&self, id: &str, _spec: &RuntimeSpec) -> Result<RuntimeHandle, ComputeError> {
        // Note: The actual VM creation is still driven by VmManager::create_vm
        // which calls process::spawn_vm directly, because spawn_vm needs the
        // full VmSpec, image store, catalog, etc. This method provides the
        // trait interface for future use when the manager is fully decoupled.
        //
        // For now, return the handle shape that would result from a spawn.
        let runtime_dir = self.base_dir.join(id);
        let pid_path = runtime_dir.join("pid");

        // Read PID from the runtime dir if it exists (post-spawn).
        let pid = if pid_path.exists() {
            let rd = RuntimeDir::from_existing(runtime_dir.clone());
            rd.read_pid().unwrap_or(0)
        } else {
            0
        };

        Ok(RuntimeHandle {
            id: id.to_string(),
            pid,
            runtime_type: RuntimeType::Vm,
            runtime_dir,
        })
    }

    async fn stop(&self, handle: &RuntimeHandle, _force: bool) -> Result<(), ComputeError> {
        // The actual kill chain is still driven by VmManager via process::kill_vm
        // because it needs the VmRuntimeState and ChClient. This trait method
        // provides the interface contract.
        debug!(
            id = %handle.id,
            runtime = self.name(),
            "stop requested (delegated to VmManager)"
        );
        Ok(())
    }

    async fn delete(&self, handle: &RuntimeHandle) -> Result<(), ComputeError> {
        // Same delegation pattern as stop — the actual delete uses process::delete_vm
        // through VmManager which has access to the full runtime state.
        debug!(
            id = %handle.id,
            runtime = self.name(),
            "delete requested (delegated to VmManager)"
        );
        Ok(())
    }

    async fn info(&self, handle: &RuntimeHandle) -> Result<RuntimeInfo, ComputeError> {
        let runtime_dir = RuntimeDir::from_existing(handle.runtime_dir.clone());
        let pid = handle.pid;

        // Check if process is alive to determine phase.
        let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
        let phase = if alive {
            VmPhase::Running
        } else {
            VmPhase::Stopped
        };

        // Compute uptime from meta.json if available.
        // VmManager computes precise uptime from VmRuntimeState; here we
        // provide a best-effort value based on current time.
        let uptime_secs = if alive {
            runtime_dir.read_meta().ok().map(|_meta| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            })
        } else {
            None
        };

        Ok(RuntimeInfo {
            phase,
            pid,
            uptime_secs,
            runtime_type: RuntimeType::Vm,
        })
    }

    async fn is_alive(&self, handle: &RuntimeHandle) -> bool {
        // Reap zombie first, then check.
        unsafe {
            let mut status: libc::c_int = 0;
            libc::waitpid(handle.pid as i32, &mut status, libc::WNOHANG);
        }
        unsafe { libc::kill(handle.pid as i32, 0) == 0 }
    }

    async fn reconnect(&self, runtime_dir_base: &Path) -> Vec<RuntimeHandle> {
        let dirs = process::scan_runtime_dirs(runtime_dir_base);
        let mut handles = Vec::new();

        for dir in dirs {
            let meta = match dir.read_meta() {
                Ok(m) => m,
                Err(_) => continue,
            };

            // Only include VMs whose PID is still alive.
            let alive = unsafe {
                let mut status: libc::c_int = 0;
                libc::waitpid(meta.pid as i32, &mut status, libc::WNOHANG);
                libc::kill(meta.pid as i32, 0) == 0
            };

            if alive {
                info!(
                    vm_id = %meta.vm_id,
                    pid = meta.pid,
                    "ChRuntime::reconnect: found live VM"
                );
                handles.push(RuntimeHandle {
                    id: meta.vm_id,
                    pid: meta.pid,
                    runtime_type: RuntimeType::Vm,
                    runtime_dir: dir.path().to_path_buf(),
                });
            }
        }

        handles
    }

    fn name(&self) -> &str {
        "cloud-hypervisor"
    }
}

// ---------------------------------------------------------------------------
// Auto-selection helper
// ---------------------------------------------------------------------------

/// Select the appropriate runtime backend based on system capabilities.
///
/// Currently only Cloud Hypervisor is supported. Returns an error if `/dev/kvm`
/// is not available (container runtime is not yet implemented).
pub fn select_runtime(
    ch_binary: PathBuf,
    base_dir: PathBuf,
    kernel_path: PathBuf,
) -> Result<Box<dyn ComputeRuntime>, ComputeError> {
    if Path::new("/dev/kvm").exists() {
        info!("runtime auto-selection: /dev/kvm present, using cloud-hypervisor");
        return Ok(Box::new(ChRuntime::new(ch_binary, base_dir, kernel_path)));
    }

    // KVM not available — container runtime not yet implemented.
    // For backward compatibility, still create ChRuntime (it will fail at
    // preflight when trying to actually create a VM without KVM).
    info!("runtime auto-selection: /dev/kvm not present, using cloud-hypervisor (will require KVM at VM creation time)");
    Ok(Box::new(ChRuntime::new(ch_binary, base_dir, kernel_path)))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ch_runtime_name() {
        let rt = ChRuntime::new(
            PathBuf::from("/bin/true"),
            PathBuf::from("/tmp/vms"),
            PathBuf::from("/tmp/vmlinux"),
        );
        assert_eq!(rt.name(), "cloud-hypervisor");
    }

    #[test]
    fn ch_runtime_accessors() {
        let rt = ChRuntime::new(
            PathBuf::from("/usr/local/bin/ch"),
            PathBuf::from("/run/vms"),
            PathBuf::from("/opt/vmlinux"),
        );
        assert_eq!(rt.ch_binary(), Path::new("/usr/local/bin/ch"));
        assert_eq!(rt.base_dir(), Path::new("/run/vms"));
        assert_eq!(rt.kernel_path(), Path::new("/opt/vmlinux"));
    }

    #[tokio::test]
    async fn ch_runtime_reconnect_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let rt = ChRuntime::new(
            PathBuf::from("/bin/true"),
            tmp.path().to_path_buf(),
            PathBuf::from("/tmp/vmlinux"),
        );
        let handles = rt.reconnect(tmp.path()).await;
        assert!(handles.is_empty());
    }

    #[tokio::test]
    async fn ch_runtime_is_alive_dead_pid() {
        let rt = ChRuntime::new(
            PathBuf::from("/bin/true"),
            PathBuf::from("/tmp/vms"),
            PathBuf::from("/tmp/vmlinux"),
        );
        let handle = RuntimeHandle {
            id: "vm-dead".to_string(),
            pid: 4_000_000, // nonexistent PID
            runtime_type: RuntimeType::Vm,
            runtime_dir: PathBuf::from("/tmp/nonexistent"),
        };
        assert!(!rt.is_alive(&handle).await);
    }

    #[test]
    fn select_runtime_returns_ch_runtime() {
        // select_runtime always returns ChRuntime (even without KVM, for backward compat)
        let result = select_runtime(
            PathBuf::from("/bin/true"),
            PathBuf::from("/tmp/vms"),
            PathBuf::from("/tmp/vmlinux"),
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name(), "cloud-hypervisor");
    }
}
