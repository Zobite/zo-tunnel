//! Dashboard REST API + static file server with authentication.

use crate::metrics::Metrics;
use crate::registry::Registry;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

// ─── Session Management ─────────────────────────────────────────

/// Active session entry.
struct Session {
    created_at: Instant,
}

/// Thread-safe session store.
pub struct SessionStore {
    sessions: DashMap<String, Session>,
    ttl_secs: u64,
}

impl SessionStore {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            sessions: DashMap::new(),
            ttl_secs,
        }
    }

    /// Create a new session, returns session ID.
    pub fn create(&self) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.sessions.insert(
            id.clone(),
            Session {
                created_at: Instant::now(),
            },
        );
        id
    }

    /// Validate a session ID. Returns true if valid and not expired.
    pub fn validate(&self, session_id: &str) -> bool {
        if let Some(entry) = self.sessions.get(session_id) {
            if entry.created_at.elapsed().as_secs() < self.ttl_secs {
                return true;
            }
            // Expired — remove it
            drop(entry);
            self.sessions.remove(session_id);
        }
        false
    }

    /// Invalidate (remove) a session.
    pub fn invalidate(&self, session_id: &str) {
        self.sessions.remove(session_id);
    }
}

// ─── Dashboard State ────────────────────────────────────────────

#[derive(Clone)]
pub struct DashboardState {
    pub registry: Arc<Registry>,
    pub metrics: Arc<Metrics>,
    pub dashboard_token: String,
    pub auth_enabled: bool,
    pub tls_enabled: bool,
    pub domain: String,
    pub sessions: Arc<SessionStore>,
    pub caddy_manager: Option<Arc<crate::caddy::CaddyManager>>,
}

pub fn create_router(state: DashboardState) -> Router {
    Router::new()
        // Public routes (no auth required)
        .route("/", get(dashboard_ui))
        .route("/style.css", get(dashboard_css))
        .route("/app.js", get(dashboard_js))
        .route("/api/login", post(api_login))
        .route("/api/auth/check", get(api_auth_check))
        .route("/api/tls-check", get(api_tls_check))
        // Protected routes (auth required)
        .route("/api/status", get(api_status))
        .route("/api/clients", get(api_clients))
        .route("/api/metrics", get(api_metrics))
        .route("/api/logout", post(api_logout))
        .with_state(state)
}

// ─── Cookie Helpers ─────────────────────────────────────────────

const COOKIE_NAME: &str = "zo-session";

/// Extract session ID from Cookie header.
fn extract_session_id(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix(COOKIE_NAME) {
            let value = value.strip_prefix('=')?;
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Build a Set-Cookie header value for a session.
fn build_session_cookie(session_id: &str, max_age_secs: u64, tls_enabled: bool) -> String {
    let mut cookie = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
        COOKIE_NAME, session_id, max_age_secs
    );
    if tls_enabled {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Build a Set-Cookie header to clear the session cookie.
fn build_clear_cookie() -> String {
    format!(
        "{}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0",
        COOKIE_NAME
    )
}

/// Check if the request is authenticated. Returns true if auth is disabled or session is valid.
fn is_authenticated(state: &DashboardState, headers: &HeaderMap) -> bool {
    if !state.auth_enabled {
        return true;
    }
    if let Some(session_id) = extract_session_id(headers) {
        return state.sessions.validate(&session_id);
    }
    false
}

// ─── Auth API Handlers ──────────────────────────────────────────

#[derive(Deserialize)]
struct LoginRequest {
    token: String,
}

#[derive(Serialize)]
struct LoginResponse {
    success: bool,
    message: String,
}

#[derive(Serialize)]
struct AuthCheckResponse {
    authenticated: bool,
    auth_required: bool,
    tls_enabled: bool,
}

/// Helper to detect if TLS is enabled (natively or via reverse proxy like Caddy/Nginx).
fn is_tls_enabled(state: &DashboardState, headers: &HeaderMap) -> bool {
    if state.tls_enabled {
        return true;
    }
    if let Some(proto) = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) {
        if proto.trim().eq_ignore_ascii_case("https") {
            return true;
        }
    }
    false
}

async fn api_login(
    State(state): State<DashboardState>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    // If auth is not enabled, always succeed
    if !state.auth_enabled {
        return (
            StatusCode::OK,
            HeaderMap::new(),
            Json(LoginResponse {
                success: true,
                message: "Authentication not required".into(),
            }),
        );
    }

    // Rate-limit: check if there's already a valid session to prevent abuse
    if is_authenticated(&state, &headers) {
        return (
            StatusCode::OK,
            HeaderMap::new(),
            Json(LoginResponse {
                success: true,
                message: "Already authenticated".into(),
            }),
        );
    }

    // Validate the token
    use crate::config::ServerConfig;
    let mut check_cfg = ServerConfig::default();
    check_cfg.dashboard_auth.token = state.dashboard_token.clone();

    if check_cfg.validate_dashboard_token(&payload.token) {
        let session_id = state.sessions.create();
        let tls_enabled = is_tls_enabled(&state, &headers);
        let cookie = build_session_cookie(
            &session_id,
            state.sessions.ttl_secs,
            tls_enabled,
        );

        let mut resp_headers = HeaderMap::new();
        resp_headers.insert(
            header::SET_COOKIE,
            cookie.parse().expect("valid cookie header"),
        );

        tracing::info!("🔓 Dashboard login successful");

        (
            StatusCode::OK,
            resp_headers,
            Json(LoginResponse {
                success: true,
                message: "Login successful".into(),
            }),
        )
    } else {
        tracing::warn!("🔒 Dashboard login failed: invalid token");

        (
            StatusCode::UNAUTHORIZED,
            HeaderMap::new(),
            Json(LoginResponse {
                success: false,
                message: "Invalid admin token".into(),
            }),
        )
    }
}

async fn api_auth_check(
    State(state): State<DashboardState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    Json(AuthCheckResponse {
        authenticated: is_authenticated(&state, &headers),
        auth_required: state.auth_enabled,
        tls_enabled: is_tls_enabled(&state, &headers),
    })
}

async fn api_logout(
    State(state): State<DashboardState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Invalidate the session
    if let Some(session_id) = extract_session_id(&headers) {
        state.sessions.invalidate(&session_id);
    }

    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(
        header::SET_COOKIE,
        build_clear_cookie().parse().expect("valid cookie header"),
    );

    (
        StatusCode::OK,
        resp_headers,
        Json(LoginResponse {
            success: true,
            message: "Logged out".into(),
        }),
    )
}

// ─── Protected API Handlers ─────────────────────────────────────

#[derive(Serialize)]
struct StatusResponse {
    status: &'static str,
    version: &'static str,
    connected_clients: usize,
}

async fn api_status(
    State(state): State<DashboardState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_authenticated(&state, &headers) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Authentication required"})),
        ));
    }
    Ok(Json(StatusResponse {
        status: "running",
        version: env!("CARGO_PKG_VERSION"),
        connected_clients: state.registry.count(),
    }))
}

