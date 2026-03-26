use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::handler::LayerHandler;

// ---------------------------------------------------------------------------
// LayerRequest / LayerResponse — the envelope that travels over the socket
// ---------------------------------------------------------------------------

/// Top-level request envelope. Each variant wraps an opaque payload that the
/// corresponding [`LayerHandler`] knows how to deserialise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LayerRequest {
    /// Request destined for the Fabric layer.
    Fabric(Vec<u8>),
}

/// Top-level response envelope returned to the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LayerResponse {
    /// Response originating from the Fabric layer.
    Fabric(Vec<u8>),
    /// The requested layer is not registered in the router.
    UnknownLayer(String),
}

// ---------------------------------------------------------------------------
// LayerRouter
// ---------------------------------------------------------------------------

/// Dispatches incoming [`LayerRequest`]s to the correct [`LayerHandler`].
pub struct LayerRouter {
    handlers: HashMap<String, Arc<dyn LayerHandler>>,
}

impl LayerRouter {
    /// Create an empty router.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a handler for a named layer (e.g. `"fabric"`).
    pub fn register(&mut self, layer: impl Into<String>, handler: Arc<dyn LayerHandler>) {
        self.handlers.insert(layer.into(), handler);
    }

    /// Route a [`LayerRequest`] to the appropriate handler and return a
    /// [`LayerResponse`].
    pub async fn dispatch(&self, request: LayerRequest) -> LayerResponse {
        match request {
            LayerRequest::Fabric(payload) => {
                if let Some(handler) = self.handlers.get("fabric") {
                    LayerResponse::Fabric(handler.handle(payload).await)
                } else {
                    LayerResponse::UnknownLayer("fabric".into())
                }
            }
        }
    }
}

impl Default for LayerRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handler::LayerHandler;

    struct UpperHandler;

    #[async_trait::async_trait]
    impl LayerHandler for UpperHandler {
        async fn handle(&self, request: Vec<u8>) -> Vec<u8> {
            request.iter().map(|b| b.to_ascii_uppercase()).collect()
        }
    }

    #[tokio::test]
    async fn dispatch_to_fabric() {
        let mut router = LayerRouter::new();
        router.register("fabric", Arc::new(UpperHandler));

        let req = LayerRequest::Fabric(b"hello".to_vec());
        let resp = router.dispatch(req).await;

        match resp {
            LayerResponse::Fabric(data) => assert_eq!(data, b"HELLO"),
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_unknown_layer() {
        let router = LayerRouter::new(); // no handlers registered

        let req = LayerRequest::Fabric(b"test".to_vec());
        let resp = router.dispatch(req).await;

        match resp {
            LayerResponse::UnknownLayer(name) => assert_eq!(name, "fabric"),
            other => panic!("unexpected response: {other:?}"),
        }
    }

    #[tokio::test]
    async fn layer_request_serde_roundtrip() {
        let req = LayerRequest::Fabric(b"payload".to_vec());
        let json = serde_json::to_vec(&req).unwrap();
        let back: LayerRequest = serde_json::from_slice(&json).unwrap();
        match back {
            LayerRequest::Fabric(data) => assert_eq!(data, b"payload"),
        }
    }

    #[tokio::test]
    async fn layer_response_serde_roundtrip() {
        let resp = LayerResponse::Fabric(b"result".to_vec());
        let json = serde_json::to_vec(&resp).unwrap();
        let back: LayerResponse = serde_json::from_slice(&json).unwrap();
        match back {
            LayerResponse::Fabric(data) => assert_eq!(data, b"result"),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
