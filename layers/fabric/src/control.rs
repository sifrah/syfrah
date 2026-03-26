use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use syfrah_api::{auth, transport};
use tokio::net::UnixStream;
use tracing::debug;

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
    RemovePeer {
        name_or_key: String,
    },
    /// Reload config.toml and apply hot-reloadable parameters.
    Reload,
    UpdatePeerEndpoint {
        name_or_key: String,
        endpoint: std::net::SocketAddr,
    },
    /// Rotate the mesh secret: generate a new secret, re-derive keys,
    /// broadcast to all peers, and re-encrypt subsequent announces.
    RotateSecret,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ControlResponse {
    Ok,
    PeeringList {
        requests: Vec<JoinRequestInfo>,
    },
    PeeringAccepted {
        peer_name: String,
    },
    PeerRemoved {
        peer_name: String,
        announced_to: usize,
    },
    PeerEndpointUpdated {
        peer_name: String,
        old_endpoint: String,
        new_endpoint: String,
    },
    Error {
        message: String,
    },
    /// Result of a config reload: lists changed parameters.
    ConfigReloaded {
        changes: Vec<String>,
        skipped: Vec<String>,
    },
    /// Secret rotation completed: returns the new secret and broadcast stats.
    SecretRotated {
        new_secret: String,
        new_ipv6: String,
        peers_notified: usize,
        peers_failed: usize,
    },
}

/// Handler trait for processing control commands. Implemented by the daemon.
#[async_trait::async_trait]
pub trait ControlHandler: Send + Sync {
    async fn handle(&self, req: ControlRequest, caller_uid: Option<u32>) -> ControlResponse;
}

/// Start the Unix domain socket control listener.
pub async fn start_control_listener(socket_path: &Path, handler: Arc<dyn ControlHandler>) {
    let listener = match transport::bind_unix_listener(socket_path) {
        Ok(l) => l,
        Err(_) => return,
    };

    debug!("control socket listening at {}", socket_path.display());

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let peer_uid = auth::get_peer_uid(&stream);

                // Reject unauthorized callers before reading any payload.
                if let Some(uid) = peer_uid {
                    if !auth::authorize_local(uid) {
                        tracing::warn!("rejecting control connection from unauthorized uid {uid}");
                        continue;
                    }
                }

                let handler = handler.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_control_connection(stream, handler, peer_uid).await {
                        debug!("control connection error: {e}");
                    }
                });
            }
            Err(e) => {
                tracing::warn!("control socket accept error: {e}");
            }
        }
    }
}

async fn handle_control_connection(
    mut stream: UnixStream,
    handler: Arc<dyn ControlHandler>,
    caller_uid: Option<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let req = match tokio::time::timeout(
        transport::READ_TIMEOUT,
        transport::read_message(&mut stream),
    )
    .await
    {
        Ok(result) => result?,
        Err(_) => {
            tracing::warn!(
                "control client timed out after {:?}, dropping connection",
                transport::READ_TIMEOUT
            );
            return Err("control read timed out".into());
        }
    };
    let resp = handler.handle(req, caller_uid).await;
    transport::write_message(&mut stream, &resp).await?;
    Ok(())
}

/// Send a control request to the daemon (CLI client side).
pub async fn send_control_request(
    socket_path: &Path,
    req: &ControlRequest,
) -> Result<ControlResponse, Box<dyn std::error::Error>> {
    let mut stream = UnixStream::connect(socket_path).await?;
    transport::write_message(&mut stream, req).await?;
    let resp = transport::read_message(&mut stream).await?;
    Ok(resp)
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
        transport::write_message(&mut client, &req).await.unwrap();
        drop(client);

        let read_req: ControlRequest = transport::read_message(&mut server).await.unwrap();
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

        let fake_len: u32 = 1_000_001;
        tokio::io::AsyncWriteExt::write_all(&mut client, &fake_len.to_be_bytes())
            .await
            .unwrap();
        drop(client);

        let result: Result<ControlRequest, _> = transport::read_message(&mut server).await;
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

        let bad_json = b"not valid json";
        let len = bad_json.len() as u32;
        tokio::io::AsyncWriteExt::write_all(&mut client, &len.to_be_bytes())
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut client, bad_json)
            .await
            .unwrap();
        drop(client);

        let result: Result<ControlRequest, _> = transport::read_message(&mut server).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn control_empty_stream_errors() {
        let (_client, mut server) = duplex(4096);
        drop(_client);

        let result: Result<ControlRequest, _> = transport::read_message(&mut server).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn control_truncated_body_errors() {
        let (mut client, mut server) = duplex(4096);

        let len: u32 = 100;
        tokio::io::AsyncWriteExt::write_all(&mut client, &len.to_be_bytes())
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut client, b"hello")
            .await
            .unwrap();
        drop(client);

        let result: Result<ControlRequest, _> = transport::read_message(&mut server).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn control_response_roundtrip() {
        let (mut client, mut server) = duplex(4096);

        let resp = ControlResponse::PeeringList {
            requests: vec![JoinRequestInfo {
                request_id: "req-1".into(),
                node_name: "node-a".into(),
                wg_public_key: "pk-abc".into(),
                endpoint: "192.168.1.1:7946".parse().unwrap(),
                wg_listen_port: 51820,
                received_at: 0,
                region: None,
                zone: None,
            }],
        };
        transport::write_message(&mut client, &resp).await.unwrap();
        drop(client);

        let read_resp: ControlResponse = transport::read_message(&mut server).await.unwrap();
        match read_resp {
            ControlResponse::PeeringList { requests } => {
                assert_eq!(requests.len(), 1);
                assert_eq!(requests[0].node_name, "node-a");
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn control_slow_client_times_out() {
        tokio::time::pause();

        use std::sync::atomic::{AtomicBool, Ordering};
        use tokio::net::UnixListener;

        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("ctl.sock");

        let handled = Arc::new(AtomicBool::new(false));
        let handled_clone = handled.clone();

        struct NoOpHandler(Arc<AtomicBool>);
        #[async_trait::async_trait]
        impl ControlHandler for NoOpHandler {
            async fn handle(
                &self,
                _req: ControlRequest,
                _caller_uid: Option<u32>,
            ) -> ControlResponse {
                self.0.store(true, Ordering::SeqCst);
                ControlResponse::Ok
            }
        }

        let handler: Arc<dyn ControlHandler> = Arc::new(NoOpHandler(handled_clone));

        let listener = UnixListener::bind(&sock).unwrap();

        let _client = tokio::net::UnixStream::connect(&sock).await.unwrap();

        let (stream, _) = listener.accept().await.unwrap();
        let result = tokio::time::timeout(
            transport::READ_TIMEOUT + std::time::Duration::from_secs(1),
            handle_control_connection(stream, handler, None),
        )
        .await
        .expect("server should complete before outer timeout");

        assert!(result.is_err(), "expected timeout error from slow client");
        assert!(
            result.unwrap_err().to_string().contains("timed out"),
            "error should mention timeout"
        );
        assert!(
            !handled.load(Ordering::SeqCst),
            "handler must not be invoked for timed-out client"
        );
    }
}
