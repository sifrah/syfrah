use std::fmt;

use serde::{Deserialize, Serialize};

use crate::phase::VmPhase;

/// Unique identifier for a VM.
///
/// TODO: Will be extended with validation once the full types module lands.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct VmId(pub String);

impl fmt::Display for VmId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// External view of a VM, exposed to forge and other layers.
///
/// This is what forge and the control plane see when querying VM state.
/// Internal details (PID, socket path, cgroup) are deliberately omitted.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VmStatus {
    pub vm_id: VmId,
    pub phase: VmPhase,
    pub vcpus: u32,
    pub memory_mb: u32,
    /// Unix timestamp of when the VM was created.
    pub created_at: Option<u64>,
    /// Seconds the VM has been running. `None` if not in the `Running` phase.
    pub uptime_secs: Option<u64>,
}

/// Observable events emitted to forge via a broadcast channel.
///
/// Delivery is best-effort, real-time. The source of truth for VM state is
/// always `info()` / `status()`, never the event stream alone.
#[derive(Clone, Debug)]
pub enum VmEvent {
    Created {
        vm_id: VmId,
    },
    Booted {
        vm_id: VmId,
    },
    Stopped {
        vm_id: VmId,
    },
    Crashed {
        vm_id: VmId,
        error: String,
    },
    Deleted {
        vm_id: VmId,
    },
    ReconnectSucceeded {
        vm_id: VmId,
    },
    ReconnectFailed {
        vm_id: VmId,
        error: String,
    },
    VmOrphanCleaned {
        vm_id: VmId,
        reason: String,
    },
    Resized {
        vm_id: VmId,
        new_vcpus: u32,
        new_memory_mb: u32,
    },
    DeviceAttached {
        vm_id: VmId,
        device: String,
    },
    DeviceDetached {
        vm_id: VmId,
        device: String,
    },
}
