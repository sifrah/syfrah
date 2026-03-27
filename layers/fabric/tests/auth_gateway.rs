//! Phase 3.5 — Tests for auth middleware, gateway auth flow, rate limiting,
//! CIDR enforcement, and audit log integration.

use std::net::IpAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use syfrah_fabric::auth_middleware::{
    ApiKeyError, ApiKeyRole, ApiKeyValidator, SharedValidator, ValidatedApiKey,
};
use syfrah_fabric::grpc_api::router_with_auth;

// =========================================================================
// Helper: stub FabricHandler (mirrors the one in grpc_api::tests)
// =========================================================================

struct StubFabricHandler;

#[async_trait::async_trait]
impl syfrah_fabric::control::FabricHandler for StubFabricHandler {
    async fn handle(
        &self,
        req: syfrah_fabric::control::FabricRequest,
        _caller_uid: Option<u32>,
    ) -> syfrah_fabric::control::FabricResponse {
        match req {
            syfrah_fabric::control::FabricRequest::PeeringList => {
                syfrah_fabric::control::FabricResponse::PeeringList { requests: vec![] }
            }
            _ => syfrah_fabric::control::FabricResponse::Ok,
        }
    }
}

fn handler() -> Arc<dyn syfrah_fabric::control::FabricHandler> {
    Arc::new(StubFabricHandler)
}

// =========================================================================
// Custom validators for testing specific auth scenarios
// =========================================================================

/// Validator that returns Expired for any key containing "expired".
struct ExpiredKeyValidator;

#[async_trait::async_trait]
impl ApiKeyValidator for ExpiredKeyValidator {
    async fn validate(&self, raw_key: &str) -> Result<ValidatedApiKey, ApiKeyError> {
        if !raw_key.starts_with("syf_key_") {
            return Err(ApiKeyError::InvalidFormat);
        }
        if raw_key.contains("expired") {
            return Err(ApiKeyError::Expired);
        }
        Ok(ValidatedApiKey {
            key_name: "test-key".to_string(),
            role: ApiKeyRole::Admin,
            trace_id: "trace-test-001".to_string(),
        })
    }
}

/// Validator that assigns Operator role (can access operator endpoints but not admin).
struct OperatorValidator;

#[async_trait::async_trait]
impl ApiKeyValidator for OperatorValidator {
    async fn validate(&self, raw_key: &str) -> Result<ValidatedApiKey, ApiKeyError> {
        if !raw_key.starts_with("syf_key_") || raw_key.len() < 12 {
            return Err(ApiKeyError::InvalidKey);
        }
        Ok(ValidatedApiKey {
            key_name: "operator-key".to_string(),
            role: ApiKeyRole::Operator,
            trace_id: "trace-op-001".to_string(),
        })
    }
}

/// Validator that assigns ReadOnly role.
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
            trace_id: "trace-ro-001".to_string(),
        })
    }
}

// =========================================================================
// Helpers
// =========================================================================

fn build_router(validator: SharedValidator) -> axum::Router {
    router_with_auth(handler(), Some(validator))
}

fn make_request(
    method: &str,
    uri: &str,
    auth: Option<&str>,
    body: Option<serde_json::Value>,
) -> Request<Body> {
    let body = match body {
        Some(v) => Body::from(serde_json::to_vec(&v).unwrap()),
        None => Body::empty(),
    };
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = auth {
        builder = builder.header("authorization", token);
    }
    builder.body(body).unwrap()
}

