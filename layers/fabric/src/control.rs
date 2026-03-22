use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, warn};

use crate::peering::JoinRequestInfo;

#[derive(Debug, Serialize, Deserialize)]
pub enum ControlRequest {
    PeeringStart {
        port: u16,
        pin: Option<String>,
    },
    PeeringStop,
    PeeringList,
    PeeringAccept {
        request_id: String,
    },
    PeeringReject {
        request_id: String,
        reason: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ControlResponse {
    Ok,
    PeeringList { requests: Vec<JoinRequestInfo> },
    PeeringAccepted { peer_name: String },
    Error { message: String },
}

/// Handler trait for processing control commands. Implemented by the daemon.
#[async_trait::async_trait]
pub trait ControlHandler: Send + Sync {
    async fn handle(&self, req: ControlRequest) -> ControlResponse;
}

/// Start the Unix domain socket control listener.
pub async fn start_control_listener(socket_path: &Path, handler: Arc<dyn ControlHandler>) {
    // Remove stale socket
    let _ = std::fs::remove_file(socket_path);

    let listener = match UnixListener::bind(socket_path) {
        Ok(l) => l,
        Err(e) => {
            warn!(
                "failed to bind control socket at {}: {e}",
                socket_path.display()
            );
            return;
        }
    };

    // Restrict permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600));
    }

    debug!("control socket listening at {}", socket_path.display());

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let handler = handler.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_control_connection(stream, handler).await {
                        debug!("control connection error: {e}");
                    }
                });
            }
            Err(e) => {
                warn!("control socket accept error: {e}");
            }
        }
    }
}

async fn handle_control_connection(
    mut stream: UnixStream,
    handler: Arc<dyn ControlHandler>,
) -> Result<(), Box<dyn std::error::Error>> {
    let req = read_control(&mut stream).await?;
    let resp = handler.handle(req).await;
    write_control(&mut stream, &resp).await?;
    Ok(())
}

/// Send a control request to the daemon (CLI client side).
pub async fn send_control_request(
    socket_path: &Path,
    req: &ControlRequest,
) -> Result<ControlResponse, Box<dyn std::error::Error>> {
    let mut stream = UnixStream::connect(socket_path).await?;
    write_control(&mut stream, req).await?;
    let resp = read_control(&mut stream).await?;
    Ok(resp)
}

async fn write_control<T: Serialize>(
    stream: &mut UnixStream,
    msg: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = serde_json::to_vec(msg)?;
    let len = data.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&data).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_control<T: serde::de::DeserializeOwned>(
    stream: &mut UnixStream,
) -> Result<T, Box<dyn std::error::Error>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf);
    if len > 1_000_000 {
        return Err("control message too large".into());
    }
    let mut data = vec![0u8; len as usize];
    stream.read_exact(&mut data).await?;
    let msg: T = serde_json::from_slice(&data)?;
    Ok(msg)
}
