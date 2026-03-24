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

    // Set restrictive umask *before* bind to eliminate the permission race window.
    // The socket is created with mode 0o600 (owner-only) from the start.
    #[cfg(unix)]
    let old_umask = unsafe { libc::umask(0o177) };

    let listener = match UnixListener::bind(socket_path) {
        Ok(l) => l,
        Err(e) => {
            #[cfg(unix)]
            unsafe {
                libc::umask(old_umask);
            }
            warn!(
                "failed to bind control socket at {}: {e}",
                socket_path.display()
            );
            return;
        }
    };

    // Restore the original umask immediately after bind
    #[cfg(unix)]
    unsafe {
        libc::umask(old_umask);
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

/// Write a length-prefixed JSON message to an async writer.
pub async fn write_control<T: Serialize, W: AsyncWriteExt + Unpin>(
    stream: &mut W,
    msg: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    let data = serde_json::to_vec(msg)?;
    let len = data.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&data).await?;
    stream.flush().await?;
    Ok(())
}

/// Read a length-prefixed JSON message from an async reader.
/// Rejects messages larger than 1,000,000 bytes.
pub async fn read_control<T: serde::de::DeserializeOwned, R: AsyncReadExt + Unpin>(
    stream: &mut R,
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn control_roundtrip() {
        let (mut client, mut server) = duplex(4096);

        let req = ControlRequest::PeeringStart {
            port: 7946,
            pin: Some("1234".into()),
        };
        write_control(&mut client, &req).await.unwrap();
        drop(client); // close write end

        let read_req: ControlRequest = read_control(&mut server).await.unwrap();
        match read_req {
            ControlRequest::PeeringStart { port, pin } => {
                assert_eq!(port, 7946);
                assert_eq!(pin.as_deref(), Some("1234"));
            }
            other => panic!("unexpected request: {other:?}"),
        }
    }

    #[tokio::test]
    async fn control_oversized_message_rejected() {
        let (mut client, mut server) = duplex(64);

        // Write a length header claiming >1MB
        let fake_len: u32 = 1_000_001;
        tokio::io::AsyncWriteExt::write_all(&mut client, &fake_len.to_be_bytes())
            .await
            .unwrap();
        drop(client);

        let result: Result<ControlRequest, _> = read_control(&mut server).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("too large"),
            "expected 'too large' error, got: {err_msg}"
        );
    }

    #[tokio::test]
    async fn control_malformed_json_rejected() {
        let (mut client, mut server) = duplex(4096);

        // Write valid length but invalid JSON
        let bad_json = b"not valid json";
        let len = bad_json.len() as u32;
        tokio::io::AsyncWriteExt::write_all(&mut client, &len.to_be_bytes())
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut client, bad_json)
            .await
            .unwrap();
        drop(client);

        let result: Result<ControlRequest, _> = read_control(&mut server).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn control_empty_stream_errors() {
        let (_client, mut server) = duplex(4096);
        drop(_client); // close immediately — empty stream

        let result: Result<ControlRequest, _> = read_control(&mut server).await;
        assert!(result.is_err());
    }
}
