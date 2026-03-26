use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use syfrah_api::{auth, transport};
use syfrah_api::{LayerHandler, LayerRequest, LayerResponse, LayerRouter};
use tokio::net::UnixStream;
use tracing::debug;

use crate::peering::JoinRequestInfo;

#[derive(Debug, Serialize, Deserialize)]
pub enum FabricRequest {
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
pub enum FabricResponse {
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

/// Handler trait for processing fabric commands. Implemented by the daemon.
#[async_trait::async_trait]
pub trait FabricHandler: Send + Sync {
    async fn handle(&self, req: FabricRequest, caller_uid: Option<u32>) -> FabricResponse;
}

#[async_trait::async_trait]
impl<T: FabricHandler> FabricHandler for std::sync::Arc<T> {
    async fn handle(&self, req: FabricRequest, caller_uid: Option<u32>) -> FabricResponse {
        (**self).handle(req, caller_uid).await
    }
}

/// Adapter that wraps a [`FabricHandler`] as a [`LayerHandler`], bridging the
/// typed fabric request/response to the opaque byte-level handler interface.
pub struct FabricLayerHandler<H: FabricHandler> {
    inner: H,
}

impl<H: FabricHandler> FabricLayerHandler<H> {
    pub fn new(inner: H) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl<H: FabricHandler + 'static> LayerHandler for FabricLayerHandler<H> {
    async fn handle(&self, request: Vec<u8>, caller_uid: Option<u32>) -> Vec<u8> {
        let req: FabricRequest = match serde_json::from_slice(&request) {
            std::result::Result::Ok(r) => r,
            Err(e) => {
                let resp = FabricResponse::Error {
                    message: format!("invalid fabric request: {e}"),
                };
                return serde_json::to_vec(&resp).unwrap_or_default();
            }
        };
        let resp = self.inner.handle(req, caller_uid).await;
        serde_json::to_vec(&resp).unwrap_or_default()
    }
}

/// Start the Unix domain socket control listener with a [`LayerRouter`].
pub async fn start_control_listener(socket_path: &Path, router: Arc<LayerRouter>) {
    let listener = match transport::bind_unix_listener(socket_path) {
        std::result::Result::Ok(l) => l,
        Err(_) => return,
    };

    debug!("control socket listening at {}", socket_path.display());

    loop {
        match listener.accept().await {
            std::result::Result::Ok((stream, _)) => {
                let peer_uid = auth::get_peer_uid(&stream);

                // Reject unauthorized callers before reading any payload.
                if let Some(uid) = peer_uid {
                    if !auth::authorize_local(uid) {
                        tracing::warn!("rejecting control connection from unauthorized uid {uid}");
                        continue;
                    }
                }

                let router = router.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_control_connection(stream, router, peer_uid).await {
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
    router: Arc<LayerRouter>,
    caller_uid: Option<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let req: LayerRequest = match tokio::time::timeout(
        transport::READ_TIMEOUT,
        transport::read_message(&mut stream),
    )
    .await
    {
        std::result::Result::Ok(result) => result?,
        Err(_) => {
            tracing::warn!(
                "control client timed out after {:?}, dropping connection",
                transport::READ_TIMEOUT
            );
            return Err("control read timed out".into());
        }
    };
    let resp = router.dispatch(req, caller_uid).await;
    transport::write_message(&mut stream, &resp).await?;
    Ok(())
}

/// Send a fabric request to the daemon (CLI client side).
///
/// Wraps the [`FabricRequest`] in a [`LayerRequest::Fabric`] envelope before
/// sending and unwraps the [`LayerResponse::Fabric`] on the way back.
pub async fn send_fabric_request(
    socket_path: &Path,
    req: &FabricRequest,
) -> Result<FabricResponse, Box<dyn std::error::Error>> {
    let payload = serde_json::to_vec(req)?;
    let envelope = LayerRequest::Fabric(payload);

    let mut stream = UnixStream::connect(socket_path).await?;
    transport::write_message(&mut stream, &envelope).await?;
    let resp: LayerResponse = transport::read_message(&mut stream).await?;

    match resp {
        LayerResponse::Fabric(data) => {
            let fabric_resp: FabricResponse = serde_json::from_slice(&data)?;
            Ok(fabric_resp)
        }
        LayerResponse::UnknownLayer(name) => Err(format!("unknown layer: {name}").into()),
    }
}

// Keep the old name as an alias for backward compatibility during migration.
pub use send_fabric_request as send_control_request;

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn fabric_request_roundtrip() {
        let (mut client, mut server) = duplex(4096);

        let req = FabricRequest::PeeringStart {
            port: 7946,
            pin: Some("1234".into()),
        };
        let payload = serde_json::to_vec(&req).unwrap();
        let envelope = LayerRequest::Fabric(payload);
        transport::write_message(&mut client, &envelope)
            .await
            .unwrap();
        drop(client);

        let read_envelope: LayerRequest = transport::read_message(&mut server).await.unwrap();
        match read_envelope {
            LayerRequest::Fabric(data) => {
                let read_req: FabricRequest = serde_json::from_slice(&data).unwrap();
                match read_req {
                    FabricRequest::PeeringStart { port, pin } => {
                        assert_eq!(port, 7946);
                        assert_eq!(pin.as_deref(), Some("1234"));
                    }
                    other => panic!("unexpected request: {other:?}"),
                }
            }
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

        let result: Result<LayerRequest, _> = transport::read_message(&mut server).await;
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

        let result: Result<LayerRequest, _> = transport::read_message(&mut server).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn control_empty_stream_errors() {
        let (_client, mut server) = duplex(4096);
        drop(_client);

        let result: Result<LayerRequest, _> = transport::read_message(&mut server).await;
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

        let result: Result<LayerRequest, _> = transport::read_message(&mut server).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fabric_response_roundtrip() {
        let (mut client, mut server) = duplex(4096);

        let resp = FabricResponse::PeeringList {
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
        let payload = serde_json::to_vec(&resp).unwrap();
        let envelope = LayerResponse::Fabric(payload);
        transport::write_message(&mut client, &envelope)
            .await
            .unwrap();
        drop(client);

        let read_envelope: LayerResponse = transport::read_message(&mut server).await.unwrap();
        match read_envelope {
            LayerResponse::Fabric(data) => {
                let read_resp: FabricResponse = serde_json::from_slice(&data).unwrap();
                match read_resp {
                    FabricResponse::PeeringList { requests } => {
                        assert_eq!(requests.len(), 1);
                        assert_eq!(requests[0].node_name, "node-a");
                    }
                    other => panic!("unexpected response: {other:?}"),
                }
            }
            other => panic!("unexpected envelope: {other:?}"),
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

        struct NoOpFabricHandler(Arc<AtomicBool>);
        #[async_trait::async_trait]
        impl FabricHandler for NoOpFabricHandler {
            async fn handle(
                &self,
                _req: FabricRequest,
                _caller_uid: Option<u32>,
            ) -> FabricResponse {
                self.0.store(true, Ordering::SeqCst);
                FabricResponse::Ok
            }
        }

        let fabric_handler = FabricLayerHandler::new(NoOpFabricHandler(handled_clone));
        let mut router = LayerRouter::new();
        router.register("fabric", Arc::new(fabric_handler));
        let router = Arc::new(router);

        let listener = UnixListener::bind(&sock).unwrap();

        let _client = tokio::net::UnixStream::connect(&sock).await.unwrap();

        let (stream, _) = listener.accept().await.unwrap();
        let result = tokio::time::timeout(
            transport::READ_TIMEOUT + std::time::Duration::from_secs(1),
            handle_control_connection(stream, router, None),
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

    #[tokio::test]
    async fn fabric_layer_handler_dispatches() {
        struct EchoFabric;
        #[async_trait::async_trait]
        impl FabricHandler for EchoFabric {
            async fn handle(
                &self,
                _req: FabricRequest,
                _caller_uid: Option<u32>,
            ) -> FabricResponse {
                FabricResponse::Ok
            }
        }

        let adapter = FabricLayerHandler::new(EchoFabric);
        let req = FabricRequest::PeeringStop;
        let payload = serde_json::to_vec(&req).unwrap();
        let result = LayerHandler::handle(&adapter, payload, None).await;
        let resp: FabricResponse = serde_json::from_slice(&result).unwrap();
        match resp {
            FabricResponse::Ok => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
}
