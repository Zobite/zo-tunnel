//! HTTP reverse proxy — routes public requests to the correct tunnel client.

use crate::config::RoutingMode;
use crate::metrics::{Metrics, RateLimiter};
use crate::registry::Registry;
use anyhow::Context;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio_util::compat::FuturesAsyncReadCompatExt;

type BoxBody = http_body_util::combinators::BoxBody<bytes::Bytes, hyper::Error>;


fn full_body(data: impl Into<bytes::Bytes>) -> BoxBody {
    http_body_util::Full::new(data.into())
        .map_err(|never| match never {})
        .boxed()
}

fn error_response(status: StatusCode, msg: &str) -> Response<BoxBody> {
    let body = format!(
        r#"{{"error":"{}","status":{}}}"#,
        msg,
        status.as_u16()
    );
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("x-powered-by", "zo-tunnel")
        .body(full_body(body))
        .unwrap()
}

/// Extract client_id and forwarded path from an HTTP request.
fn extract_routing(
    req: &Request<Incoming>,
    mode: &RoutingMode,
    domain: Option<&str>,
) -> Option<(String, String)> {
    match mode {
        RoutingMode::Path => {
            let path = req.uri().path();
            // /client_id/rest/of/path → client_id, /rest/of/path
            let trimmed = path.trim_start_matches('/');
            let mut parts = trimmed.splitn(2, '/');
            let client_id = parts.next()?.to_string();
            if client_id.is_empty() {
                return None;
            }
            let rest = parts.next().unwrap_or("");
            let forwarded_path = format!("/{}", rest);
            Some((client_id, forwarded_path))
        }
        RoutingMode::Subdomain => {
            let host = req
                .headers()
                .get("host")
                .and_then(|v| v.to_str().ok())?;
            let domain = domain?;
            // client_id.domain.com → client_id
            let host_no_port = host.split(':').next().unwrap_or(host);
            let suffix = format!(".{}", domain);
            if host_no_port.ends_with(&suffix) {
                let client_id = host_no_port
                    .strip_suffix(&suffix)?
                    .to_string();
                let path = req.uri().path().to_string();
                Some((client_id, path))
            } else {
                None
            }
        }
    }
}

/// Build a forwarded request, rewriting the path and adding proxy headers.
fn build_forwarded_request(
    req: Request<Incoming>,
    path: &str,
    client_id: &str,
) -> anyhow::Result<Request<Incoming>> {
    let (mut parts, body) = req.into_parts();

    // Build new URI with rewritten path
    let query = parts
        .uri
        .query()
        .map(|q| format!("?{}", q))
        .unwrap_or_default();
    let new_uri: hyper::Uri = format!("{}{}", path, query)
        .parse()
        .context("build forwarded uri")?;
    parts.uri = new_uri;

    // Add forwarding headers
    if let Ok(val) = hyper::header::HeaderValue::from_str(client_id) {
        parts.headers.insert("x-zo-tunnel-client", val);
    }

    // Force Connection: close for clean stream lifecycle
    parts.headers.insert(
        hyper::header::CONNECTION,
        hyper::header::HeaderValue::from_static("close"),
    );

    Ok(Request::from_parts(parts, body))
}

