//! Server configuration — YAML file + CLI override.

use serde::{Deserialize, Serialize};
use std::path::Path;

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
    pub rate_limit: RateLimitConfig,

    /// TCP port range for dedicated per-client TCP forwarding
    #[serde(default)]
    pub tcp_ports: TcpPortConfig,

    #[serde(default = "default_log_level")]
    pub log_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RoutingMode {
    Path,
    Subdomain,
}

impl Default for RoutingMode {
    fn default() -> Self {
        Self::Path
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub tokens: Vec<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self { tokens: vec![] }
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
    zobite_tunnel_protocol::DEFAULT_CONTROL_PORT
}
fn default_public_port() -> u16 {
    zobite_tunnel_protocol::DEFAULT_PUBLIC_PORT
}
fn default_dashboard_port() -> u16 {
    zobite_tunnel_protocol::DEFAULT_DASHBOARD_PORT
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
}
