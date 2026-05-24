//! Client configuration — YAML file support.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub server: String,
    pub client_id: String,
    pub local_addr: String,
    pub token: String,
    /// Request dedicated TCP port instead of HTTP routing
    #[serde(default)]
    pub tcp_mode: bool,
    #[serde(default)]
    pub reconnect: ReconnectConfig,
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
        assert!(!cfg.tcp_mode); // default false
    }

    #[test]
    fn test_client_config_tcp_mode() {
        let yaml = r#"
server: "vps:6200"
client_id: "db"
local_addr: "localhost:5432"
token: "tok"
tcp_mode: true
"#;
        let cfg: ClientConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.tcp_mode);
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
}
