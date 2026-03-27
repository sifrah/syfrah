use std::fmt;

use serde::{Deserialize, Serialize};

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
