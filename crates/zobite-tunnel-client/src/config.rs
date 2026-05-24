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
