//! Client configuration — YAML file support + saved credentials.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[allow(dead_code)] // kept for config file compatibility
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

#[allow(dead_code)]
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
#[allow(dead_code)]
fn default_max_interval() -> u64 {
    30
}

#[allow(dead_code)]
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
    let home =
        std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME environment variable not set"))?;
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
        credentials_path().map(|p| p.exists()).unwrap_or(false)
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

// ─── Multi-Tunnel Configuration ──────────────────────────────────

/// A single tunnel configuration entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelEntry {
    /// Unique ID (UUID)
    pub id: String,
    /// Tunnel name — used as subdomain on the server
    pub client_id: String,
    /// Local service address to forward to (e.g. "localhost:3000")
    pub local_addr: String,
    /// Whether to auto-connect when running `serve`
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Multi-tunnel config stored at `~/.zo-tunnel/tunnels.yaml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TunnelsConfig {
    #[serde(default)]
    pub tunnels: Vec<TunnelEntry>,
}

/// Return the tunnels config path: `~/.zo-tunnel/tunnels.yaml`
pub fn tunnels_path() -> anyhow::Result<PathBuf> {
    Ok(credentials_dir()?.join("tunnels.yaml"))
}

impl TunnelsConfig {
    /// Load tunnels config from `~/.zo-tunnel/tunnels.yaml`.
    /// Returns empty config if the file does not exist.
    pub fn load() -> anyhow::Result<Self> {
        let path = tunnels_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        let config: TunnelsConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// Save tunnels config to `~/.zo-tunnel/tunnels.yaml` with mode 0600.
    pub fn save(&self) -> anyhow::Result<()> {
        let dir = credentials_dir()?;
        std::fs::create_dir_all(&dir)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }

        let path = tunnels_path()?;
        let yaml = serde_yaml::to_string(self)?;
        std::fs::write(&path, yaml)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    /// Generate a new unique tunnel ID.
    pub fn generate_id() -> String {
        let mut buf = [0u8; 8];
        getrandom::getrandom(&mut buf).expect("getrandom failed");
        buf.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// Add a tunnel entry. Generates an ID if empty.
    pub fn add_tunnel(&mut self, mut entry: TunnelEntry) -> anyhow::Result<TunnelEntry> {
        if entry.id.is_empty() {
            entry.id = Self::generate_id();
        }

        // Validate: client_id must not be empty
        if entry.client_id.trim().is_empty() {
            anyhow::bail!("client_id cannot be empty");
        }

        // Validate: local_addr must not be empty
        if entry.local_addr.trim().is_empty() {
            anyhow::bail!("local_addr cannot be empty");
        }

        // Validate: client_id must be unique
        if self.tunnels.iter().any(|t| t.client_id == entry.client_id) {
            anyhow::bail!("tunnel with client_id '{}' already exists", entry.client_id);
        }

        self.tunnels.push(entry.clone());
        self.save()?;
        Ok(entry)
    }

    /// Update a tunnel entry by ID.
    pub fn update_tunnel(
        &mut self,
        id: &str,
        client_id: String,
        local_addr: String,
        enabled: bool,
    ) -> anyhow::Result<TunnelEntry> {
        // Validate inputs
        if client_id.trim().is_empty() {
            anyhow::bail!("client_id cannot be empty");
        }
        if local_addr.trim().is_empty() {
            anyhow::bail!("local_addr cannot be empty");
        }

        // Check uniqueness: client_id must not conflict with another tunnel
        if self
            .tunnels
            .iter()
            .any(|t| t.client_id == client_id && t.id != id)
        {
            anyhow::bail!("tunnel with client_id '{}' already exists", client_id);
        }

        let entry = self
            .tunnels
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| anyhow::anyhow!("tunnel '{}' not found", id))?;

        entry.client_id = client_id;
        entry.local_addr = local_addr;
        entry.enabled = enabled;

        let updated = entry.clone();
        self.save()?;
        Ok(updated)
    }

    /// Remove a tunnel entry by ID.
    pub fn remove_tunnel(&mut self, id: &str) -> anyhow::Result<TunnelEntry> {
        let pos = self
            .tunnels
            .iter()
            .position(|t| t.id == id)
            .ok_or_else(|| anyhow::anyhow!("tunnel '{}' not found", id))?;
        let removed = self.tunnels.remove(pos);
        self.save()?;
        Ok(removed)
    }

    /// Get a tunnel by ID.
    pub fn get_tunnel(&self, id: &str) -> Option<&TunnelEntry> {
        self.tunnels.iter().find(|t| t.id == id)
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

    // ─── TunnelsConfig Tests ─────────────────────────────────────

    #[test]
    fn test_tunnel_entry_yaml_parsing() {
        let yaml = r#"
tunnels:
  - id: "abc123"
    client_id: "my-api"
    local_addr: "localhost:3000"
    enabled: true
  - id: "def456"
    client_id: "my-web"
    local_addr: "localhost:8080"
    enabled: false
"#;
        let cfg: TunnelsConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.tunnels.len(), 2);
        assert_eq!(cfg.tunnels[0].client_id, "my-api");
        assert_eq!(cfg.tunnels[0].local_addr, "localhost:3000");
        assert!(cfg.tunnels[0].enabled);
        assert_eq!(cfg.tunnels[1].client_id, "my-web");
        assert!(!cfg.tunnels[1].enabled);
    }

    #[test]
    fn test_tunnel_entry_defaults() {
        let yaml = r#"
tunnels:
  - id: "x"
    client_id: "app"
    local_addr: "localhost:3000"
"#;
        let cfg: TunnelsConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.tunnels[0].enabled); // default true
    }

    #[test]
    fn test_tunnels_config_add_tunnel() {
        let mut cfg = TunnelsConfig::default();
        let entry = TunnelEntry {
            id: String::new(),
            client_id: "my-api".into(),
            local_addr: "localhost:3000".into(),
            enabled: true,
        };
        let added = cfg.add_tunnel(entry).unwrap();
        assert!(!added.id.is_empty());
        assert_eq!(cfg.tunnels.len(), 1);
    }

    #[test]
    fn test_tunnels_config_add_duplicate_fails() {
        let mut cfg = TunnelsConfig::default();
        let entry1 = TunnelEntry {
            id: "1".into(),
            client_id: "my-api".into(),
            local_addr: "localhost:3000".into(),
            enabled: true,
        };
        cfg.tunnels.push(entry1);

        let entry2 = TunnelEntry {
            id: String::new(),
            client_id: "my-api".into(),
            local_addr: "localhost:8080".into(),
            enabled: true,
        };
        assert!(cfg.add_tunnel(entry2).is_err());
    }

    #[test]
    fn test_tunnels_config_add_empty_client_id_fails() {
        let mut cfg = TunnelsConfig::default();
        let entry = TunnelEntry {
            id: String::new(),
            client_id: "".into(),
            local_addr: "localhost:3000".into(),
            enabled: true,
        };
        assert!(cfg.add_tunnel(entry).is_err());
    }

    #[test]
    fn test_tunnels_config_remove_tunnel() {
        let mut cfg = TunnelsConfig {
            tunnels: vec![TunnelEntry {
                id: "abc".into(),
                client_id: "my-api".into(),
                local_addr: "localhost:3000".into(),
                enabled: true,
            }],
        };
        let removed = cfg.remove_tunnel("abc").unwrap();
        assert_eq!(removed.client_id, "my-api");
        assert!(cfg.tunnels.is_empty());
    }

    #[test]
    fn test_tunnels_config_remove_not_found() {
        let mut cfg = TunnelsConfig::default();
        assert!(cfg.remove_tunnel("nonexistent").is_err());
    }

    #[test]
    fn test_tunnels_config_generate_id() {
        let id = TunnelsConfig::generate_id();
        assert_eq!(id.len(), 16); // 8 bytes = 16 hex chars
    }
}
