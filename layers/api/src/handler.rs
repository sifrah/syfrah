/// Trait that each layer implements to handle incoming requests.
///
/// The daemon registers one `LayerHandler` per layer. Requests and responses
/// are opaque byte vectors — the layer is responsible for deserialising the
/// request and serialising the response (typically via `serde_json`).
#[async_trait::async_trait]
pub trait LayerHandler: Send + Sync {
    /// Process a serialised request and return a serialised response.
    async fn handle(&self, request: Vec<u8>) -> Vec<u8>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct EchoHandler;

    #[async_trait::async_trait]
    impl LayerHandler for EchoHandler {
        async fn handle(&self, request: Vec<u8>) -> Vec<u8> {
            request
        }
    }

    #[tokio::test]
    async fn echo_handler_returns_input() {
        let handler: Arc<dyn LayerHandler> = Arc::new(EchoHandler);
        let input = b"hello".to_vec();
        let output = handler.handle(input.clone()).await;
        assert_eq!(input, output);
    }
}
