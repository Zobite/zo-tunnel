//! Dashboard REST API + static file server.

use crate::metrics::Metrics;
use crate::registry::Registry;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use serde::Serialize;
use std::sync::Arc;

#[derive(Clone)]
pub struct DashboardState {
    pub registry: Arc<Registry>,
    pub metrics: Arc<Metrics>,
}

pub fn create_router(state: DashboardState) -> Router {
    Router::new()
        .route("/api/status", get(api_status))
        .route("/api/clients", get(api_clients))
        .route("/api/metrics", get(api_metrics))
        .route("/", get(dashboard_ui))
        .route("/style.css", get(dashboard_css))
        .route("/app.js", get(dashboard_js))
        .with_state(state)
}

// ─── API Handlers ────────────────────────────────────────────────

#[derive(Serialize)]
struct StatusResponse {
    status: &'static str,
    version: &'static str,
    connected_clients: usize,
}

async fn api_status(State(state): State<DashboardState>) -> impl IntoResponse {
    Json(StatusResponse {
        status: "running",
        version: env!("CARGO_PKG_VERSION"),
        connected_clients: state.registry.count(),
    })
}

async fn api_clients(State(state): State<DashboardState>) -> impl IntoResponse {
    Json(state.registry.list())
}

async fn api_metrics(State(state): State<DashboardState>) -> impl IntoResponse {
    Json(state.metrics.snapshot())
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