async fn send(
    app: axum::Router,
    method: &str,
    uri: &str,
    auth: Option<&str>,
    body: Option<serde_json::Value>,
) -> (StatusCode, String) {
    let req = make_request(method, uri, auth, body);
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

// =========================================================================
// Auth flow tests
// =========================================================================

#[tokio::test]
async fn valid_key_returns_200() {
    let validator: SharedValidator = Arc::new(ExpiredKeyValidator);
    let app = build_router(validator);
    let (status, _) = send(
        app,
        "GET",
        "/v1/fabric/status",
        Some("Bearer syf_key_validtoken123"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn expired_key_returns_401() {
    let validator: SharedValidator = Arc::new(ExpiredKeyValidator);
    let app = build_router(validator);
    let (status, body) = send(
        app,
        "GET",
        "/v1/fabric/status",
        Some("Bearer syf_key_expired_abc"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["error"].as_str().unwrap().contains("expired"));
}

#[tokio::test]
async fn no_header_returns_401() {
    let validator: SharedValidator = Arc::new(ExpiredKeyValidator);
    let app = build_router(validator);
    let (status, body) = send(app, "GET", "/v1/fabric/status", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["error"].as_str().unwrap().contains("missing"));
}

#[tokio::test]
async fn wrong_role_returns_403_on_admin_endpoint() {
    // Operator key cannot access admin-only endpoint (rotate-secret).
    let validator: SharedValidator = Arc::new(OperatorValidator);
    let app = build_router(validator);
    let (status, body) = send(
        app,
        "POST",
        "/v1/fabric/rotate-secret",
        Some("Bearer syf_key_operator_test"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["error"].as_str().unwrap().contains("insufficient"));
}

#[tokio::test]
async fn wrong_role_returns_403_readonly_on_operator_endpoint() {
    // ReadOnly key cannot access operator endpoint (peering/start).
    let validator: SharedValidator = Arc::new(ReadOnlyValidator);
    let app = build_router(validator);
    let (status, body) = send(
        app,
        "POST",
        "/v1/fabric/peering/start",
        Some("Bearer syf_key_readonly_test"),
        Some(serde_json::json!({"port": 7946})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["error"].as_str().unwrap().contains("insufficient"));
}

#[tokio::test]
async fn operator_key_can_access_operator_endpoint() {
    let validator: SharedValidator = Arc::new(OperatorValidator);
    let app = build_router(validator);
    let (status, _) = send(
        app,
        "POST",
        "/v1/fabric/peering/start",
        Some("Bearer syf_key_operator_test"),
        Some(serde_json::json!({"port": 7946})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn operator_key_can_access_readonly_endpoint() {
    let validator: SharedValidator = Arc::new(OperatorValidator);
    let app = build_router(validator);
    let (status, _) = send(
        app,
        "GET",
        "/v1/fabric/status",
        Some("Bearer syf_key_operator_test"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn auth_failure_response_includes_trace_id() {
    let validator: SharedValidator = Arc::new(ExpiredKeyValidator);
    let app = build_router(validator);
    let (status, body) = send(
        app,
        "GET",
        "/v1/fabric/status",
        Some("Bearer syf_key_expired_abc"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(v["trace_id"].is_string(), "response must include trace_id");
}

#[tokio::test]
async fn forbidden_response_includes_trace_id() {
    let validator: SharedValidator = Arc::new(ReadOnlyValidator);
    let app = build_router(validator);
    let (status, body) = send(
        app,
        "POST",
        "/v1/fabric/rotate-secret",
        Some("Bearer syf_key_readonly_test"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        v["trace_id"].is_string(),
        "forbidden response must include trace_id"
    );
}

// =========================================================================
// Rate limiting: PinRateLimiter bucket fill/drain, burst, per-key isolation
// =========================================================================

#[test]
fn rate_limiter_starts_unlocked() {
    use syfrah_fabric::peering::PinRateLimiter;
    let mut rl = PinRateLimiter::new();
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    assert!(
        !rl.is_locked_out(ip),
        "fresh limiter should not lock out any IP"
    );
}

#[test]
fn rate_limiter_single_failure_does_not_lock() {
    use syfrah_fabric::peering::PinRateLimiter;
    let mut rl = PinRateLimiter::new();
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    assert!(!rl.record_failure(ip));
    assert!(!rl.is_locked_out(ip));
}

#[test]
fn rate_limiter_burst_up_to_threshold_no_lockout() {
    use syfrah_fabric::peering::PinRateLimiter;
    let mut rl = PinRateLimiter::new();
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    // 4 failures should not lock out (threshold is 5).
    for _ in 0..4 {
        rl.record_failure(ip);
    }
    assert!(
        !rl.is_locked_out(ip),
        "4 failures should not exceed the threshold"
    );
}

#[test]
fn rate_limiter_locks_at_threshold() {
    use syfrah_fabric::peering::PinRateLimiter;
    let mut rl = PinRateLimiter::new();
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    for _ in 0..5 {
        rl.record_failure(ip);
    }
    assert!(rl.is_locked_out(ip), "5 failures should trigger lockout");
}

#[test]
fn rate_limiter_per_ip_isolation() {
    use syfrah_fabric::peering::PinRateLimiter;
    let mut rl = PinRateLimiter::new();
    let ip_a: IpAddr = "10.0.0.1".parse().unwrap();
    let ip_b: IpAddr = "10.0.0.2".parse().unwrap();
    let ip_c: IpAddr = "10.0.0.3".parse().unwrap();

    // Lock out ip_a.
    for _ in 0..5 {
        rl.record_failure(ip_a);
    }
    assert!(rl.is_locked_out(ip_a));

    // ip_b and ip_c should remain unlocked.
    assert!(!rl.is_locked_out(ip_b));
    assert!(!rl.is_locked_out(ip_c));

    // Add some failures to ip_b (but not enough to lock out).
    for _ in 0..3 {
        rl.record_failure(ip_b);
    }
    assert!(!rl.is_locked_out(ip_b));
    assert!(!rl.is_locked_out(ip_c));
}

#[test]
fn rate_limiter_ipv6_isolation() {
    use syfrah_fabric::peering::PinRateLimiter;
    let mut rl = PinRateLimiter::new();
    let ip_v4: IpAddr = "10.0.0.1".parse().unwrap();
    let ip_v6: IpAddr = "fd00::1".parse().unwrap();

    // Lock out the IPv4 address.
    for _ in 0..5 {
        rl.record_failure(ip_v4);
    }
    assert!(rl.is_locked_out(ip_v4));
    // IPv6 address should be independent.
    assert!(!rl.is_locked_out(ip_v6));
}

#[test]
fn rate_limiter_record_failure_returns_true_on_lockout() {
    use syfrah_fabric::peering::PinRateLimiter;
    let mut rl = PinRateLimiter::new();
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    let mut locked = false;
    for _ in 0..6 {
        if rl.record_failure(ip) {
            locked = true;
        }
    }
    assert!(
        locked,
        "record_failure should return true once threshold is exceeded"
    );
}

// =========================================================================
// Audit: verify auth events appear in audit log with key name and trace_id
//
// These sub-tests share a single HOME env var to avoid races between
// concurrent tokio tests (env vars are process-global).
// =========================================================================

#[tokio::test]
async fn audit_log_records_auth_success_failure_and_forbidden() {
    // Set HOME to an isolated temp directory for the entire test.
    let tmp = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("HOME", tmp.path()) };

    // --- Part 1: auth success emits audit event with key_name and trace_id ---
    {
        let validator: SharedValidator =
            Arc::new(syfrah_fabric::auth_middleware::StubApiKeyValidator);
        let app = build_router(validator);

        let (status, _) = send(
            app,
            "GET",
            "/v1/fabric/status",
            Some("Bearer syf_key_auditcheck1234"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let entries = syfrah_fabric::audit::read_entries().unwrap();
        let auth_entries: Vec<_> = entries
            .iter()
            .filter(|e| {
                e.details
                    .as_deref()
                    .map(|d| d.contains("api.auth.success"))
                    .unwrap_or(false)
            })
            .collect();
        assert!(
            !auth_entries.is_empty(),
            "audit log should contain an api.auth.success event"
        );

        let last = auth_entries.last().unwrap();
        let details = last.details.as_deref().unwrap();
        assert!(
            details.contains("key_name="),
            "audit details should include key_name, got: {details}"
        );
        assert!(
            details.contains("trace_id="),
            "audit details should include trace_id, got: {details}"
        );
    }

    // --- Part 2: auth failure (no header) emits audit event ---
    {
        let validator: SharedValidator =
            Arc::new(syfrah_fabric::auth_middleware::StubApiKeyValidator);
        let app = build_router(validator);

        let (status, _) = send(app, "GET", "/v1/fabric/status", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let entries = syfrah_fabric::audit::read_entries().unwrap();
        let failure_entries: Vec<_> = entries
            .iter()
            .filter(|e| {
                e.details
                    .as_deref()
                    .map(|d| d.contains("api.auth.failure"))
                    .unwrap_or(false)
            })
            .collect();
        assert!(
            !failure_entries.is_empty(),
            "audit log should contain an api.auth.failure event"
        );
        let details = failure_entries.last().unwrap().details.as_deref().unwrap();
        assert!(
            details.contains("trace_id="),
            "failure audit should include trace_id, got: {details}"
        );
    }

    // --- Part 3: auth forbidden emits audit event with key_name ---
    {
        let validator: SharedValidator = Arc::new(ReadOnlyValidator);
        let app = build_router(validator);

        let (status, _) = send(
            app,
            "POST",
            "/v1/fabric/rotate-secret",
            Some("Bearer syf_key_readonly_test"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let entries = syfrah_fabric::audit::read_entries().unwrap();
        let forbidden_entries: Vec<_> = entries
            .iter()
            .filter(|e| {
                e.details
                    .as_deref()
                    .map(|d| d.contains("api.auth.forbidden"))
                    .unwrap_or(false)
            })
            .collect();
        assert!(
            !forbidden_entries.is_empty(),
            "audit log should contain an api.auth.forbidden event"
        );
        let details = forbidden_entries
            .last()
            .unwrap()
            .details
            .as_deref()
            .unwrap();
        assert!(
            details.contains("key_name="),
            "forbidden audit should include key_name, got: {details}"
        );
        assert!(
            details.contains("trace_id="),
            "forbidden audit should include trace_id, got: {details}"
        );
    }
}

// =========================================================================
// CIDR: key with allowed_cidrs rejects requests from other IPs
// =========================================================================

#[test]
fn cidr_allows_matching_ip() {
    use syfrah_api::apikey::{generate_key, validate_key, Role};
    let (raw, mut key) = generate_key("cidr_test", Role::Developer);
    key.allowed_cidrs = vec!["192.168.1.0/24".to_string()];
    let mut keys = vec![key];
    let ip: IpAddr = "192.168.1.50".parse().unwrap();
    let result = validate_key(&raw, &mut keys, Some(ip));
    assert!(result.is_ok(), "IP within CIDR should be accepted");
}

#[test]
fn cidr_rejects_non_matching_ip() {
    use syfrah_api::apikey::{generate_key, validate_key, Role};
    let (raw, mut key) = generate_key("cidr_test", Role::Developer);
    key.allowed_cidrs = vec!["192.168.1.0/24".to_string()];
    let mut keys = vec![key];
    let ip: IpAddr = "10.0.0.1".parse().unwrap();
    let result = validate_key(&raw, &mut keys, Some(ip));
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, "AUTH_FORBIDDEN");
}

#[test]
fn cidr_multiple_ranges_accepts_if_any_match() {
    use syfrah_api::apikey::{generate_key, validate_key, Role};
    let (raw, mut key) = generate_key("cidr_multi", Role::Admin);
    key.allowed_cidrs = vec![
        "10.0.0.0/8".to_string(),
        "172.16.0.0/12".to_string(),
        "192.168.0.0/16".to_string(),
    ];
    let mut keys = vec![key];

    // IP in the second range.
    let ip: IpAddr = "172.20.5.10".parse().unwrap();
    let result = validate_key(&raw, &mut keys, Some(ip));
    assert!(result.is_ok(), "IP matching any CIDR should be accepted");
}

#[test]
fn cidr_multiple_ranges_rejects_if_none_match() {
    use syfrah_api::apikey::{generate_key, validate_key, Role};
    let (raw, mut key) = generate_key("cidr_multi2", Role::Admin);
    key.allowed_cidrs = vec!["10.0.0.0/8".to_string(), "172.16.0.0/12".to_string()];
    let mut keys = vec![key];

    let ip: IpAddr = "192.168.1.1".parse().unwrap();
    let result = validate_key(&raw, &mut keys, Some(ip));
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, "AUTH_FORBIDDEN");
}

#[test]
fn cidr_no_source_ip_with_cidr_set_rejects() {
    use syfrah_api::apikey::{generate_key, validate_key, Role};
    let (raw, mut key) = generate_key("cidr_nosrc", Role::Developer);
    key.allowed_cidrs = vec!["10.0.0.0/8".to_string()];
    let mut keys = vec![key];

    // No source IP provided but CIDR is configured.
    let result = validate_key(&raw, &mut keys, None);
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, "AUTH_FORBIDDEN");
}

#[test]
fn cidr_ipv6_rejects_wrong_prefix() {
    use syfrah_api::apikey::{generate_key, validate_key, Role};
    let (raw, mut key) = generate_key("cidr_v6", Role::Developer);
    key.allowed_cidrs = vec!["fd00::/16".to_string()];
    let mut keys = vec![key];

    let bad_ip: IpAddr = "2001:db8::1".parse().unwrap();
    let result = validate_key(&raw, &mut keys, Some(bad_ip));
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, "AUTH_FORBIDDEN");
}

#[test]
fn cidr_ipv6_accepts_matching_prefix() {
    use syfrah_api::apikey::{generate_key, validate_key, Role};
    let (raw, mut key) = generate_key("cidr_v6ok", Role::Developer);
    key.allowed_cidrs = vec!["fd00::/16".to_string()];
    let mut keys = vec![key];

    let good_ip: IpAddr = "fd00::abcd:1234".parse().unwrap();
    let result = validate_key(&raw, &mut keys, Some(good_ip));
    assert!(result.is_ok());
}

#[test]
fn cidr_empty_allowlist_accepts_any_ip() {
    use syfrah_api::apikey::{generate_key, validate_key, Role};
    let (raw, key) = generate_key("cidr_open", Role::Developer);
    // No CIDRs set (default).
    assert!(key.allowed_cidrs.is_empty());
    let mut keys = vec![key];

    let ip: IpAddr = "8.8.8.8".parse().unwrap();
    let result = validate_key(&raw, &mut keys, Some(ip));
    assert!(result.is_ok(), "empty CIDR allowlist should accept any IP");
}

#[test]
fn cidr_single_ip_slash_32() {
    use syfrah_api::apikey::{generate_key, validate_key, Role};
    let (raw, mut key) = generate_key("cidr_exact", Role::Admin);
    key.allowed_cidrs = vec!["203.0.113.42/32".to_string()];
    let mut keys = vec![key];

    let exact_ip: IpAddr = "203.0.113.42".parse().unwrap();
    assert!(validate_key(&raw, &mut keys, Some(exact_ip)).is_ok());

    let other_ip: IpAddr = "203.0.113.43".parse().unwrap();
    assert!(validate_key(&raw, &mut keys, Some(other_ip)).is_err());
}
