use std::path::PathBuf;
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::client::conn;
use hyper::Request;
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;
use tracing::debug;

use crate::error::ClientError;

/// HTTP client for the Cloud Hypervisor REST API over a Unix socket.
///
/// Each VM exposes its own socket at `/run/syfrah/vms/{id}/api.sock`.
/// `ChClient` wraps one socket path and provides typed methods for every
/// CH endpoint that Syfrah uses.
pub struct ChClient {
    socket_path: PathBuf,
    timeout: Duration,
}

/// Default request timeout (10 seconds).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

impl ChClient {
    /// Create a new client for the given socket path with the default 10s timeout.
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Create a new client with a custom timeout.
    pub fn with_timeout(socket_path: PathBuf, timeout: Duration) -> Self {
        Self {
            socket_path,
            timeout,
        }
    }

    /// Send an HTTP request over the Unix socket.
    ///
    /// Returns `Some(json)` for 200 responses with a body, `None` for 204 (no content).
    /// Non-2xx responses are returned as errors unless the caller handles them
    /// via idempotence logic.
    async fn request(
        &self,
        method: hyper::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<(u16, Option<serde_json::Value>), ClientError> {
        // Check socket exists before attempting connection.
        if !self.socket_path.exists() {
            return Err(ClientError::SocketNotFound {
                path: self.socket_path.display().to_string(),
            });
        }

        let result = tokio::time::timeout(self.timeout, self.do_request(method, path, body)).await;

        match result {
            Ok(inner) => inner,
            Err(_) => Err(ClientError::Timeout {
                operation: path.to_string(),
            }),
        }
    }

    /// Perform the actual HTTP request (no timeout wrapper).
    async fn do_request(
        &self,
        method: hyper::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<(u16, Option<serde_json::Value>), ClientError> {
        let stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::ConnectionRefused {
                ClientError::ConnectionRefused
            } else if e.kind() == std::io::ErrorKind::NotFound {
                ClientError::SocketNotFound {
                    path: self.socket_path.display().to_string(),
                }
            } else {
                ClientError::ConnectionRefused
            }
        })?;

        let io = TokioIo::new(stream);
        let (mut sender, connection) =
            conn::http1::handshake(io)
                .await
                .map_err(|_| ClientError::InvalidResponse {
                    detail: "HTTP handshake failed".to_string(),
                })?;

        // Spawn the connection driver so it processes in the background.
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                debug!("connection driver error: {e}");
            }
        });

        let req_body = match &body {
            Some(json) => Full::new(Bytes::from(
                serde_json::to_vec(json).expect("JSON serialization cannot fail"),
            )),
            None => Full::new(Bytes::new()),
        };

        let mut builder = Request::builder()
            .method(method)
            .uri(format!("http://localhost{path}"))
            .header("Host", "localhost");

        if body.is_some() {
            builder = builder.header("Content-Type", "application/json");
        }

        let req = builder
            .body(req_body)
            .map_err(|_| ClientError::InvalidResponse {
                detail: "failed to build request".to_string(),
            })?;

        let response =
            sender
                .send_request(req)
                .await
                .map_err(|_| ClientError::InvalidResponse {
                    detail: "request failed".to_string(),
                })?;

        let status = response.status().as_u16();

        // Collect the response body.
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|_| ClientError::InvalidResponse {
                detail: "failed to read response body".to_string(),
            })?
            .to_bytes();

        if status == 204 || body_bytes.is_empty() {
            return Ok((status, None));
        }

        let json: serde_json::Value =
            serde_json::from_slice(&body_bytes).map_err(|e| ClientError::InvalidResponse {
                detail: format!("invalid JSON in response: {e}"),
            })?;

        Ok((status, Some(json)))
    }

    // ── GA endpoints (#468) ──────────────────────────────────────────

    /// Health check. Returns `true` if the VMM is responding (HTTP 200).
    pub async fn ping(&self) -> Result<bool, ClientError> {
        let (status, _) = self.request(hyper::Method::GET, "/vmm.ping", None).await?;
        Ok(status == 200)
    }

    /// VM info. Returns the full VM status JSON from Cloud Hypervisor.
    pub async fn info(&self) -> Result<serde_json::Value, ClientError> {
        let (status, body) = self.request(hyper::Method::GET, "/vm.info", None).await?;
        if !(200..300).contains(&status) {
            return Err(ClientError::UnexpectedStatus {
                status,
                body: body.map(|v| v.to_string()).unwrap_or_default(),
            });
        }
        body.ok_or(ClientError::InvalidResponse {
            detail: "expected JSON body from /vm.info".to_string(),
        })
    }

    /// Create a VM definition. NOT idempotent — creating twice is a bug (409 = error).
    pub async fn create(&self, config: serde_json::Value) -> Result<(), ClientError> {
        let (status, body) = self
            .request(hyper::Method::PUT, "/vm.create", Some(config))
            .await?;
        if (200..300).contains(&status) {
            return Ok(());
        }
        // create is explicitly NOT idempotent: 409 means the VM already exists,
        // and the caller has a bug.
        Err(ClientError::UnexpectedStatus {
            status,
            body: body.map(|v| v.to_string()).unwrap_or_default(),
        })
    }

    /// Boot the VM. Idempotent: if already booted (409), returns Ok.
    pub async fn boot(&self) -> Result<(), ClientError> {
        let (status, body) = self.request(hyper::Method::PUT, "/vm.boot", None).await?;
        if (200..300).contains(&status) || is_idempotent_ok(status, "boot") {
            return Ok(());
        }
        Err(ClientError::UnexpectedStatus {
            status,
            body: body.map(|v| v.to_string()).unwrap_or_default(),
        })
    }

    /// ACPI shutdown signal to guest. Idempotent: if already stopped (404), returns Ok.
    pub async fn shutdown_graceful(&self) -> Result<(), ClientError> {
        let (status, body) = self
            .request(hyper::Method::PUT, "/vm.shutdown", None)
            .await?;
        if (200..300).contains(&status) || is_idempotent_ok(status, "shutdown_graceful") {
            return Ok(());
        }
        Err(ClientError::UnexpectedStatus {
            status,
            body: body.map(|v| v.to_string()).unwrap_or_default(),
        })
    }

    /// Power button (guest-level force). Idempotent: if already stopped (404), returns Ok.
    pub async fn shutdown_force(&self) -> Result<(), ClientError> {
        let (status, body) = self
            .request(hyper::Method::PUT, "/vm.power-button", None)
            .await?;
        if (200..300).contains(&status) || is_idempotent_ok(status, "shutdown_force") {
            return Ok(());
        }
        Err(ClientError::UnexpectedStatus {
            status,
            body: body.map(|v| v.to_string()).unwrap_or_default(),
        })
    }

    /// Delete VM definition. Idempotent: if already deleted (404), returns Ok.
    pub async fn delete(&self) -> Result<(), ClientError> {
        let (status, body) = self.request(hyper::Method::PUT, "/vm.delete", None).await?;
        if (200..300).contains(&status) || is_idempotent_ok(status, "delete") {
            return Ok(());
        }
        Err(ClientError::UnexpectedStatus {
            status,
            body: body.map(|v| v.to_string()).unwrap_or_default(),
        })
    }

    // ── Beta endpoints (#469) ────────────────────────────────────────

    /// Reboot guest. NOT idempotent on stopped VM — must be running.
    pub async fn reboot(&self) -> Result<(), ClientError> {
        let (status, body) = self.request(hyper::Method::PUT, "/vm.reboot", None).await?;
        if (200..300).contains(&status) {
            return Ok(());
        }
        Err(ClientError::UnexpectedStatus {
            status,
            body: body.map(|v| v.to_string()).unwrap_or_default(),
        })
    }

    /// Pause VM execution. Idempotent: if already paused (409), returns Ok.
    pub async fn pause(&self) -> Result<(), ClientError> {
        let (status, body) = self.request(hyper::Method::PUT, "/vm.pause", None).await?;
        if (200..300).contains(&status) || is_idempotent_ok(status, "pause") {
            return Ok(());
        }
        Err(ClientError::UnexpectedStatus {
            status,
            body: body.map(|v| v.to_string()).unwrap_or_default(),
        })
    }

    /// Resume paused VM. Idempotent: if already running (409), returns Ok.
    pub async fn resume(&self) -> Result<(), ClientError> {
        let (status, body) = self.request(hyper::Method::PUT, "/vm.resume", None).await?;
        if (200..300).contains(&status) || is_idempotent_ok(status, "resume") {
            return Ok(());
        }
        Err(ClientError::UnexpectedStatus {
            status,
            body: body.map(|v| v.to_string()).unwrap_or_default(),
        })
    }

    /// Hot-resize CPU and/or memory. NOT idempotent on stopped VM — must be running.
    pub async fn resize(
        &self,
        vcpus: Option<u32>,
        memory_bytes: Option<u64>,
    ) -> Result<(), ClientError> {
        let mut body = serde_json::Map::new();
        if let Some(v) = vcpus {
            body.insert("desired_vcpus".to_string(), serde_json::Value::from(v));
        }
        if let Some(m) = memory_bytes {
            body.insert("desired_ram".to_string(), serde_json::Value::from(m));
        }

        let (status, resp_body) = self
            .request(
                hyper::Method::PUT,
                "/vm.resize",
                Some(serde_json::Value::Object(body)),
            )
            .await?;
        if (200..300).contains(&status) {
            return Ok(());
        }
        Err(ClientError::UnexpectedStatus {
            status,
            body: resp_body.map(|v| v.to_string()).unwrap_or_default(),
        })
    }

    /// Performance counters. Returns parsed JSON.
    pub async fn counters(&self) -> Result<serde_json::Value, ClientError> {
        let (status, body) = self
            .request(hyper::Method::GET, "/vm.counters", None)
            .await?;
        if !(200..300).contains(&status) {
            return Err(ClientError::UnexpectedStatus {
                status,
                body: body.map(|v| v.to_string()).unwrap_or_default(),
            });
        }
        body.ok_or(ClientError::InvalidResponse {
            detail: "expected JSON body from /vm.counters".to_string(),
        })
    }
}

