//! API key authentication middleware for the REST/gRPC-compatible server.
//!
//! Extracts `Authorization: Bearer syf_key_...` from incoming requests,
//! validates the key, checks role-based permissions for the endpoint, and
//! injects validated key metadata into request extensions for downstream
//! handlers.
//!
//! Until the IAM layer (#359) is merged, validation uses a stub that accepts
//! any key matching the `syf_key_` prefix and assigns `ApiKeyRole::Admin`.

use std::sync::Arc;

use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};

use crate::audit;

// ---------------------------------------------------------------------------
// API key types
// ---------------------------------------------------------------------------

/// Role attached to a validated API key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiKeyRole {
    /// Read-only access (status, list operations).
    ReadOnly,
    /// Operator-level access (peering, peer management).
    Operator,
    /// Full administrative access (rotate secrets, reload config).
    Admin,
}

impl std::fmt::Display for ApiKeyRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiKeyRole::ReadOnly => write!(f, "read_only"),
            ApiKeyRole::Operator => write!(f, "operator"),
            ApiKeyRole::Admin => write!(f, "admin"),
        }
    }
}

/// Validated API key information injected into request extensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatedApiKey {
    /// Human-readable key name (never the raw secret).
    pub key_name: String,
    /// Role associated with this key.
    pub role: ApiKeyRole,
    /// Trace ID for request correlation.
    pub trace_id: String,
}

/// Errors returned by the API key validator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiKeyError {
    /// The key is missing from the request.
    Missing,
    /// The key format is invalid (does not match `syf_key_` prefix).
    InvalidFormat,
    /// The key was not found or has been revoked.
    InvalidKey,
    /// The key has expired.
    Expired,
}

// ---------------------------------------------------------------------------
// Validator trait + stub
// ---------------------------------------------------------------------------

/// Trait for API key validation. Allows swapping the stub for the real IAM
/// validator once #359 is merged.
#[async_trait::async_trait]
pub trait ApiKeyValidator: Send + Sync + 'static {
    /// Validate a raw API key string and return the associated metadata.
    async fn validate(&self, raw_key: &str) -> Result<ValidatedApiKey, ApiKeyError>;
}

/// Stub validator that accepts any key with the `syf_key_` prefix.
/// Assigns `ApiKeyRole::Admin` to all valid keys.
///
/// Replace with the real IAM validator when #359 lands.
#[derive(Debug, Clone)]
pub struct StubApiKeyValidator;

#[async_trait::async_trait]
impl ApiKeyValidator for StubApiKeyValidator {
    async fn validate(&self, raw_key: &str) -> Result<ValidatedApiKey, ApiKeyError> {
        if !raw_key.starts_with("syf_key_") {
            return Err(ApiKeyError::InvalidFormat);
        }
        if raw_key.len() < 12 {
            return Err(ApiKeyError::InvalidKey);
        }
        // Derive a stable key name from the key (last 4 chars for identification).
        let suffix = &raw_key[raw_key.len().saturating_sub(4)..];
        Ok(ValidatedApiKey {
            key_name: format!("key_...{suffix}"),
            role: ApiKeyRole::Admin,
            trace_id: generate_trace_id(),
        })
    }
}

/// Generate a simple trace ID for request correlation.
fn generate_trace_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{ts:x}")
}

// ---------------------------------------------------------------------------
// Endpoint permission mapping
// ---------------------------------------------------------------------------

/// Required minimum role for each API endpoint path.
fn required_role_for_path(path: &str) -> ApiKeyRole {
    match path {
        // Read-only endpoints
        "/v1/fabric/status" | "/v1/fabric/peering/requests" => ApiKeyRole::ReadOnly,
        // Admin-only endpoints
        "/v1/fabric/rotate-secret" | "/v1/fabric/reload" => ApiKeyRole::Admin,
        // Everything else requires operator
        _ => ApiKeyRole::Operator,
    }
}

/// Check if the given role satisfies the required role.
fn role_satisfies(actual: ApiKeyRole, required: ApiKeyRole) -> bool {
    match required {
        ApiKeyRole::ReadOnly => true,
        ApiKeyRole::Operator => matches!(actual, ApiKeyRole::Operator | ApiKeyRole::Admin),
        ApiKeyRole::Admin => actual == ApiKeyRole::Admin,
    }
}

// ---------------------------------------------------------------------------
// Audit event types for auth
// ---------------------------------------------------------------------------

/// Audit event types specific to API key authentication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthAuditEvent {
    /// An API request was authenticated successfully.
    ApiAuthSuccess,
    /// An API request was rejected due to missing/invalid key.
    ApiAuthFailure,
    /// An API request was rejected due to insufficient permissions.
    ApiAuthForbidden,
}

