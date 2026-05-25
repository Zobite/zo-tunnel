//! Server configuration — YAML file + CLI override.

use serde::{Deserialize, Serialize};
use std::path::Path;
use subtle::ConstantTimeEq;

/// Reserved subdomains that cannot be used as client IDs.
pub const RESERVED_SUBDOMAINS: &[&str] = &["dashboard", "connect", "api", "www", "admin"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_control_port")]
    pub control_port: u16,

    /// Public HTTP port (subdomain routing + dashboard).
    #[serde(default = "default_public_port")]
    pub public_port: u16,

    /// Base domain for subdomain routing (e.g. "tunnel.zobite.com").
    /// Each client is accessible at <client_id>.<domain>.
    /// Dashboard is at dashboard.<domain>.
    pub domain: String,

    #[serde(default)]
    pub auth: AuthConfig,

    #[serde(default)]
    pub dashboard_auth: DashboardAuthConfig,

    #[serde(default)]
    pub rate_limit: RateLimitConfig,

    #[serde(default)]
    pub traefik: TraefikConfig,

    #[serde(default = "default_log_level")]
    pub log_level: String,
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

/// Traefik integration — auto-create/delete per-client route config files.
/// Auto-detected: if a Traefik dynamic config directory exists, integration
/// is enabled automatically. No manual configuration needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraefikConfig {
    /// Enable Traefik integration (auto-detected if not set)
    #[serde(default)]
    pub enabled: bool,
    /// Directory for Traefik dynamic config files
    #[serde(default = "default_traefik_dir")]
    pub config_dir: String,
    /// Traefik entrypoint name
    #[serde(default = "default_traefik_entrypoint")]
    pub entrypoint: String,
    /// Traefik certificate resolver name
    #[serde(default = "default_traefik_cert_resolver")]
    pub cert_resolver: String,
}

impl TraefikConfig {
    /// Common Traefik dynamic config directories to auto-detect.
    const TRAEFIK_DIRS: &[&str] = &[
        "/etc/traefik/conf.d",
        "/etc/traefik/dynamic",
    ];

    /// Auto-detect Traefik: if a known directory exists, enable integration.
    pub fn auto_detect() -> Self {
        for dir in Self::TRAEFIK_DIRS {
            if std::path::Path::new(dir).is_dir() {
                return Self {
                    enabled: true,
                    config_dir: dir.to_string(),
                    entrypoint: default_traefik_entrypoint(),
                    cert_resolver: default_traefik_cert_resolver(),
                };
            }
        }
        Self::default()
    }
}

impl Default for TraefikConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            config_dir: default_traefik_dir(),
            entrypoint: default_traefik_entrypoint(),
            cert_resolver: default_traefik_cert_resolver(),
        }
    }
}

fn default_traefik_dir() -> String {
    "/etc/traefik/conf.d".into()
}
fn default_traefik_entrypoint() -> String {
    "websecure".into()
}
fn default_traefik_cert_resolver() -> String {
    "letsencrypt".into()
}

fn default_control_port() -> u16 {
    zo_tunnel_protocol::DEFAULT_CONTROL_PORT
}
fn default_public_port() -> u16 {
    zo_tunnel_protocol::DEFAULT_PUBLIC_PORT
}
fn default_log_level() -> String {
    "info".into()
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
            domain: String::new(),
            auth: AuthConfig::default(),
            dashboard_auth: DashboardAuthConfig::default(),
            rate_limit: RateLimitConfig::default(),
            traefik: TraefikConfig::default(),
            log_level: default_log_level(),
        }
    }
}



impl ServerConfig {
    /// Config directory for saving (used by `start --domain`).
    /// - `/etc/zo-tunnel/` when running as root
    /// - `~/.config/zo-tunnel/` otherwise
    pub fn config_dir() -> std::path::PathBuf {
        if nix_is_root() {
            std::path::PathBuf::from("/etc/zo-tunnel")
        } else {
            dirs_fallback().join("zo-tunnel")
        }
    }

