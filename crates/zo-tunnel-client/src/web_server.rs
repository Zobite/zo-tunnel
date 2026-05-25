//! Embedded web server for the client management UI.
//! Serves REST API + static HTML/CSS/JS for managing tunnels.
//! Binds to 127.0.0.1 only (localhost) — no authentication needed.
// TODO(security): If binding to non-localhost is added in the future,
// authentication must be implemented (e.g., token-based auth).

use crate::config::{self, TunnelEntry};
use crate::tunnel_manager::TunnelManager;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::{delete, get, post, put};
use axum::Router;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

// ─── App State ──────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    /// TunnelManager — None until user connects (logs in via web UI)
    inner: Arc<RwLock<Option<Arc<TunnelManager>>>>,
    bind: String,
    port: u16,
}

impl AppState {
    /// Create a new AppState. If credentials exist, initializes TunnelManager.
    pub async fn new(bind: String, port: u16) -> Self {
        let state = Self {
            inner: Arc::new(RwLock::new(None)),
            bind,
            port,
        };

        // Try to load existing credentials
        if let Ok(Some(creds)) = config::SavedCredentials::load() {
            let tunnels_config = config::TunnelsConfig::load().unwrap_or_default();
            let manager = Arc::new(TunnelManager::new(
                creds.server,
                creds.token,
                creds.tls,
                tunnels_config,
            ));
            *state.inner.write().await = Some(manager);
        }

        state
    }

    /// Check if connected (credentials + manager exist).
    pub async fn is_connected(&self) -> bool {
        self.inner.read().await.is_some()
    }

    /// Get the manager (if connected).
    pub async fn manager(&self) -> Option<Arc<TunnelManager>> {
        self.inner.read().await.clone()
    }

    /// Connect: save credentials, create manager, auto-start tunnels.
    pub async fn connect(
        &self,
        server: String,
        token: String,
        tls: bool,
        tls_server_name: Option<String>,
        tls_skip_verify: bool,
    ) -> anyhow::Result<()> {
        // Disconnect first if already connected
        self.disconnect().await?;

        let tls_config = config::ClientTlsConfig {
            enabled: tls,
            server_name: tls_server_name.unwrap_or_default(),
            skip_verify: tls_skip_verify,
        };

        // Save credentials
        let creds = config::SavedCredentials {
            server: server.clone(),
            token: token.clone(),
            tls: tls_config.clone(),
        };
        creds.save()?;

        // Create manager
        let tunnels_config = config::TunnelsConfig::load().unwrap_or_default();
        let manager = Arc::new(TunnelManager::new(
            server,
            token,
            tls_config,
            tunnels_config,
        ));

        // Auto-start enabled tunnels
        manager.start_all().await;

        *self.inner.write().await = Some(manager);

        tracing::info!("🔗 Connected via web UI");
        Ok(())
    }

    /// Disconnect: stop all tunnels, delete credentials.
    pub async fn disconnect(&self) -> anyhow::Result<()> {
        let mut lock = self.inner.write().await;
        if let Some(mgr) = lock.take() {
            mgr.stop_all();
        }
        let _ = config::SavedCredentials::delete();
        tracing::info!("🔌 Disconnected via web UI");
        Ok(())
    }
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Static files (embedded)
        .route("/", get(ui_html))
        .route("/style.css", get(ui_css))
        .route("/app.js", get(ui_js))
        // Auth API
        .route("/api/connect", post(api_connect))
        .route("/api/disconnect", post(api_disconnect))
        // Tunnel CRUD API
        .route("/api/tunnels", get(api_list_tunnels))
        .route("/api/tunnels", post(api_add_tunnel))
        .route("/api/tunnels/:id", put(api_update_tunnel))
        .route("/api/tunnels/:id", delete(api_delete_tunnel))
        // Tunnel actions
        .route("/api/tunnels/:id/start", post(api_start_tunnel))
        .route("/api/tunnels/:id/stop", post(api_stop_tunnel))
        .route("/api/tunnels/:id/restart", post(api_restart_tunnel))
        // Status
        .route("/api/status", get(api_status))
        // Upgrade API
        .route("/api/upgrade/check", get(api_upgrade_check))
        .route("/api/upgrade", post(api_upgrade))
        .with_state(state)
}

// ─── Security Headers ───────────────────────────────────────────

