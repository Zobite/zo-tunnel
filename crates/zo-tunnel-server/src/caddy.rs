//! Caddy integration — auto-detect, register, and chain on-demand TLS.
//!
//! Caddy uses On-Demand TLS: it provisions certificates automatically when the
//! first request arrives for a new subdomain. Caddy calls a configured `ask`
//! endpoint to verify that a subdomain should receive a certificate.
//!
//! **Auto-detect & chain**: On startup, zo-tunnel reads the existing Caddy
//! on-demand TLS config via the Admin API. If another app already has an `ask`
//! endpoint configured, zo-tunnel saves it as a fallback and sets itself as
//! the new endpoint. TLS check requests for domains outside `*.{base_domain}`
//! are forwarded to the fallback — so multiple apps can share Caddy's single
//! global on-demand TLS `ask` endpoint seamlessly.

use http_body_util::BodyExt;
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Caddy integration config.
/// Auto-detected: if Caddy is installed, integration is enabled automatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaddyConfig {
    /// Enable Caddy integration (auto-detected if not set)
    #[serde(default)]
    pub enabled: bool,
    /// Caddy Admin API address (default: http://localhost:2019)
    #[serde(default = "default_admin_api")]
    pub admin_api: String,
}

impl Default for CaddyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            admin_api: default_admin_api(),
        }
    }
}

fn default_admin_api() -> String {
    "http://localhost:2019".into()
}

impl CaddyConfig {
    /// Auto-detect Caddy: check if the binary exists or /etc/caddy/ is present.
    pub fn auto_detect() -> Self {
        // Check if Caddy config directory exists
        if std::path::Path::new("/etc/caddy").is_dir() {
            return Self {
                enabled: true,
                admin_api: default_admin_api(),
            };
        }

        // Check if caddy binary is in PATH
        if let Ok(output) = std::process::Command::new("which").arg("caddy").output() {
            if output.status.success() {
                return Self {
                    enabled: true,
                    admin_api: default_admin_api(),
                };
            }
        }

        Self::default()
    }
}

// ─── CaddyManager ───────────────────────────────────────────────

/// Manages Caddy integration: auto-registers zo-tunnel as the on-demand TLS
/// check endpoint and chains to any previously-configured endpoint.
///
/// On startup:
///   1. Reads the current `on_demand.permission.endpoint` from Caddy Admin API
///   2. If one exists (and it's not ours), saves it as a fallback
///   3. Sets zo-tunnel's `/api/tls-check` as the new endpoint
///
/// On TLS check:
///   - Domains matching `*.{base_domain}` → handled by zo-tunnel
///   - Other domains → forwarded to the fallback endpoint
///
/// On shutdown:
///   - Restores the original fallback endpoint
pub struct CaddyManager {
    admin_api: String,
    our_endpoint: String,
    fallback_endpoint: RwLock<Option<String>>,
}

impl CaddyManager {
    pub fn new(admin_api: String, our_endpoint: String) -> Self {
        Self {
            admin_api,
            our_endpoint,
            fallback_endpoint: RwLock::new(None),
        }
    }

    /// Register with Caddy: read existing on-demand config, save fallback, set ours.
    pub async fn register(&self) -> anyhow::Result<()> {
        // 1. Read current on-demand permission endpoint from Caddy Admin API
        let get_url = format!(
            "{}/config/apps/tls/automation/on_demand/permission/endpoint",
            self.admin_api
        );

        match http_get(&get_url).await {
            Ok((200, body)) => {
                // Body is a JSON string like "http://localhost:2615/api/sites/check-domain"
                let existing: String = serde_json::from_str(body.trim())
                    .unwrap_or_else(|_| body.trim().trim_matches('"').to_string());

                if !existing.is_empty() && existing != self.our_endpoint {
                    tracing::info!(
                        "🔗 Caddy: found existing on-demand endpoint: {} → saved as fallback",
                        existing
                    );
                    *self.fallback_endpoint.write().await = Some(existing);
                }
            }
            Ok((status, _)) => {
                tracing::debug!(
                    "Caddy: no existing on-demand TLS endpoint (status {})",
                    status
                );
            }
            Err(e) => {
                tracing::debug!("Caddy: could not read on-demand config: {}", e);
            }
        }

        // 2. Set our endpoint via Caddy Admin API (PATCH)
        let patch_url = format!(
            "{}/config/apps/tls/automation/on_demand/permission/endpoint",
            self.admin_api
        );
        let body = serde_json::to_string(&self.our_endpoint)?;

        match http_request("PATCH", &patch_url, Some(&body)).await {
            Ok((200, _)) => {
                tracing::info!(
                    "✅ Caddy: registered as on-demand TLS endpoint: {}",
                    self.our_endpoint
                );
                let fallback = self.fallback_endpoint.read().await;
                if let Some(ref fb) = *fallback {
                    tracing::info!("🔗 Caddy: non-tunnel domains will chain to: {}", fb);
                }
                Ok(())
            }
            Ok((status, resp_body)) => {
                anyhow::bail!(
                    "Caddy Admin API returned {} when setting endpoint: {}",
                    status,
                    resp_body.chars().take(200).collect::<String>()
                );
            }
            Err(e) => {
                anyhow::bail!("Failed to set Caddy on-demand endpoint: {}", e);
            }
        }
    }