// ── Idempotence logic (#471) ─────────────────────────────────────────

/// Determine whether a non-2xx HTTP status should be treated as a
/// successful no-op for the given operation.
///
/// Cloud Hypervisor returns specific status codes when an operation is
/// applied to a VM that is already in the target state. For reconciliation
/// loops, these are success — the desired state already matches reality.
///
/// | Operation          | Idempotent status | Meaning                |
/// |--------------------|-------------------|------------------------|
/// | boot               | 409               | Already booted         |
/// | shutdown_graceful   | 404               | Already stopped        |
/// | shutdown_force      | 404               | Already stopped        |
/// | delete             | 404               | Already deleted/absent |
/// | pause              | 409               | Already paused         |
/// | resume             | 409               | Already running        |
fn is_idempotent_ok(status: u16, operation: &str) -> bool {
    match operation {
        // Already booted → no-op.
        "boot" => status == 409,
        // Already stopped → no-op.
        "shutdown_graceful" | "shutdown_force" => status == 404,
        // Already deleted → no-op.
        "delete" => status == 404,
        // Already paused → no-op.
        "pause" => status == 409,
        // Already running → no-op.
        "resume" => status == 409,
        // All other operations: non-2xx is an error.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Idempotence truth table ──────────────────────────────────────

    #[test]
    fn idempotent_boot_409() {
        assert!(is_idempotent_ok(409, "boot"));
    }

    #[test]
    fn idempotent_boot_404_is_not_ok() {
        assert!(!is_idempotent_ok(404, "boot"));
    }

    #[test]
    fn idempotent_shutdown_graceful_404() {
        assert!(is_idempotent_ok(404, "shutdown_graceful"));
    }

    #[test]
    fn idempotent_shutdown_force_404() {
        assert!(is_idempotent_ok(404, "shutdown_force"));
    }

    #[test]
    fn idempotent_delete_404() {
        assert!(is_idempotent_ok(404, "delete"));
    }

    #[test]
    fn idempotent_pause_409() {
        assert!(is_idempotent_ok(409, "pause"));
    }

    #[test]
    fn idempotent_resume_409() {
        assert!(is_idempotent_ok(409, "resume"));
    }

    #[test]
    fn create_is_not_idempotent() {
        // 409 on create is an error, not a no-op.
        assert!(!is_idempotent_ok(409, "create"));
    }

    #[test]
    fn reboot_is_not_idempotent() {
        assert!(!is_idempotent_ok(409, "reboot"));
    }

    #[test]
    fn resize_is_not_idempotent() {
        assert!(!is_idempotent_ok(409, "resize"));
    }

    // ── Constructor tests ────────────────────────────────────────────

    #[test]
    fn new_uses_default_timeout() {
        let client = ChClient::new(PathBuf::from("/tmp/test.sock"));
        assert_eq!(client.timeout, DEFAULT_TIMEOUT);
    }

    #[test]
    fn with_timeout_uses_custom() {
        let t = Duration::from_secs(30);
        let client = ChClient::with_timeout(PathBuf::from("/tmp/test.sock"), t);
        assert_eq!(client.timeout, t);
    }

    // ── Socket-not-found test ────────────────────────────────────────

    #[tokio::test]
    async fn request_to_missing_socket_returns_socket_not_found() {
        let client = ChClient::new(PathBuf::from("/tmp/nonexistent-ch-test.sock"));
        let result = client.ping().await;
        assert!(matches!(result, Err(ClientError::SocketNotFound { .. })));
    }
}
