//! Caddy integration — auto-detect Caddy reverse proxy.
//!
//! Unlike the old Traefik integration (which created/deleted per-client route
//! config files), Caddy uses On-Demand TLS: it provisions certificates
//! automatically when the first request arrives for a new subdomain.
//!
//! Caddy calls our `/api/tls-check?domain=<fqdn>` endpoint to verify that a
//! subdomain belongs to a connected client before issuing a certificate.
//! This means zo-tunnel-server does NOT need to manage any Caddy config files.

use serde::{Deserialize, Serialize};

/// Caddy integration config.
/// Auto-detected: if Caddy is installed, integration is enabled automatically.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaddyConfig {
    /// Enable Caddy integration (auto-detected if not set)
    #[serde(default)]
    pub enabled: bool,
}

impl CaddyConfig {
    /// Auto-detect Caddy: check if the binary exists or /etc/caddy/ is present.
    pub fn auto_detect() -> Self {
        // Check if Caddy config directory exists
        if std::path::Path::new("/etc/caddy").is_dir() {
            return Self { enabled: true };
        }

        // Check if caddy binary is in PATH
        if let Ok(output) = std::process::Command::new("which")
            .arg("caddy")
            .output()
        {
            if output.status.success() {
                return Self { enabled: true };
            }
        }

        Self::default()
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_disabled() {
        let cfg = CaddyConfig::default();
        assert!(!cfg.enabled);
    }

    #[test]
    fn test_serde_roundtrip() {
        let cfg = CaddyConfig { enabled: true };
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        let parsed: CaddyConfig = serde_yaml::from_str(&yaml).unwrap();
        assert!(parsed.enabled);
    }
}