    /// Unregister from Caddy: restore the fallback endpoint on shutdown.
    /// If no fallback was saved, does nothing.
    pub async fn unregister(&self) {
        let fallback = self.fallback_endpoint.read().await;
        if let Some(ref endpoint) = *fallback {
            let patch_url = format!(
                "{}/config/apps/tls/automation/on_demand/permission/endpoint",
                self.admin_api
            );
            let body = match serde_json::to_string(endpoint) {
                Ok(b) => b,
                Err(_) => return,
            };

            match http_request("PATCH", &patch_url, Some(&body)).await {
                Ok((200, _)) => {
                    tracing::info!("🔗 Caddy: restored on-demand TLS endpoint: {}", endpoint);
                }
                Ok((status, _)) => {
                    tracing::warn!("Caddy: failed to restore endpoint (status {})", status);
                }
                Err(e) => {
                    tracing::warn!("Caddy: failed to restore endpoint: {}", e);
                }
            }
        }
    }

    /// Start a background watchdog that periodically checks if zo-tunnel is
    /// still the configured on-demand TLS endpoint. If another app overwrites
    /// it (e.g. installed after zo-tunnel), the watchdog detects this, saves
    /// the new endpoint as fallback, and re-registers zo-tunnel.
    pub fn start_watchdog(self: &std::sync::Arc<Self>) {
        let mgr = std::sync::Arc::clone(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                mgr.check_and_reregister().await;
            }
        });
    }

    /// Check if we're still the configured endpoint; if not, re-register.
    async fn check_and_reregister(&self) {
        let get_url = format!(
            "{}/config/apps/tls/automation/on_demand/permission/endpoint",
            self.admin_api
        );

        let current = match http_get(&get_url).await {
            Ok((200, body)) => serde_json::from_str(body.trim())
                .unwrap_or_else(|_| body.trim().trim_matches('"').to_string()),
            _ => return, // Caddy not reachable or no config — skip
        };

        if current == self.our_endpoint {
            return; // Still us, all good
        }

        // Someone overwrote our endpoint — save as new fallback and re-register
        if !current.is_empty() {
            tracing::info!(
                "🔁 Caddy watchdog: endpoint was changed to '{}' → saving as fallback",
                current
            );
            *self.fallback_endpoint.write().await = Some(current);
        }

        let patch_url = format!(
            "{}/config/apps/tls/automation/on_demand/permission/endpoint",
            self.admin_api
        );
        let body = match serde_json::to_string(&self.our_endpoint) {
            Ok(b) => b,
            Err(_) => return,
        };

        match http_request("PATCH", &patch_url, Some(&body)).await {
            Ok((200, _)) => {
                tracing::info!("✅ Caddy watchdog: re-registered as on-demand TLS endpoint");
            }
            Ok((status, _)) => {
                tracing::warn!("Caddy watchdog: re-register failed (status {})", status);
            }
            Err(e) => {
                tracing::warn!("Caddy watchdog: re-register failed: {}", e);
            }
        }
    }

    /// Check the fallback endpoint for a domain.
    /// Returns `Some(status_code)` if a fallback exists and responds, `None` otherwise.
    pub async fn check_fallback(&self, domain: &str) -> Option<u16> {
        let fallback = self.fallback_endpoint.read().await;
        let endpoint = fallback.as_ref()?;

        // Build the fallback URL with domain query parameter
        let separator = if endpoint.contains('?') { '&' } else { '?' };
        let url = format!("{}{}domain={}", endpoint, separator, domain);

        match http_get(&url).await {
            Ok((status, _)) => {
                tracing::debug!(
                    "TLS fallback: '{}' → {} (status {})",
                    domain,
                    endpoint,
                    status
                );
                Some(status)
            }
            Err(e) => {
                tracing::warn!("TLS fallback: '{}' → {} failed: {}", domain, endpoint, e);
                None
            }
        }
    }
}