impl std::fmt::Display for AuthAuditEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthAuditEvent::ApiAuthSuccess => write!(f, "api.auth.success"),
            AuthAuditEvent::ApiAuthFailure => write!(f, "api.auth.failure"),
            AuthAuditEvent::ApiAuthForbidden => write!(f, "api.auth.forbidden"),
        }
    }
}

/// Log an auth audit event using the existing audit infrastructure.
fn audit_auth_event(event: AuthAuditEvent, source_ip: &str, details: &str) {
    audit::emit(
        // Reuse the closest existing audit event type for compatibility.
        // The details field carries the specific auth event info.
        audit::AuditEventType::DaemonStarted, // placeholder until AuditEventType is extended
        None,
        Some(source_ip),
        Some(&format!("{event}: {details}")),
    );
    // Also log via tracing for structured observability.
    tracing::info!(
        event = %event,
        source_ip = %source_ip,
        details = %details,
        "auth audit"
    );
}

// ---------------------------------------------------------------------------
// Axum middleware
// ---------------------------------------------------------------------------

/// Shared validator reference for the middleware layer.
pub type SharedValidator = Arc<dyn ApiKeyValidator>;

/// Error response body for auth failures.
#[derive(Debug, Serialize)]
struct AuthErrorResponse {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace_id: Option<String>,
}

