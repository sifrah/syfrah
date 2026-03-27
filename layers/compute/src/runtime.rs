use std::path::PathBuf;

use crate::phase::VmPhase;
use crate::types::{VmId, VmStatus};

/// How this VM's runtime state was established.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) enum ReconnectSource {
    /// The VM was freshly spawned by this daemon instance.
    FreshSpawn,
    /// The VM was already running and recovered after a daemon restart.
    Recovered,
}

/// Compute-internal runtime state for a VM. Never leaked outside the crate.
///
/// This tracks everything compute needs to manage a Cloud Hypervisor process:
/// the OS PID, socket path, cgroup, binary version, health-check timestamps,
/// and the current lifecycle phase.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(crate) struct VmRuntimeState {
    pub(crate) vm_id: VmId,
    pub(crate) pid: u32,
    pub(crate) socket_path: PathBuf,
    pub(crate) cgroup_path: Option<PathBuf>,
    pub(crate) ch_binary_path: PathBuf,
    pub(crate) ch_binary_version: String,
    /// Unix timestamp when the VM was launched.
    pub(crate) launched_at: u64,
    /// Unix timestamp of the last successful health-check ping.
    pub(crate) last_ping_at: Option<u64>,
    /// Last error observed during health checks or lifecycle operations.
    pub(crate) last_error: Option<String>,
    pub(crate) current_phase: VmPhase,
    pub(crate) reconnect_source: ReconnectSource,
}

#[allow(dead_code)]
impl VmRuntimeState {
    /// Produce the public external view of this VM's state.
    ///
    /// This is what forge and the control plane see. Internal details like
    /// PID, socket path, cgroup, and reconnect source are deliberately omitted.
    pub(crate) fn to_status(&self, now_unix: u64) -> VmStatus {
        let uptime_secs = if self.current_phase == VmPhase::Running {
            Some(now_unix.saturating_sub(self.launched_at))
        } else {
            None
        };

        VmStatus {
            vm_id: self.vm_id.clone(),
            phase: self.current_phase,
            vcpus: 0,     // TODO: populate from VmSpec once wired through
            memory_mb: 0, // TODO: populate from VmSpec once wired through
            created_at: Some(self.launched_at),
            uptime_secs,
        }
    }
}
