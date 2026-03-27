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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn vm_id_serde_roundtrip() {
        let id = VmId("vm-abc-123".to_string());
        let json = serde_json::to_string(&id).unwrap();
        let back: VmId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn vm_id_display() {
        let id = VmId("vm-test-42".to_string());
        assert_eq!(id.to_string(), "vm-test-42");
    }

    #[test]
    fn vm_id_as_hashmap_key() {
        let mut map = HashMap::new();
        let id = VmId("vm-1".to_string());
        map.insert(id.clone(), 42u64);
        assert_eq!(map.get(&id), Some(&42));
    }

    #[test]
    fn vm_spec_serde_roundtrip_full() {
        let spec = VmSpec {
            id: VmId("vm-full".to_string()),
            vcpus: 4,
            memory_mb: 8192,
            image: "ubuntu-22.04.qcow2".to_string(),
            kernel: Some("/boot/vmlinuz".to_string()),
            network: Some(NetworkConfig {
                tap_name: "tap0".to_string(),
                mac: Some("52:54:00:12:34:56".to_string()),
            }),
            volumes: vec![VolumeAttachment {
                path: "/dev/sda1".to_string(),
                read_only: false,
            }],
            gpu: GpuMode::Passthrough {
                bdf: "0000:01:00.0".to_string(),
            },
        };
        let json = serde_json::to_string(&spec).unwrap();
        let back: VmSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(spec, back);
    }

    #[test]
    fn vm_spec_serde_roundtrip_minimal() {
        let spec = VmSpec {
            id: VmId("vm-min".to_string()),
            vcpus: 1,
            memory_mb: 512,
            image: "alpine.qcow2".to_string(),
            kernel: None,
            network: None,
            volumes: vec![],
            gpu: GpuMode::None,
        };
        let json = serde_json::to_string(&spec).unwrap();
        let back: VmSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(spec, back);
    }

    #[test]
    fn gpu_mode_none_serde_roundtrip() {
        let mode = GpuMode::None;
        let json = serde_json::to_string(&mode).unwrap();
        let back: GpuMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, back);
    }

    #[test]
    fn gpu_mode_passthrough_serde_roundtrip() {
        let mode = GpuMode::Passthrough {
            bdf: "0000:41:00.0".to_string(),
        };
        let json = serde_json::to_string(&mode).unwrap();
        let back: GpuMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, back);
        if let GpuMode::Passthrough { bdf } = &back {
            assert_eq!(bdf, "0000:41:00.0");
        } else {
            panic!("expected Passthrough variant");
        }
    }

    #[test]
    fn gpu_mode_default_is_none() {
        assert_eq!(GpuMode::default(), GpuMode::None);
    }

    #[test]
    fn network_config_serde_roundtrip() {
        let cfg = NetworkConfig {
            tap_name: "tap7".to_string(),
            mac: Some("aa:bb:cc:dd:ee:ff".to_string()),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: NetworkConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn volume_attachment_serde_roundtrip() {
        let vol = VolumeAttachment {
            path: "/mnt/data".to_string(),
            read_only: true,
        };
        let json = serde_json::to_string(&vol).unwrap();
        let back: VolumeAttachment = serde_json::from_str(&json).unwrap();
        assert_eq!(vol, back);
    }

    #[test]
    fn vm_status_serde_roundtrip() {
        let status = VmStatus {
            vm_id: VmId("vm-status-1".to_string()),
            phase: VmPhase::Running,
            vcpus: 2,
            memory_mb: 4096,
            created_at: Some(1700000000),
            uptime_secs: Some(3600),
        };
        let json = serde_json::to_string(&status).unwrap();
        let back: VmStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back.vm_id, status.vm_id);
        assert_eq!(back.phase, VmPhase::Running);
        assert_eq!(back.vcpus, 2);
        assert_eq!(back.uptime_secs, Some(3600));
    }

    #[test]
    fn vm_event_created_variant() {
        let event = VmEvent::Created {
            vm_id: VmId("vm-e1".to_string()),
        };
        let cloned = event.clone();
        assert!(format!("{cloned:?}").contains("Created"));
    }

    #[test]
    fn vm_event_crashed_carries_error() {
        let event = VmEvent::Crashed {
            vm_id: VmId("vm-crash".to_string()),
            error: "out of memory".to_string(),
        };
        let debug = format!("{event:?}");
        assert!(debug.contains("out of memory"));
    }

    #[test]
    fn vm_event_resized_carries_new_values() {
        let event = VmEvent::Resized {
            vm_id: VmId("vm-resize".to_string()),
            new_vcpus: 8,
            new_memory_mb: 16384,
        };
        if let VmEvent::Resized {
            new_vcpus,
            new_memory_mb,
            ..
        } = event
        {
            assert_eq!(new_vcpus, 8);
            assert_eq!(new_memory_mb, 16384);
        } else {
            panic!("expected Resized");
        }
    }
}
