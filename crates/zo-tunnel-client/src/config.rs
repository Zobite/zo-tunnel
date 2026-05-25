//! Client configuration — YAML file support + saved credentials.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub server: String,
    pub client_id: String,
    pub local_addr: String,
    pub token: String,
    #[serde(default)]
    pub reconnect: ReconnectConfig,
    #[serde(default)]
    pub tls: ClientTlsConfig,
}

/// TLS configuration for the control channel connection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientTlsConfig {
    /// Enable TLS for the control channel
    #[serde(default)]
    pub enabled: bool,
    /// Server name for TLS SNI and certificate verification.
    /// Default: extracted from the server address hostname.
    #[serde(default)]
    pub server_name: String,
    /// Skip TLS certificate verification (DANGEROUS — only for self-signed certs in dev)
    #[serde(default)]
    pub skip_verify: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconnectConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_interval")]
    pub max_interval: u64,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_interval: 30,
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_max_interval() -> u64 {
    30
}

impl ClientConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: ClientConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }
}

// ─── Saved Credentials (persistent auth) ─────────────────────────

/// Credentials saved after `zo-tunnel-client login`.
/// Stored at `~/.zo-tunnel/credentials.yaml` with mode 0600.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedCredentials {
    pub server: String,
    pub token: String,
    #[serde(default)]
    pub tls: ClientTlsConfig,
}

/// Return the credentials directory: `~/.zo-tunnel/`
pub fn credentials_dir() -> anyhow::Result<PathBuf> {
    let home = std::env::var("HOME")
        .map_err(|_| anyhow::anyhow!("HOME environment variable not set"))?;
    Ok(PathBuf::from(home).join(".zo-tunnel"))
}

/// Return the credentials file path: `~/.zo-tunnel/credentials.yaml`
pub fn credentials_path() -> anyhow::Result<PathBuf> {
    Ok(credentials_dir()?.join("credentials.yaml"))
}

impl SavedCredentials {
    /// Load saved credentials from `~/.zo-tunnel/credentials.yaml`.
    /// Returns `None` if the file does not exist.
    pub fn load() -> anyhow::Result<Option<Self>> {
        let path = credentials_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let creds: SavedCredentials = serde_yaml::from_str(&content)?;
        Ok(Some(creds))
    }

    /// Save credentials to `~/.zo-tunnel/credentials.yaml` with mode 0600.
    pub fn save(&self) -> anyhow::Result<()> {
        let dir = credentials_dir()?;
        std::fs::create_dir_all(&dir)?;

        // Set directory permissions to 0700 (owner only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }

        let path = credentials_path()?;
        let yaml = serde_yaml::to_string(self)?;
        std::fs::write(&path, yaml)?;

        // Set file permissions to 0600 (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    /// Delete the saved credentials file.
    pub fn delete() -> anyhow::Result<bool> {
        let path = credentials_path()?;
        if path.exists() {
            std::fs::remove_file(&path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Check if saved credentials exist.
    pub fn exists() -> bool {
        credentials_path()
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    /// Return a masked version of the token for display.
    pub fn masked_token(&self) -> String {
        if self.token.len() <= 4 {
            "****".to_string()
        } else {
            let visible = &self.token[..4];
            format!("{}****", visible)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_yaml_parsing() {
        let yaml = r#"
server: "vps:6200"
client_id: "my-app"
local_addr: "localhost:3000"
token: "secret123"
"#;
        let cfg: ClientConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.server, "vps:6200");
        assert_eq!(cfg.client_id, "my-app");
        assert_eq!(cfg.local_addr, "localhost:3000");
        assert_eq!(cfg.token, "secret123");
    }

    #[test]
    fn test_reconnect_defaults() {
        let rc = ReconnectConfig::default();
        assert!(rc.enabled);
        assert_eq!(rc.max_interval, 30);
    }

    #[test]
    fn test_client_config_custom_reconnect() {
        let yaml = r#"
server: "vps:6200"
client_id: "app"
local_addr: "localhost:8080"
token: "t"
reconnect:
  enabled: false
  max_interval: 60
"#;
        let cfg: ClientConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(!cfg.reconnect.enabled);
        assert_eq!(cfg.reconnect.max_interval, 60);
    }

    #[test]
    fn test_saved_credentials_yaml_parsing() {
        let yaml = r#"
server: "vps:6200"
token: "secret-token-123"
"#;
        let creds: SavedCredentials = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(creds.server, "vps:6200");
        assert_eq!(creds.token, "secret-token-123");
        assert!(!creds.tls.enabled); // default
    }

    #[test]
    fn test_saved_credentials_with_tls() {
        let yaml = r#"
server: "vps:6200"
token: "tok"
tls:
  enabled: true
  server_name: "example.com"
  skip_verify: false
"#;
        let creds: SavedCredentials = serde_yaml::from_str(yaml).unwrap();
        assert!(creds.tls.enabled);
        assert_eq!(creds.tls.server_name, "example.com");
        assert!(!creds.tls.skip_verify);
    }

    #[test]
    fn test_masked_token_long() {
        let creds = SavedCredentials {
            server: "s".into(),
            token: "abcdefgh".into(),
            tls: ClientTlsConfig::default(),
        };
        assert_eq!(creds.masked_token(), "abcd****");
    }

    #[test]
    fn test_masked_token_short() {
        let creds = SavedCredentials {
            server: "s".into(),
            token: "ab".into(),
            tls: ClientTlsConfig::default(),
        };
        assert_eq!(creds.masked_token(), "****");
    }

    #[test]
    fn test_saved_credentials_roundtrip() {
        let creds = SavedCredentials {
            server: "myserver:6200".into(),
            token: "my-secret-token".into(),
            tls: ClientTlsConfig {
                enabled: true,
                server_name: "example.com".into(),
                skip_verify: false,
            },
        };
        let yaml = serde_yaml::to_string(&creds).unwrap();
        let loaded: SavedCredentials = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(loaded.server, creds.server);
        assert_eq!(loaded.token, creds.token);
        assert_eq!(loaded.tls.enabled, creds.tls.enabled);
        assert_eq!(loaded.tls.server_name, creds.tls.server_name);
    }
}
