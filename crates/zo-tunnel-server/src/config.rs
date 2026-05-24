//! Server configuration — YAML file + CLI override.

use serde::{Deserialize, Serialize};
use std::path::Path;
use subtle::ConstantTimeEq;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_control_port")]
    pub control_port: u16,

    #[serde(default = "default_public_port")]
    pub public_port: u16,

    #[serde(default = "default_dashboard_port")]
    pub dashboard_port: u16,

    #[serde(default)]
    pub routing_mode: RoutingMode,

    #[serde(default)]
    pub domain: Option<String>,

    #[serde(default)]
    pub tls: TlsConfig,

    #[serde(default)]
    pub auth: AuthConfig,

    #[serde(default)]
    pub dashboard_auth: DashboardAuthConfig,

    #[serde(default)]
    pub rate_limit: RateLimitConfig,

    /// TCP port range for dedicated per-client TCP forwarding
    #[serde(default)]
    pub tcp_ports: TcpPortConfig,

    #[serde(default = "default_log_level")]
    pub log_level: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RoutingMode {
    #[default]
    Path,
    Subdomain,
}


#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TlsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub cert: String,
    #[serde(default)]
    pub key: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub tokens: Vec<String>,
}

/// Dashboard authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardAuthConfig {
    /// Token required to access the dashboard. If empty, dashboard is open.
    #[serde(default)]
    pub token: String,
    /// Session cookie TTL in seconds (default: 24 hours).
    #[serde(default = "default_session_ttl")]
    pub session_ttl_secs: u64,
}

impl Default for DashboardAuthConfig {
    fn default() -> Self {
        Self {
            token: String::new(),
            session_ttl_secs: default_session_ttl(),
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_rps")]
    pub requests_per_second: u32,
    #[serde(default = "default_max_conn")]
    pub max_connections_per_client: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 100,
            max_connections_per_client: 50,
        }
    }
}

/// TCP port range configuration for dedicated TCP tunnels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcpPortConfig {
    /// Enable dedicated TCP port allocation
    #[serde(default)]
    pub enabled: bool,
    /// Start of TCP port range (inclusive)
    #[serde(default = "default_tcp_start")]
    pub port_start: u16,
    /// End of TCP port range (inclusive)
    #[serde(default = "default_tcp_end")]
    pub port_end: u16,
}

impl Default for TcpPortConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port_start: 10000,
            port_end: 10100,
        }
    }
}

fn default_control_port() -> u16 {
    zo_tunnel_protocol::DEFAULT_CONTROL_PORT
}
fn default_public_port() -> u16 {
    zo_tunnel_protocol::DEFAULT_PUBLIC_PORT
}
fn default_dashboard_port() -> u16 {
    zo_tunnel_protocol::DEFAULT_DASHBOARD_PORT
}
fn default_log_level() -> String {
    "info".into()
}
fn default_tcp_start() -> u16 {
    10000
}
fn default_tcp_end() -> u16 {
    10100
}
fn default_rps() -> u32 {
    100
}
fn default_max_conn() -> u32 {
    50
}
fn default_session_ttl() -> u64 {
    86400 // 24 hours
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            control_port: default_control_port(),
            public_port: default_public_port(),
            dashboard_port: default_dashboard_port(),
            routing_mode: RoutingMode::default(),
            domain: None,
            tls: TlsConfig::default(),
            auth: AuthConfig::default(),
            dashboard_auth: DashboardAuthConfig::default(),
            rate_limit: RateLimitConfig::default(),
            tcp_ports: TcpPortConfig::default(),
            log_level: default_log_level(),
        }
    }
}

impl ServerConfig {
    /// Load config from YAML file, falling back to defaults.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: ServerConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// Check if a token is valid. If no tokens configured, all are accepted.
    pub fn validate_token(&self, token: &str) -> bool {
        if self.auth.tokens.is_empty() {
            return true;
        }
        self.auth.tokens.iter().any(|t| t == token)
    }