/// Axum middleware that validates API key authentication.
///
/// This function is designed to be used with `axum::middleware::from_fn_with_state`.
///
/// # Flow
/// 1. Extract `Authorization: Bearer syf_key_...` header
/// 2. Validate the key via the provided validator
/// 3. Check role-based permissions for the requested endpoint
/// 4. On success: inject `ValidatedApiKey` into request extensions and continue
/// 5. On failure: return 401 (unauthenticated) or 403 (forbidden)
pub async fn auth_middleware(
    axum::extract::Extension(validator): axum::extract::Extension<SharedValidator>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    // Extract source IP from X-Forwarded-For or X-Real-IP headers, falling
    // back to "unknown" when neither is present (ConnectInfo may not be
    // available depending on how the server is launched).
    let source_ip = request
        .headers()
        .get("x-forwarded-for")
        .or_else(|| request.headers().get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let path = request.uri().path().to_string();

    // --- Step 1: Extract the Authorization header ---
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let bearer_token = match auth_header {
        Some(header) if header.starts_with("Bearer ") => &header[7..],
        Some(_) => {
            let trace_id = generate_trace_id();
            audit_auth_event(
                AuthAuditEvent::ApiAuthFailure,
                &source_ip,
                &format!("invalid auth header format, path={path}, trace_id={trace_id}"),
            );
            return (
                StatusCode::UNAUTHORIZED,
                Json(AuthErrorResponse {
                    error: "invalid authorization header format, expected: Bearer syf_key_..."
                        .to_string(),
                    trace_id: Some(trace_id),
                }),
            )
                .into_response();
        }
        None => {
            let trace_id = generate_trace_id();
            audit_auth_event(
                AuthAuditEvent::ApiAuthFailure,
                &source_ip,
                &format!("missing auth header, path={path}, trace_id={trace_id}"),
            );
            return (
                StatusCode::UNAUTHORIZED,
                Json(AuthErrorResponse {
                    error: "missing Authorization header".to_string(),
                    trace_id: Some(trace_id),
                }),
            )
                .into_response();
        }
    };

    // --- Step 2: Validate the key ---
    let validated = match validator.validate(bearer_token).await {
        Ok(v) => v,
        Err(e) => {
            let trace_id = generate_trace_id();
            let reason = match e {
                ApiKeyError::Missing => "missing key",
                ApiKeyError::InvalidFormat => "invalid key format",
                ApiKeyError::InvalidKey => "invalid or revoked key",
                ApiKeyError::Expired => "expired key",
            };
            audit_auth_event(
                AuthAuditEvent::ApiAuthFailure,
                &source_ip,
                &format!(
                    "key_validation_failed, reason={reason}, path={path}, trace_id={trace_id}"
                ),
            );
            return (
                StatusCode::UNAUTHORIZED,
                Json(AuthErrorResponse {
                    error: format!("authentication failed: {reason}"),
                    trace_id: Some(trace_id),
                }),
            )
                .into_response();
        }
    };

    // --- Step 3: Check role-based permissions ---
    let required_role = required_role_for_path(&path);
    if !role_satisfies(validated.role, required_role) {
        audit_auth_event(
            AuthAuditEvent::ApiAuthForbidden,
            &source_ip,
            &format!(
                "key_name={}, role={}, required={required_role}, path={path}, trace_id={}",
                validated.key_name, validated.role, validated.trace_id
            ),
        );
        return (
            StatusCode::FORBIDDEN,
            Json(AuthErrorResponse {
                error: format!(
                    "insufficient permissions: role '{}' cannot access this endpoint (requires '{required_role}')",
                    validated.role
                ),
                trace_id: Some(validated.trace_id),
            }),
        )
            .into_response();
    }

    // --- Step 4: Audit success and inject into extensions ---
    audit_auth_event(
        AuthAuditEvent::ApiAuthSuccess,
        &source_ip,
        &format!(
            "key_name={}, role={}, path={path}, trace_id={}",
            validated.key_name, validated.role, validated.trace_id
        ),
    );

    request.extensions_mut().insert(validated);
    next.run(request).await
}

// ---------------------------------------------------------------------------
// Router helper
// ---------------------------------------------------------------------------

/// Wrap a router with the auth middleware layer.
///
/// The `/v1/fabric/status` endpoint is left **inside** the auth layer (it
/// still requires a valid key, but only `ReadOnly` role). If you need an
/// unauthenticated health-check, add a separate `/healthz` route outside
/// this layer.
pub fn with_auth_layer(
    router: axum::Router<Arc<dyn crate::control::FabricHandler>>,
    validator: SharedValidator,
) -> axum::Router<Arc<dyn crate::control::FabricHandler>> {
    // Order matters: axum applies layers outside-in, so `from_fn` runs first
    // and sees the Extension that was added to the inner service.
    router
        .layer(axum::middleware::from_fn(auth_middleware))
        .layer(axum::Extension(validator))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::routing::get;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// Test handler that echoes back the validated key info from extensions.
    async fn echo_key(request: Request<Body>) -> impl IntoResponse {
        match request.extensions().get::<ValidatedApiKey>() {
            Some(key) => Json(serde_json::json!({
                "key_name": key.key_name,
                "role": key.role,
                "trace_id": key.trace_id,
            }))
            .into_response(),
            None => (StatusCode::INTERNAL_SERVER_ERROR, "no key in extensions").into_response(),
        }
    }

    fn test_app() -> axum::Router {
        let validator: SharedValidator = Arc::new(StubApiKeyValidator);
        axum::Router::new()
            .route("/v1/fabric/status", get(echo_key))
            .route("/v1/fabric/peering/start", axum::routing::post(echo_key))
            .route("/v1/fabric/rotate-secret", axum::routing::post(echo_key))
            .layer(axum::middleware::from_fn(auth_middleware))
            .layer(axum::Extension(validator))
    }

    async fn send(
        app: axum::Router,
        method: &str,
        uri: &str,
        auth: Option<&str>,
    ) -> (StatusCode, String) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = auth {
            builder = builder.header("authorization", token);
        }
        let req = builder.body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&bytes).to_string())
    }

    #[tokio::test]
    async fn missing_auth_header_returns_401() {
        let app = test_app();
        let (status, body) = send(app, "GET", "/v1/fabric/status", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["error"].as_str().unwrap().contains("missing"));
        assert!(v["trace_id"].is_string());
    }

    #[tokio::test]
    async fn invalid_auth_format_returns_401() {
        let app = test_app();
        let (status, body) = send(app, "GET", "/v1/fabric/status", Some("Basic abc")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["error"].as_str().unwrap().contains("invalid"));
    }

    #[tokio::test]
    async fn invalid_key_prefix_returns_401() {
        let app = test_app();
        let (status, body) =
            send(app, "GET", "/v1/fabric/status", Some("Bearer bad_key_xxxx")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["error"].as_str().unwrap().contains("invalid key format"));
    }

    #[tokio::test]
    async fn short_key_returns_401() {
        let app = test_app();
        let (status, _) = send(app, "GET", "/v1/fabric/status", Some("Bearer syf_key_")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn valid_key_returns_200_with_extensions() {
        let app = test_app();
        let (status, body) = send(
            app,
            "GET",
            "/v1/fabric/status",
            Some("Bearer syf_key_test1234"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["key_name"], "key_...1234");
        assert_eq!(v["role"], "admin");
        assert!(v["trace_id"].is_string());
    }

    #[tokio::test]
    async fn validated_key_injected_into_extensions() {
        let app = test_app();
        let (status, body) = send(
            app,
            "POST",
            "/v1/fabric/peering/start",
            Some("Bearer syf_key_operator_abc"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["key_name"].as_str().unwrap().starts_with("key_..."));
    }

    #[test]
    fn role_satisfies_hierarchy() {
        // Admin can access everything
        assert!(role_satisfies(ApiKeyRole::Admin, ApiKeyRole::ReadOnly));
        assert!(role_satisfies(ApiKeyRole::Admin, ApiKeyRole::Operator));
        assert!(role_satisfies(ApiKeyRole::Admin, ApiKeyRole::Admin));

        // Operator can access read-only and operator
        assert!(role_satisfies(ApiKeyRole::Operator, ApiKeyRole::ReadOnly));
        assert!(role_satisfies(ApiKeyRole::Operator, ApiKeyRole::Operator));
        assert!(!role_satisfies(ApiKeyRole::Operator, ApiKeyRole::Admin));

        // ReadOnly can only access read-only
        assert!(role_satisfies(ApiKeyRole::ReadOnly, ApiKeyRole::ReadOnly));
        assert!(!role_satisfies(ApiKeyRole::ReadOnly, ApiKeyRole::Operator));
        assert!(!role_satisfies(ApiKeyRole::ReadOnly, ApiKeyRole::Admin));
    }

    #[test]
    fn required_role_mapping() {
        assert_eq!(
            required_role_for_path("/v1/fabric/status"),
            ApiKeyRole::ReadOnly
        );
        assert_eq!(
            required_role_for_path("/v1/fabric/peering/requests"),
            ApiKeyRole::ReadOnly
        );
        assert_eq!(
            required_role_for_path("/v1/fabric/rotate-secret"),
            ApiKeyRole::Admin
        );
        assert_eq!(
            required_role_for_path("/v1/fabric/reload"),
            ApiKeyRole::Admin
        );
        assert_eq!(
            required_role_for_path("/v1/fabric/peering/start"),
            ApiKeyRole::Operator
        );
        assert_eq!(
            required_role_for_path("/v1/fabric/peers/remove"),
            ApiKeyRole::Operator
        );
    }

    #[test]
    fn trace_id_is_generated() {
        let id = generate_trace_id();
        assert!(!id.is_empty());
        // Should be a hex string
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn stub_validator_key_name_masks_key() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let validator = StubApiKeyValidator;
        let result = rt.block_on(validator.validate("syf_key_supersecret123"));
        let key = result.unwrap();
        // Key name should NOT contain the full key
        assert!(!key.key_name.contains("supersecret"));
        assert!(key.key_name.contains("..."));
    }

    /// Custom validator that always returns ReadOnly for testing permission checks.
    struct ReadOnlyValidator;

    #[async_trait::async_trait]
    impl ApiKeyValidator for ReadOnlyValidator {
        async fn validate(&self, raw_key: &str) -> Result<ValidatedApiKey, ApiKeyError> {
            if !raw_key.starts_with("syf_key_") || raw_key.len() < 12 {
                return Err(ApiKeyError::InvalidKey);
            }
            Ok(ValidatedApiKey {
                key_name: "readonly-key".to_string(),
                role: ApiKeyRole::ReadOnly,
                trace_id: generate_trace_id(),
            })
        }
    }

    fn readonly_app() -> axum::Router {
        let validator: SharedValidator = Arc::new(ReadOnlyValidator);
        axum::Router::new()
            .route("/v1/fabric/status", get(echo_key))
            .route("/v1/fabric/peering/start", axum::routing::post(echo_key))
            .route("/v1/fabric/rotate-secret", axum::routing::post(echo_key))
            .layer(axum::middleware::from_fn(auth_middleware))
            .layer(axum::Extension(validator))
    }

    #[tokio::test]
    async fn readonly_key_can_access_status() {
        let app = readonly_app();
        let (status, _) = send(
            app,
            "GET",
            "/v1/fabric/status",
            Some("Bearer syf_key_readonly_test"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn readonly_key_cannot_access_operator_endpoint() {
        let app = readonly_app();
        let (status, body) = send(
            app,
            "POST",
            "/v1/fabric/peering/start",
            Some("Bearer syf_key_readonly_test"),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["error"].as_str().unwrap().contains("insufficient"));
    }

    #[tokio::test]
    async fn readonly_key_cannot_access_admin_endpoint() {
        let app = readonly_app();
        let (status, body) = send(
            app,
            "POST",
            "/v1/fabric/rotate-secret",
            Some("Bearer syf_key_readonly_test"),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["error"].as_str().unwrap().contains("insufficient"));
        assert!(v["trace_id"].is_string());
    }
}