// ─── HTTP Helpers (using hyper client) ──────────────────────────

/// Parse a URL into (host, port, path_and_query).
fn parse_url(url: &str) -> anyhow::Result<(String, u16, String)> {
    let uri: hyper::Uri = url.parse()?;
    let host = uri.host().unwrap_or("localhost").to_string();
    let port = uri.port_u16().unwrap_or(80);
    let path = uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());
    Ok((host, port, path))
}

/// Simple HTTP GET request. Returns (status_code, body_string).
async fn http_get(url: &str) -> anyhow::Result<(u16, String)> {
    http_request("GET", url, None).await
}

/// Simple HTTP request with optional JSON body. Returns (status_code, body_string).
async fn http_request(
    method: &str,
    url: &str,
    body: Option<&str>,
) -> anyhow::Result<(u16, String)> {
    let (host, port, path) = parse_url(url)?;

    // Use 127.0.0.1 for localhost to avoid IPv6 resolution issues
    // (many services bind to 127.0.0.1 only, not ::1)
    let connect_host = if host == "localhost" {
        "127.0.0.1"
    } else {
        &host
    };

    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::net::TcpStream::connect(format!("{}:{}", connect_host, port)),
    )
    .await
    .map_err(|_| anyhow::anyhow!("connect timeout to {}:{}", host, port))?
    .map_err(|e| anyhow::anyhow!("connect to {}:{}: {}", host, port, e))?;

    let io = TokioIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|e| anyhow::anyhow!("HTTP handshake: {}", e))?;

    // Drive the HTTP connection in background
    tokio::spawn(async move {
        let _ = conn.await;
    });

    // Build request body (Full for non-empty, Empty for GET/no-body)
    let req_body = match body {
        Some(b) => http_body_util::Full::new(bytes::Bytes::from(b.to_string())).boxed(),
        None => http_body_util::Empty::<bytes::Bytes>::new().boxed(),
    };

    // Host header must include port (Caddy Admin API validates this strictly)
    let host_header = if port == 80 {
        host.clone()
    } else {
        format!("{}:{}", host, port)
    };

    let req = hyper::Request::builder()
        .method(method)
        .uri(&path)
        .header("host", &host_header)
        .header("content-type", "application/json")
        .body(req_body)
        .map_err(|e| anyhow::anyhow!("build request: {}", e))?;

    let resp = tokio::time::timeout(std::time::Duration::from_secs(5), sender.send_request(req))
        .await
        .map_err(|_| anyhow::anyhow!("request timeout to {}", url))?
        .map_err(|e| anyhow::anyhow!("request failed: {}", e))?;

    let status = resp.status().as_u16();
    let body_bytes = resp
        .into_body()
        .collect()
        .await
        .map_err(|e| anyhow::anyhow!("read body: {}", e))?
        .to_bytes();
    let body_str = String::from_utf8_lossy(&body_bytes).to_string();

    Ok((status, body_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_disabled() {
        let cfg = CaddyConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.admin_api, "http://localhost:2019");
    }

    #[test]
    fn test_serde_roundtrip() {
        let cfg = CaddyConfig {
            enabled: true,
            admin_api: "http://localhost:2019".into(),
        };
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let parsed: CaddyConfig = serde_yaml::from_str(&yaml).unwrap();
        assert!(parsed.enabled);
        assert_eq!(parsed.admin_api, "http://localhost:2019");
    }

    #[test]
    fn test_serde_backward_compat() {
        // Existing configs without admin_api field should use default
        let yaml = "enabled: true\n";
        let parsed: CaddyConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(parsed.enabled);
        assert_eq!(parsed.admin_api, "http://localhost:2019");
    }

    #[test]
    fn test_parse_url_basic() {
        let (host, port, path) = parse_url("http://localhost:2019/config/test").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 2019);
        assert_eq!(path, "/config/test");
    }

    #[test]
    fn test_parse_url_default_port() {
        let (host, port, path) = parse_url("http://example.com/path").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 80);
        assert_eq!(path, "/path");
    }

    #[test]
    fn test_parse_url_with_query() {
        let (_, _, path) = parse_url("http://localhost:2615/api/check?domain=test.com").unwrap();
        assert_eq!(path, "/api/check?domain=test.com");
    }
}