/// Handle an incoming public HTTP request by routing it through a tunnel.
pub async fn handle_proxy_request(
    req: Request<Incoming>,
    registry: Arc<Registry>,
    metrics: Arc<Metrics>,
    rate_limiter: Arc<RateLimiter>,
    routing_mode: RoutingMode,
    domain: Option<String>,
) -> Result<Response<BoxBody>, hyper::Error> {
    metrics.total_requests.fetch_add(1, Ordering::Relaxed);

    // Extract routing
    let (client_id, path) = match extract_routing(&req, &routing_mode, domain.as_deref()) {
        Some(r) => r,
        None => {
            // No routing info — list available clients
            let clients = registry.list();
            if clients.is_empty() {
                return Ok(error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "No tunnel clients connected",
                ));
            }
            let client_list: Vec<String> = clients.iter().map(|c| c.client_id.clone()).collect();
            let msg = format!(
                "Available tunnels: {}. Use /<tunnel_name>/your/path to route.",
                client_list.join(", ")
            );
            return Ok(error_response(StatusCode::NOT_FOUND, &msg));
        }
    };

    // Rate limiting
    if !rate_limiter.check(&client_id) {
        metrics.rate_limited.fetch_add(1, Ordering::Relaxed);
        return Ok(error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "Rate limit exceeded",
        ));
    }

    // Find the client
    let client = match registry.get(&client_id) {
        Some(c) => c,
        None => {
            return Ok(error_response(
                StatusCode::BAD_GATEWAY,
                &format!("Tunnel '{}' not found or offline", client_id),
            ));
        }
    };

    // Track metrics
    client.metrics.requests.fetch_add(1, Ordering::Relaxed);
    client.metrics.active_streams.fetch_add(1, Ordering::Relaxed);
    metrics.active_connections.fetch_add(1, Ordering::Relaxed);

    // Open yamux stream to the client
    let yamux_stream = match client.handle.open_stream().await {
        Ok(s) => s,
        Err(e) => {
            client.metrics.active_streams.fetch_sub(1, Ordering::Relaxed);
            metrics.active_connections.fetch_sub(1, Ordering::Relaxed);
            tracing::error!("Failed to open yamux stream to '{}': {}", client_id, e);
            return Ok(error_response(
                StatusCode::BAD_GATEWAY,
                &format!("Tunnel '{}' connection error", client_id),
            ));
        }
    };

    // Compat: yamux uses futures-io, hyper needs tokio-io
    let compat_stream = yamux_stream.compat();
    let io = TokioIo::new(compat_stream);

    // HTTP/1.1 handshake over the yamux stream
    let (mut sender, conn) = match hyper::client::conn::http1::handshake(io).await {
        Ok(r) => r,
        Err(e) => {
            client.metrics.active_streams.fetch_sub(1, Ordering::Relaxed);
            metrics.active_connections.fetch_sub(1, Ordering::Relaxed);
            tracing::error!("HTTP handshake to '{}' failed: {}", client_id, e);
            return Ok(error_response(
                StatusCode::BAD_GATEWAY,
                "Tunnel HTTP handshake failed",
            ));
        }
    };

    // Drive the HTTP connection in background
    let cid = client_id.clone();
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            tracing::debug!("HTTP conn to '{}' done: {}", cid, e);
        }
    });

    // Build and forward the request
    let forwarded = match build_forwarded_request(req, &path, &client_id) {
        Ok(r) => r,
        Err(e) => {
            client.metrics.active_streams.fetch_sub(1, Ordering::Relaxed);
            metrics.active_connections.fetch_sub(1, Ordering::Relaxed);
            tracing::error!("Build forwarded request failed: {}", e);
            return Ok(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Request forwarding error",
            ));
        }
    };

    let resp = match sender.send_request(forwarded).await {
        Ok(resp) => resp,
        Err(e) => {
            client.metrics.active_streams.fetch_sub(1, Ordering::Relaxed);
            metrics.active_connections.fetch_sub(1, Ordering::Relaxed);
            tracing::error!("Proxy request to '{}' failed: {}", client_id, e);
            return Ok(error_response(
                StatusCode::BAD_GATEWAY,
                &format!("Tunnel '{}' did not respond", client_id),
            ));
        }
    };

    client.metrics.active_streams.fetch_sub(1, Ordering::Relaxed);
    metrics.active_connections.fetch_sub(1, Ordering::Relaxed);

    tracing::info!(
        "→ {} {} → '{}' → {}",
        resp.status().as_u16(),
        path,
        client_id,
        resp.status()
    );

    // Convert response body
    let (parts, body) = resp.into_parts();
    let boxed = body
        .map_err(|e| {
            tracing::debug!("Body error: {}", e);
            e
        })
        .boxed();
    Ok(Response::from_parts(parts, boxed))
}