fn security_headers() -> [(header::HeaderName, HeaderValue); 4] {
    [
        (
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; script-src 'self'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; object-src 'none'; frame-ancestors 'none'"
            ),
        ),
        (
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ),
        (
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ),
        (
            http::HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
        ),
    ]
}

// ─── Static Files ───────────────────────────────────────────────

async fn ui_html() -> impl IntoResponse {
    let headers = security_headers();
    (headers, Html(include_str!("../../../web/client/index.html")))
}

async fn ui_css() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/css")],
        include_str!("../../../web/client/style.css"),
    )
}

async fn ui_js() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/javascript")],
        include_str!("../../../web/client/app.js"),
    )
}

// ─── Response Helpers ───────────────────────────────────────────

#[derive(serde::Serialize)]
struct ApiResponse<T: serde::Serialize> {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn ok_response<T: serde::Serialize>(data: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
    })
}

fn err_response(msg: String) -> (StatusCode, Json<ApiResponse<()>>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiResponse {
            success: false,
            data: None,
            error: Some(msg),
        }),
    )
}

/// Return error if not connected yet.
macro_rules! require_connected {
    ($state:expr) => {
        match $state.manager().await {
            Some(mgr) => mgr,
            None => {
                return err_response("Not connected. Please connect first.".into()).into_response();
            }
        }
    };
}

// ─── Auth API ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct ConnectRequest {
    server: String,
    token: String,
    #[serde(default)]
    tls: bool,
    #[serde(default)]
    tls_server_name: Option<String>,
    #[serde(default)]
    tls_skip_verify: bool,
}

async fn api_connect(
    State(state): State<AppState>,
    Json(payload): Json<ConnectRequest>,
) -> impl IntoResponse {
    let server = payload.server.trim().to_string();
    let token = payload.token.trim().to_string();

    if server.is_empty() {
        return err_response("Server address is required".into()).into_response();
    }
    if token.is_empty() {
        return err_response("Token is required".into()).into_response();
    }

    match state
        .connect(server, token, payload.tls, payload.tls_server_name, payload.tls_skip_verify)
        .await
    {
        Ok(_) => ok_response("connected").into_response(),
        Err(e) => err_response(format!("{:#}", e)).into_response(),
    }
}

async fn api_disconnect(State(state): State<AppState>) -> impl IntoResponse {
    match state.disconnect().await {
        Ok(_) => ok_response("disconnected").into_response(),
        Err(e) => err_response(format!("{:#}", e)).into_response(),
    }
}

// ─── Status API ─────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct StatusInfo {
    version: &'static str,
    connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    server: Option<String>,
    total_tunnels: usize,
    running_tunnels: usize,
}

async fn api_status(State(state): State<AppState>) -> impl IntoResponse {
    if let Some(mgr) = state.manager().await {
        let tunnels = mgr.list_tunnels().await;
        ok_response(StatusInfo {
            version: env!("CARGO_PKG_VERSION"),
            connected: true,
            server: Some(mgr.server_addr().to_string()),
            total_tunnels: tunnels.len(),
            running_tunnels: mgr.running_count(),
        })
    } else {
        ok_response(StatusInfo {
            version: env!("CARGO_PKG_VERSION"),
            connected: false,
            server: None,
            total_tunnels: 0,
            running_tunnels: 0,
        })
    }
}

#[derive(serde::Serialize)]
struct UpgradeCheckInfo {
    current: String,
    latest: String,
    upgrade_available: bool,
}

async fn api_upgrade_check() -> impl IntoResponse {
    let current = env!("CARGO_PKG_VERSION").to_string();
    match tokio::task::spawn_blocking(zo_tunnel_protocol::self_update::fetch_latest_version).await {
        Ok(Ok(latest)) => {
            let upgrade_available = zo_tunnel_protocol::self_update::is_newer(&current, &latest);
            ok_response(UpgradeCheckInfo {
                current: format!("v{}", current),
                latest,
                upgrade_available,
            }).into_response()
        }
        _ => err_response("Failed to fetch latest version from GitHub".into()).into_response(),
    }
}

