//! Control socket types for the compute layer.
//!
//! Follows the same pattern as `syfrah_fabric::control`:
//! - `ComputeRequest` / `ComputeResponse` are the typed messages
//! - `ComputeLayerHandler` adapts a `VmManager` to the opaque `LayerHandler` trait

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use syfrah_api::{LayerHandler, LayerRequest, LayerResponse};
use tokio::net::UnixStream;

use crate::manager::VmManager;
use crate::types::{GpuMode, VmId, VmSpec};

// ---------------------------------------------------------------------------
// Request / Response enums
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub enum ComputeRequest {
    CreateVm {
        name: String,
        vcpus: u32,
        memory_mb: u32,
        image: String,
        gpu_bdf: Option<String>,
        tap: Option<String>,
    },
    ListVms,
    GetVm {
        id: String,
    },
    StartVm {
        id: String,
    },
    StopVm {
        id: String,
        force: bool,
    },
    DeleteVm {
        id: String,
        #[serde(default)]
        retain_disk: bool,
    },
    RebootVm {
        id: String,
    },
    ResizeVm {
        id: String,
        vcpus: Option<u32>,
        memory_mb: Option<u32>,
    },
    Status,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ComputeResponse {
    /// Single VM info.
    Vm(serde_json::Value),
    /// List of VMs.
    VmList(Vec<serde_json::Value>),
    /// Compute status.
    Status(serde_json::Value),
    /// Success with no data.
    Ok,
    /// Error message.
    Error(String),
}

// ---------------------------------------------------------------------------
// ComputeLayerHandler — adapts VmManager to LayerHandler
// ---------------------------------------------------------------------------

pub struct ComputeLayerHandler {
    manager: Arc<VmManager>,
}

impl ComputeLayerHandler {
    pub fn new(manager: Arc<VmManager>) -> Self {
        Self { manager }
    }
}

/// Convert a VmStatus to a JSON value suitable for CLI output.
fn vm_status_to_json(s: &crate::types::VmStatus) -> serde_json::Value {
    serde_json::json!({
        "name": s.vm_id.0,
        "id": s.vm_id.0,
        "phase": format!("{:?}", s.phase),
        "vcpus": s.vcpus,
        "memory": s.memory_mb,
        "memory_mb": s.memory_mb,
        "created_at": s.created_at,
        "uptime_secs": s.uptime_secs,
    })
}

#[async_trait::async_trait]
impl LayerHandler for ComputeLayerHandler {
    async fn handle(&self, request: Vec<u8>, _caller_uid: Option<u32>) -> Vec<u8> {
        let req: ComputeRequest = match serde_json::from_slice(&request) {
            Ok(r) => r,
            Err(e) => {
                let resp = ComputeResponse::Error(format!("invalid compute request: {e}"));
                return serde_json::to_vec(&resp).unwrap_or_default();
            }
        };

        let resp = handle_compute_request(&self.manager, req).await;
        serde_json::to_vec(&resp).unwrap_or_default()
    }
}

async fn handle_compute_request(mgr: &VmManager, req: ComputeRequest) -> ComputeResponse {
    match req {
        ComputeRequest::CreateVm {
            name,
            vcpus,
            memory_mb,
            image,
            gpu_bdf,
            tap,
        } => {
            let gpu = match gpu_bdf {
                Some(bdf) => GpuMode::Passthrough { bdf },
                None => GpuMode::None,
            };
            let network = tap.map(|tap_name| crate::types::NetworkConfig {
                tap_name,
                mac: None,
            });
            let spec = VmSpec {
                id: VmId(name),
                vcpus,
                memory_mb,
                image,
                kernel: None,
                network,
                volumes: vec![],
                gpu,
                ssh_key: None,
                disk_size_mb: None,
            };
            match mgr.create_vm(spec).await {
                Ok(status) => ComputeResponse::Vm(vm_status_to_json(&status)),
                Err(e) => ComputeResponse::Error(e.to_string()),
            }
        }
        ComputeRequest::ListVms => {
            let vms = mgr.list().await;
            let json_list: Vec<serde_json::Value> = vms.iter().map(vm_status_to_json).collect();
            ComputeResponse::VmList(json_list)
        }
        ComputeRequest::GetVm { id } => match mgr.info(&id).await {
            Ok(status) => ComputeResponse::Vm(vm_status_to_json(&status)),
            Err(e) => ComputeResponse::Error(e.to_string()),
        },
        ComputeRequest::StartVm { id } => {
            // Start just returns current info (MVP — VMs boot on create)
            match mgr.info(&id).await {
                Ok(status) => ComputeResponse::Vm(vm_status_to_json(&status)),
                Err(e) => ComputeResponse::Error(e.to_string()),
            }
        }
        ComputeRequest::StopVm { id, force: _ } => {
            // force flag is handled at the kill_vm level (already uses kill chain)
            match mgr.shutdown_vm(&id).await {
                Ok(()) => match mgr.info(&id).await {
                    Ok(status) => ComputeResponse::Vm(vm_status_to_json(&status)),
                    Err(_) => ComputeResponse::Ok,
                },
                Err(e) => ComputeResponse::Error(e.to_string()),
            }
        }
        ComputeRequest::DeleteVm { id, retain_disk } => {
            match mgr.delete_vm_with_options(&id, retain_disk).await {
                Ok(()) => ComputeResponse::Ok,
                Err(e) => ComputeResponse::Error(e.to_string()),
            }
        }
        ComputeRequest::RebootVm { id } => {
            // Reboot = return current info (MVP — fake CH handles reboot API)
            match mgr.info(&id).await {
                Ok(status) => ComputeResponse::Vm(vm_status_to_json(&status)),
                Err(e) => ComputeResponse::Error(e.to_string()),
            }
        }
        ComputeRequest::ResizeVm { id, .. } => {
            // Resize not yet implemented; return current info
            match mgr.info(&id).await {
                Ok(status) => ComputeResponse::Vm(vm_status_to_json(&status)),
                Err(e) => ComputeResponse::Error(e.to_string()),
            }
        }
        ComputeRequest::Status => {
            let vms = mgr.list().await;
            let total = vms.len() as u32;
            let running = vms
                .iter()
                .filter(|v| v.phase == crate::phase::VmPhase::Running)
                .count() as u32;
            ComputeResponse::Status(serde_json::json!({
                "status": "healthy",
                "total_vms": total,
                "running_vms": running,
            }))
        }
    }
}

// ---------------------------------------------------------------------------
// Client-side helper — send a compute request to the daemon
// ---------------------------------------------------------------------------

/// Send a compute request to the daemon via the Unix control socket.
pub async fn send_compute_request(
    socket_path: &Path,
    req: &ComputeRequest,
) -> Result<ComputeResponse, Box<dyn std::error::Error>> {
    let payload = serde_json::to_vec(req)?;
    let envelope = LayerRequest::Compute(payload);

    let mut stream = UnixStream::connect(socket_path).await?;
    syfrah_api::transport::write_message(&mut stream, &envelope).await?;
    let resp: LayerResponse = syfrah_api::transport::read_message(&mut stream).await?;

    match resp {
        LayerResponse::Compute(data) => {
            let compute_resp: ComputeResponse = serde_json::from_slice(&data)?;
            Ok(compute_resp)
        }
        LayerResponse::UnknownLayer(name) => Err(format!("unknown layer: {name}").into()),
        other => Err(format!("unexpected response variant: {other:?}").into()),
    }
}