    /// Check if the dashboard requires authentication.
    pub fn dashboard_auth_enabled(&self) -> bool {
        !self.dashboard_auth.token.is_empty()
    }

    /// Validate a dashboard token using constant-time comparison.
    pub fn validate_dashboard_token(&self, token: &str) -> bool {
        if !self.dashboard_auth_enabled() {
            return true; // no auth configured → open access
        }
        let expected = self.dashboard_auth.token.as_bytes();
        let provided = token.as_bytes();
        // Constant-time comparison to prevent timing attacks
        expected.len() == provided.len() && expected.ct_eq(provided).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = ServerConfig::default();
        assert_eq!(cfg.control_port, 6200);
        assert_eq!(cfg.public_port, 6210);
        assert_eq!(cfg.dashboard_port, 6220);
        assert_eq!(cfg.routing_mode, RoutingMode::Path);
        assert!(!cfg.tls.enabled);
        assert!(cfg.auth.tokens.is_empty());
        assert!(cfg.dashboard_auth.token.is_empty());
        assert_eq!(cfg.dashboard_auth.session_ttl_secs, 86400);
    }

    #[test]
    fn test_validate_token_empty_allows_all() {
        let cfg = ServerConfig::default();
        assert!(cfg.validate_token("anything"));
        assert!(cfg.validate_token(""));
    }

    #[test]
    fn test_validate_token_checks_list() {
        let mut cfg = ServerConfig::default();
        cfg.auth.tokens = vec!["secret1".into(), "secret2".into()];

        assert!(cfg.validate_token("secret1"));
        assert!(cfg.validate_token("secret2"));
        assert!(!cfg.validate_token("wrong"));
        assert!(!cfg.validate_token(""));
    }

    #[test]
    fn test_yaml_parsing() {
        let yaml = r#"
control_port: 7777
public_port: 8888
dashboard_port: 9999
auth:
  tokens:
    - "tok1"
    - "tok2"
"#;
        let cfg: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.control_port, 7777);
        assert_eq!(cfg.public_port, 8888);
        assert_eq!(cfg.dashboard_port, 9999);
        assert_eq!(cfg.auth.tokens, vec!["tok1", "tok2"]);
    }

    #[test]
    fn test_yaml_defaults_for_missing_fields() {
        let yaml = "{}";
        let cfg: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.control_port, 6200);
        assert_eq!(cfg.public_port, 6210);
        assert_eq!(cfg.dashboard_port, 6220);
        assert_eq!(cfg.log_level, "info");
    }

    #[test]
    fn test_tcp_port_config_defaults() {
        let cfg = TcpPortConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.port_start, 10000);
        assert_eq!(cfg.port_end, 10100);
    }

    #[test]
    fn test_dashboard_auth_disabled_by_default() {
        let cfg = ServerConfig::default();
        assert!(!cfg.dashboard_auth_enabled());
        // When no token is configured, all tokens are accepted
        assert!(cfg.validate_dashboard_token("anything"));
    }

    #[test]
    fn test_dashboard_auth_validates_token() {
        let mut cfg = ServerConfig::default();
        cfg.dashboard_auth.token = "super-secret-admin".into();
        assert!(cfg.dashboard_auth_enabled());
        assert!(cfg.validate_dashboard_token("super-secret-admin"));
        assert!(!cfg.validate_dashboard_token("wrong-token"));
        assert!(!cfg.validate_dashboard_token(""));
        // Different length should also fail
        assert!(!cfg.validate_dashboard_token("super-secret-admin-extra"));
        assert!(!cfg.validate_dashboard_token("super"));
    }

    #[test]
    fn test_dashboard_auth_yaml_parsing() {
        let yaml = r#"
dashboard_auth:
  token: "my-admin-token"
  session_ttl_secs: 3600
"#;
        let cfg: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.dashboard_auth.token, "my-admin-token");
        assert_eq!(cfg.dashboard_auth.session_ttl_secs, 3600);
    }
}