async fn api_upgrade(State(state): State<AppState>) -> impl IntoResponse {
    let current = env!("CARGO_PKG_VERSION");
    
    // Perform self update in blocking task
    let update_result = tokio::task::spawn_blocking(move || {
        zo_tunnel_protocol::self_update::upgrade("zo-tunnel-client", current)
    }).await;

    match update_result {
        Ok(Ok(())) => {
            // Self-upgrade successful! We now schedule a background self-restart.
            // Spawning a shell command that sleeps for 1 second, then starts client again.
            let bind = state.bind.clone();
            let port = state.port;

            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                tracing::info!("🔄 Initiating client self-restart...");

                // Execute background restart script (completely detached using nohup and &)
                let restart_cmd = format!(
                    "nohup sh -c 'sleep 1 && /usr/local/bin/zo-tunnel-client stop && sleep 1 && /usr/local/bin/zo-tunnel-client start --bind {} --port {}' >/dev/null 2>&1 &",
                    bind, port
                );

                let status = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&restart_cmd)
                    .status();

                if let Err(e) = status {
                    tracing::error!("Failed to trigger background self-restart command: {}", e);
                }

                // Exit current process
                std::process::exit(0);
            });

            ok_response("Upgrade successful. Restarting client...").into_response()
        }
        Ok(Err(e)) => err_response(format!("Upgrade failed: {:#}", e)).into_response(),
        Err(e) => err_response(format!("Task panic: {}", e)).into_response(),
    }
}

// ─── Tunnel CRUD API ────────────────────────────────────────────

async fn api_list_tunnels(State(state): State<AppState>) -> impl IntoResponse {
    let mgr = require_connected!(state);
    let tunnels = mgr.list_tunnels().await;
    ok_response(tunnels).into_response()
}

#[derive(Deserialize)]
struct AddTunnelRequest {
    client_id: String,
    local_addr: String,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool {
    true
}

async fn api_add_tunnel(
    State(state): State<AppState>,
    Json(payload): Json<AddTunnelRequest>,
) -> impl IntoResponse {
    let mgr = require_connected!(state);

    let client_id = payload.client_id.trim().to_string();
    let local_addr = payload.local_addr.trim().to_string();

    if client_id.is_empty() {
        return err_response("client_id is required".into()).into_response();
    }
    if local_addr.is_empty() {
        return err_response("local_addr is required".into()).into_response();
    }
    if !client_id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return err_response(
            "client_id must contain only alphanumeric characters, hyphens, and underscores".into(),
        )
        .into_response();
    }

    let entry = TunnelEntry {
        id: String::new(),
        client_id,
        local_addr,
        enabled: payload.enabled,
    };

    match mgr.add_tunnel(entry).await {
        Ok(added) => ok_response(added).into_response(),
        Err(e) => err_response(format!("{:#}", e)).into_response(),
    }
}

#[derive(Deserialize)]
struct UpdateTunnelRequest {
    client_id: String,
    local_addr: String,
    enabled: bool,
}

async fn api_update_tunnel(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateTunnelRequest>,
) -> impl IntoResponse {
    let mgr = require_connected!(state);

    let client_id = payload.client_id.trim().to_string();
    let local_addr = payload.local_addr.trim().to_string();

    if client_id.is_empty() {
        return err_response("client_id is required".into()).into_response();
    }
    if local_addr.is_empty() {
        return err_response("local_addr is required".into()).into_response();
    }
    if !client_id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return err_response(
            "client_id must contain only alphanumeric characters, hyphens, and underscores".into(),
        )
        .into_response();
    }

    match mgr
        .update_tunnel(&id, client_id, local_addr, payload.enabled)
        .await
    {
        Ok(updated) => ok_response(updated).into_response(),
        Err(e) => err_response(format!("{:#}", e)).into_response(),
    }
}

async fn api_delete_tunnel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mgr = require_connected!(state);
    match mgr.remove_tunnel(&id).await {
        Ok(removed) => ok_response(removed).into_response(),
        Err(e) => err_response(format!("{:#}", e)).into_response(),
    }
}

// ─── Tunnel Actions API ─────────────────────────────────────────

async fn api_start_tunnel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mgr = require_connected!(state);
    match mgr.start_tunnel(&id).await {
        Ok(_) => ok_response("started").into_response(),
        Err(e) => err_response(format!("{:#}", e)).into_response(),
    }
}

async fn api_stop_tunnel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mgr = require_connected!(state);
    match mgr.stop_tunnel(&id) {
        Ok(_) => ok_response("stopped").into_response(),
        Err(e) => err_response(format!("{:#}", e)).into_response(),
    }
}

async fn api_restart_tunnel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mgr = require_connected!(state);
    match mgr.restart_tunnel(&id).await {
        Ok(_) => ok_response("restarted").into_response(),
        Err(e) => err_response(format!("{:#}", e)).into_response(),
    }
}
