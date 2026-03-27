//! Test utilities for the compute layer.
//!
//! `MockChServer` is a lightweight HTTP/1.1 server on a Unix socket that
//! simulates the Cloud Hypervisor REST API. It is designed to be reusable
//! across client tests and future process-manager tests.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::UnixListener;
use tokio::sync::Notify;

/// A canned response that the mock will return for a given (method, path).
#[derive(Clone)]
pub struct MockResponse {
    pub status: u16,
    pub body: Option<String>,
    /// Optional delay before sending the response (simulates slow CH).
    pub delay: Option<Duration>,
    /// If true, drop the connection without sending a response.
    pub drop_connection: bool,
}

impl MockResponse {
    /// 204 No Content — the standard success for mutating CH endpoints.
    pub fn no_content() -> Self {
        Self {
            status: 204,
            body: None,
            delay: None,
            drop_connection: false,
        }
    }

    /// 200 OK with a JSON body.
    pub fn ok_json(json: &str) -> Self {
        Self {
            status: 200,
            body: Some(json.to_string()),
            delay: None,
            drop_connection: false,
        }
    }

    /// An error response with the given status code.
    pub fn error(status: u16) -> Self {
        Self {
            status,
            body: None,
            delay: None,
            drop_connection: false,
        }
    }

    /// An error response with a body.
    pub fn error_with_body(status: u16, body: &str) -> Self {
        Self {
            status,
            body: Some(body.to_string()),
            delay: None,
            drop_connection: false,
        }
    }

    /// Response that is delayed (to trigger client-side timeout).
    pub fn delayed(delay: Duration) -> Self {
        Self {
            status: 200,
            body: Some("{}".to_string()),
            delay: Some(delay),
            drop_connection: false,
        }
    }

    /// Drop the connection without responding (simulates crash).
    pub fn drop_conn() -> Self {
        Self {
            status: 0,
            body: None,
            delay: None,
            drop_connection: true,
        }
    }
}

type RouteKey = (String, String); // (METHOD, path)
type RouteMap = HashMap<RouteKey, MockResponse>;

/// A lightweight mock HTTP server on a Unix socket.
///
/// # Usage
///
/// ```ignore
/// let mut mock = MockChServer::new(&socket_path);
/// mock.route("GET", "/vmm.ping", MockResponse::ok_json(r#"{"pong":true}"#));
/// mock.start().await;
/// // ... use ChClient pointing at socket_path ...
/// mock.shutdown();
/// ```
pub struct MockChServer {
    socket_path: PathBuf,
    routes: RouteMap,
    /// Captured request bodies, keyed by (method, path).
    captured_bodies: Arc<tokio::sync::Mutex<HashMap<RouteKey, String>>>,
    shutdown: Arc<Notify>,
}

impl MockChServer {
    /// Create a new mock server bound to the given socket path.
    pub fn new(socket_path: &Path) -> Self {
        Self {
            socket_path: socket_path.to_path_buf(),
            routes: HashMap::new(),
            captured_bodies: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            shutdown: Arc::new(Notify::new()),
        }
    }

    /// Register a canned response for a (method, path) pair.
    pub fn route(&mut self, method: &str, path: &str, response: MockResponse) {
        self.routes
            .insert((method.to_string(), path.to_string()), response);
    }

    /// Start accepting connections in the background.
    pub async fn start(&self) {
        // Remove stale socket if it exists.
        let _ = std::fs::remove_file(&self.socket_path);

        let listener =
            UnixListener::bind(&self.socket_path).expect("failed to bind mock Unix socket");

        let routes = Arc::new(self.routes.clone());
        let captured = self.captured_bodies.clone();
        let shutdown = self.shutdown.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept = listener.accept() => {
                        match accept {
                            Ok((stream, _)) => {
                                let routes = routes.clone();
                                let captured = captured.clone();
                                tokio::spawn(async move {
                                    let io = TokioIo::new(stream);
                                    let svc = service_fn(move |req: Request<hyper::body::Incoming>| {
                                        let routes = routes.clone();
                                        let captured = captured.clone();
                                        async move {
                                            handle_request(req, &routes, &captured).await
                                        }
                                    });
                                    let _ = http1::Builder::new()
                                        .serve_connection(io, svc)
                                        .await;
                                });
                            }
                            Err(_) => break,
                        }
                    }
                    _ = shutdown.notified() => break,
                }
            }
        });

        // Give the listener a moment to be ready.
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    /// Stop the server.
    pub fn shutdown(&self) {
        self.shutdown.notify_one();
    }

    /// Retrieve the captured request body for a given (method, path).
    pub async fn captured_body(&self, method: &str, path: &str) -> Option<String> {
        let guard = self.captured_bodies.lock().await;
        guard.get(&(method.to_string(), path.to_string())).cloned()
    }
}

async fn handle_request(
    req: Request<hyper::body::Incoming>,
    routes: &RouteMap,
    captured: &Arc<tokio::sync::Mutex<HashMap<RouteKey, String>>>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    use http_body_util::BodyExt;

    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let key = (method.to_string(), path.clone());

    // Capture request body.
    let body_bytes = req.into_body().collect().await?.to_bytes();
    if !body_bytes.is_empty() {
        let body_str = String::from_utf8_lossy(&body_bytes).to_string();
        captured.lock().await.insert(key.clone(), body_str);
    }

    let mock_resp = routes.get(&key);

    match mock_resp {
        Some(resp) if resp.drop_connection => {
            // Return a normal response but we want to simulate a drop.
            // The simplest approach: just stall forever, the test timeout handles it.
            tokio::time::sleep(Duration::from_secs(3600)).await;
            Ok(Response::builder()
                .status(500)
                .body(Full::new(Bytes::new()))
                .unwrap())
        }
        Some(resp) => {
            if let Some(delay) = resp.delay {
                tokio::time::sleep(delay).await;
            }
            let status = StatusCode::from_u16(resp.status).unwrap_or(StatusCode::OK);
            let body = resp.body.clone().unwrap_or_default();
            Ok(Response::builder()
                .status(status)
                .header("Content-Type", "application/json")
                .body(Full::new(Bytes::from(body)))
                .unwrap())
        }
        None => {
            // Unregistered route: 404.
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Full::new(Bytes::from("{\"error\":\"not found\"}")))
                .unwrap())
        }
    }
}