    /// Full path to the server config file (for saving).
    pub fn config_path() -> std::path::PathBuf {
        Self::config_dir().join("server.yaml")
    }

    /// Resolve the config path for loading.
    /// Checks /etc/zo-tunnel/server.yaml first (system-wide),
    /// then falls back to ~/.config/zo-tunnel/server.yaml.
    pub fn resolve_config_path() -> Option<std::path::PathBuf> {
        let system_path = std::path::PathBuf::from("/etc/zo-tunnel/server.yaml");
        if system_path.exists() {
            return Some(system_path);
        }
        let user_path = dirs_fallback().join("zo-tunnel").join("server.yaml");
        if user_path.exists() {
            return Some(user_path);
        }
        None
    }

    /// Load config from a specific YAML file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: ServerConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// Save config to the system config path.
    pub fn save(&self) -> anyhow::Result<std::path::PathBuf> {
        let dir = Self::config_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| anyhow::anyhow!("Failed to create config dir {}: {}", dir.display(), e))?;

        let path = Self::config_path();
        let yaml = serde_yaml::to_string(self)?;

        // Add a header comment
        let content = format!(
            "# Zo Tunnel Server Configuration\n\
             # Generated by: zo-tunnel-server start --domain\n\
             # Config path: {}\n\
             #\n\
             # To reconfigure, run: zo-tunnel-server start --domain YOUR_DOMAIN --force\n\
             \n{}", 
            path.display(), yaml
        );

        std::fs::write(&path, content)
            .map_err(|e| anyhow::anyhow!("Failed to write config to {}: {}", path.display(), e))?;

        Ok(path)
    }

    /// Generate a cryptographically secure random hex token.
    pub fn generate_token(len_bytes: usize) -> String {
        let mut buf = vec![0u8; len_bytes];
        getrandom::getrandom(&mut buf).expect("getrandom failed");
        buf.iter().map(|b| format!("{:02x}", b)).collect()
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

/// Check if running as root (uid 0).
fn nix_is_root() -> bool {
    unsafe { libc_geteuid() == 0 }
}

extern "C" {
    #[link_name = "geteuid"]
    fn libc_geteuid() -> u32;
}

/// Fallback config dir: ~/.config/
fn dirs_fallback() -> std::path::PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            std::path::PathBuf::from(home).join(".config")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = ServerConfig::default();
        assert_eq!(cfg.control_port, 6200);
        assert_eq!(cfg.public_port, 6210);
        assert!(cfg.domain.is_empty());
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
domain: "test.example.com"
auth:
  tokens:
    - "tok1"
    - "tok2"
"#;
        let cfg: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.control_port, 7777);
        assert_eq!(cfg.public_port, 8888);
        assert_eq!(cfg.domain, "test.example.com");
        assert_eq!(cfg.auth.tokens, vec!["tok1", "tok2"]);
    }

    #[test]
    fn test_yaml_defaults_for_missing_fields() {
        let yaml = r#"domain: "tunnel.example.com""#;
        let cfg: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.control_port, 6200);
        assert_eq!(cfg.public_port, 6210);
        assert_eq!(cfg.domain, "tunnel.example.com");
        assert_eq!(cfg.log_level, "info");
    }

    #[test]
    fn test_dashboard_auth_disabled_by_default() {
        let cfg = ServerConfig::default();
        assert!(!cfg.dashboard_auth_enabled());
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
        assert!(!cfg.validate_dashboard_token("super-secret-admin-extra"));
        assert!(!cfg.validate_dashboard_token("super"));
    }

    #[test]
    fn test_dashboard_auth_yaml_parsing() {
        let yaml = r#"
domain: "tunnel.example.com"
dashboard_auth:
  token: "my-admin-token"
  session_ttl_secs: 3600
"#;
        let cfg: ServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.dashboard_auth.token, "my-admin-token");
        assert_eq!(cfg.dashboard_auth.session_ttl_secs, 3600);
    }
}