async fn api_clients(
    State(state): State<DashboardState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_authenticated(&state, &headers) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Authentication required"})),
        ));
    }
    Ok(Json(state.registry.list()))
}

async fn api_metrics(
    State(state): State<DashboardState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !is_authenticated(&state, &headers) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Authentication required"})),
        ));
    }
    Ok(Json(state.metrics.snapshot()))
}

// ─── Caddy On-Demand TLS Check ──────────────────────────────────

#[derive(Deserialize)]
struct TlsCheckQuery {
    domain: String,
}

/// Caddy On-Demand TLS check endpoint.
/// Called by Caddy before issuing a certificate for a subdomain.
///
/// Returns 200 OK if the subdomain belongs to a connected client
/// or is a reserved subdomain (e.g. "dashboard").
/// Returns 403 Forbidden otherwise (tells Caddy to reject the request).
///
/// No authentication required — this endpoint is called internally by Caddy.
async fn api_tls_check(
    State(state): State<DashboardState>,
    axum::extract::Query(query): axum::extract::Query<TlsCheckQuery>,
) -> impl IntoResponse {
    let requested_domain = &query.domain;

    // Extract the subdomain: "my-api.tunnel.example.com" → "my-api"
    let subdomain = if let Some(sub) = requested_domain.strip_suffix(&format!(".{}", state.domain))
    {
        sub
    } else {
        // Domain doesn't match our base domain — try fallback chain
        if let Some(ref mgr) = state.caddy_manager {
            if let Some(status) = mgr.check_fallback(requested_domain).await {
                tracing::debug!(
                    "TLS check: forwarded '{}' to fallback → {}",
                    requested_domain,
                    status
                );
                return StatusCode::from_u16(status).unwrap_or(StatusCode::FORBIDDEN);
            }
        }
        tracing::debug!(
            "TLS check: rejected '{}' (not under *.{})",
            requested_domain,
            state.domain
        );
        return StatusCode::FORBIDDEN;
    };

    // Allow reserved subdomains (dashboard, etc.)
    if crate::config::RESERVED_SUBDOMAINS.contains(&subdomain) {
        tracing::debug!("TLS check: approved '{}' (reserved subdomain)", requested_domain);
        return StatusCode::OK;
    }

    // Check if the subdomain corresponds to a connected client
    if state.registry.get(subdomain).is_some() {
        tracing::debug!("TLS check: approved '{}' (client connected)", requested_domain);
        return StatusCode::OK;
    }

    tracing::debug!(
        "TLS check: rejected '{}' (no connected client '{}')",
        requested_domain,
        subdomain
    );
    StatusCode::FORBIDDEN
}

// ─── Embedded Dashboard UI ──────────────────────────────────────

async fn dashboard_ui() -> impl IntoResponse {
    Html(include_str!("../../../web/index.html"))
}

async fn dashboard_css() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "text/css")],
        include_str!("../../../web/style.css"),
    )
}

async fn dashboard_js() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "application/javascript")],
        include_str!("../../../web/app.js"),
    )
}
