use std::fmt;

use serde::{Deserialize, Serialize};

use crate::phase::VmPhase;

/// Unique identifier for a VM.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct VmId(pub String);

impl fmt::Display for VmId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Desired state of a VM.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct VmSpec {
    pub id: VmId,
    pub vcpus: u32,
    pub memory_mb: u32,
    pub image: String,
    pub kernel: Option<String>,
    pub network: Option<NetworkConfig>,
    pub volumes: Vec<VolumeAttachment>,
    pub gpu: GpuMode,
}

/// TAP device configuration, provided by overlay via forge.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct NetworkConfig {
    pub tap_name: String,
    pub mac: Option<String>,
}

/// Block device attachment.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct VolumeAttachment {
    pub path: String,
    pub read_only: bool,
}

/// GPU mode for a VM.
///
/// No `Shared` variant — virtio-gpu shared rendering is a future consideration.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub enum GpuMode {
    /// No GPU attached.
    #[default]
    None,
    /// VFIO passthrough of a PCI device. `bdf` is the PCI bus:device.function
    /// (e.g., "0000:01:00.0").
    Passthrough { bdf: String },
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
    Created { vm_id: VmId },
    Booted { vm_id: VmId },
    Stopped { vm_id: VmId },
    Crashed { vm_id: VmId, error: String },
    Deleted { vm_id: VmId },
    ReconnectSucceeded { vm_id: VmId },
    ReconnectFailed { vm_id: VmId, error: String },
    VmOrphanCleaned { vm_id: VmId, reason: String },
    Resized { vm_id: VmId, new_vcpus: u32, new_memory_mb: u32 },
    DeviceAttached { vm_id: VmId, device: String },
    DeviceDetached { vm_id: VmId, device: String },
}
