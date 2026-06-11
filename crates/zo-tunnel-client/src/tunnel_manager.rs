//! Tunnel Manager — manages multiple tunnel connections concurrently.
//! Each tunnel runs in its own tokio task with reconnect loop.
//! Supports hot-reload: add/remove/restart tunnels without restarting the client.

use crate::client::{Client, TunnelStatus};
use crate::config::{ClientTlsConfig, TunnelEntry, TunnelsConfig};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpStream;
use tokio::sync::{watch, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Handle to a running tunnel — task + cancellation + status.
pub struct TunnelHandle {
    pub entry: TunnelEntry,
    pub cancel: CancellationToken,
    pub status_rx: watch::Receiver<TunnelStatus>,
    pub task: JoinHandle<()>,
    #[allow(dead_code)]
    pub started_at: Instant,
}

/// Info returned by the API for each tunnel.
#[derive(serde::Serialize, Clone)]
pub struct TunnelInfo {
    pub id: String,
    pub client_id: String,
    pub local_addr: String,
    pub enabled: bool,
    pub status: TunnelStatusInfo,
}

/// Serializable status for the API.
#[derive(serde::Serialize, Clone)]
pub struct TunnelStatusInfo {
    /// Server connection state: "connected", "connecting", "error", "stopped"
    pub state: String,
    pub route: Option<String>,
    pub connected_since_secs: Option<u64>,
    pub error: Option<String>,
    /// Whether the local service (e.g. localhost:3000) is reachable
    pub local_reachable: bool,
}

impl TunnelStatusInfo {
    fn from_status(status: &TunnelStatus, local_reachable: bool) -> Self {
        match status {
            TunnelStatus::Stopped => TunnelStatusInfo {
                state: "stopped".into(),
                route: None,
                connected_since_secs: None,
                error: None,
                local_reachable,
            },
            TunnelStatus::Connecting => TunnelStatusInfo {
                state: "connecting".into(),
                route: None,
                connected_since_secs: None,
                error: None,
                local_reachable,
            },
            TunnelStatus::Connected { route, since } => TunnelStatusInfo {
                state: "connected".into(),
                route: Some(route.clone()),
                connected_since_secs: Some(since.elapsed().as_secs()),
                error: None,
                local_reachable,
            },
            TunnelStatus::Error { message } => TunnelStatusInfo {
                state: "error".into(),
                route: None,
                connected_since_secs: None,
                error: Some(message.clone()),
                local_reachable,
            },
        }
    }
}

/// Check if a local address is reachable (TCP connect with 300ms timeout).
async fn check_local_reachable(addr: &str) -> bool {
    tokio::time::timeout(
        std::time::Duration::from_millis(300),
        TcpStream::connect(addr),
    )
    .await
    .map(|r| r.is_ok())
    .unwrap_or(false)
}

/// Manages multiple tunnel connections.
pub struct TunnelManager {
    server_addr: String,
    token: String,
    tls_config: ClientTlsConfig,
    /// Running tunnels: tunnel_id -> TunnelHandle
    tunnels: Arc<DashMap<String, TunnelHandle>>,
    /// Persistent config
    config: Arc<RwLock<TunnelsConfig>>,
}

impl TunnelManager {
    pub fn new(
        server_addr: String,
        token: String,
        tls_config: ClientTlsConfig,
        config: TunnelsConfig,
    ) -> Self {
        Self {
            server_addr,
            token,
            tls_config,
            tunnels: Arc::new(DashMap::new()),
            config: Arc::new(RwLock::new(config)),
        }
    }

    /// Start all enabled tunnels from config.
    pub async fn start_all(&self) {
        let config = self.config.read().await;
        let entries: Vec<_> = config
            .tunnels
            .iter()
            .filter(|t| t.enabled)
            .cloned()
            .collect();
        drop(config);

        for entry in entries {
            if let Err(e) = self.start_tunnel_internal(&entry) {
                tracing::error!("Failed to start tunnel '{}': {:#}", entry.client_id, e);
            }
        }
    }

    /// Start a single tunnel by entry.
    fn start_tunnel_internal(&self, entry: &TunnelEntry) -> anyhow::Result<()> {
        if self.tunnels.contains_key(&entry.id) {
            anyhow::bail!("Tunnel '{}' is already running", entry.client_id);
        }

        let cancel = CancellationToken::new();
        let (status_tx, status_rx) = watch::channel(TunnelStatus::Stopped);

        let client = Client::new(
            self.server_addr.clone(),
            entry.local_addr.clone(),
            entry.client_id.clone(),
            self.token.clone(),
            self.tls_config.clone(),
        );

        let cancel_clone = cancel.clone();
        let client_id = entry.client_id.clone();

        let task = tokio::spawn(async move {
            let mut backoff_secs = 1u64;
            let max_backoff = 30u64;

            loop {
                match client
                    .run_cancellable(cancel_clone.clone(), status_tx.clone())
                    .await
                {
                    Ok(_) => {
                        let _ = status_tx.send(TunnelStatus::Stopped);
                        backoff_secs = 1;
                    }
                    Err(e) => {
                        tracing::error!("Tunnel '{}' error: {:#}", client_id, e);
                        let _ = status_tx.send(TunnelStatus::Error {
                            message: format!("{:#}", e),
                        });
                    }
                }

                if cancel_clone.is_cancelled() {
                    let _ = status_tx.send(TunnelStatus::Stopped);
                    break;
                }

                tracing::info!(
                    "🔄 Tunnel '{}' reconnecting in {}s...",
                    client_id,
                    backoff_secs
                );
                let _ = status_tx.send(TunnelStatus::Connecting);

                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)) => {}
                    _ = cancel_clone.cancelled() => {
                        let _ = status_tx.send(TunnelStatus::Stopped);
                        break;
                    }
                }

                backoff_secs = (backoff_secs * 2).min(max_backoff);
            }
        });

        let handle = TunnelHandle {
            entry: entry.clone(),
            cancel,
            status_rx,
            task,
            started_at: Instant::now(),
        };

        self.tunnels.insert(entry.id.clone(), handle);
        tracing::info!(
            "▶ Started tunnel '{}' → {}",
            entry.client_id,
            entry.local_addr
        );

        Ok(())
    }

    /// Stop a running tunnel.
    pub fn stop_tunnel(&self, id: &str) -> anyhow::Result<()> {
        let handle = self
            .tunnels
            .remove(id)
            .map(|(_, h)| h)
            .ok_or_else(|| anyhow::anyhow!("Tunnel '{}' is not running", id))?;

        handle.cancel.cancel();
        handle.task.abort();
        tracing::info!("⏹ Stopped tunnel '{}'", handle.entry.client_id);
        Ok(())
    }

    /// Add a new tunnel: save to config + auto-start if enabled.
    pub async fn add_tunnel(&self, entry: TunnelEntry) -> anyhow::Result<TunnelEntry> {
        let mut config = self.config.write().await;
        let added = config.add_tunnel(entry)?;
        drop(config);

        if added.enabled {
            self.start_tunnel_internal(&added)?;
        }

        Ok(added)
    }

    /// Update a tunnel: stop old, update config, start new.
    pub async fn update_tunnel(
        &self,
        id: &str,
        client_id: String,
        local_addr: String,
        enabled: bool,
    ) -> anyhow::Result<TunnelEntry> {
        let _ = self.stop_tunnel(id);

        let mut config = self.config.write().await;
        let updated = config.update_tunnel(id, client_id, local_addr, enabled)?;
        drop(config);

        if updated.enabled {
            self.start_tunnel_internal(&updated)?;
        }

        Ok(updated)
    }

    /// Remove a tunnel: stop + remove from config.
    pub async fn remove_tunnel(&self, id: &str) -> anyhow::Result<TunnelEntry> {
        let _ = self.stop_tunnel(id);

        let mut config = self.config.write().await;
        config.remove_tunnel(id)
    }

    /// Start a specific tunnel (by ID).
    pub async fn start_tunnel(&self, id: &str) -> anyhow::Result<()> {
        let config = self.config.read().await;
        let entry = config
            .get_tunnel(id)
            .ok_or_else(|| anyhow::anyhow!("Tunnel '{}' not found in config", id))?
            .clone();
        drop(config);

        self.start_tunnel_internal(&entry)
    }

    /// Restart a specific tunnel.
    pub async fn restart_tunnel(&self, id: &str) -> anyhow::Result<()> {
        let _ = self.stop_tunnel(id);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        self.start_tunnel(id).await
    }

    /// List all tunnels with real-time status.
    /// Checks local service reachability on-demand (only when called by the web UI).
    pub async fn list_tunnels(&self) -> Vec<TunnelInfo> {
        let config = self.config.read().await;
        let entries: Vec<TunnelEntry> = config.tunnels.clone();
        drop(config);

        // Collect server status + local_addr for each entry (snapshot, no locks held)
        let snapshots: Vec<_> = entries
            .iter()
            .map(|entry| {
                let server_status = if let Some(handle) = self.tunnels.get(&entry.id) {
                    handle.status_rx.borrow().clone()
                } else {
                    TunnelStatus::Stopped
                };
                (entry.clone(), server_status)
            })
            .collect();

        // Check all local services concurrently (no locks held)
        let health_futures: Vec<_> = snapshots
            .iter()
            .map(|(entry, _)| check_local_reachable(&entry.local_addr))
            .collect();
        let health_results = futures::future::join_all(health_futures).await;

        // Build response
        snapshots
            .into_iter()
            .zip(health_results)
            .map(|((entry, server_status), local_ok)| TunnelInfo {
                id: entry.id,
                client_id: entry.client_id,
                local_addr: entry.local_addr,
                enabled: entry.enabled,
                status: TunnelStatusInfo::from_status(&server_status, local_ok),
            })
            .collect()
    }

    /// Stop all running tunnels (for graceful shutdown).
    pub fn stop_all(&self) {
        let ids: Vec<String> = self.tunnels.iter().map(|e| e.key().clone()).collect();
        for id in ids {
            let _ = self.stop_tunnel(&id);
        }
    }

    /// Get the server address.
    pub fn server_addr(&self) -> &str {
        &self.server_addr
    }

    /// Get the count of running tunnels.
    pub fn running_count(&self) -> usize {
        self.tunnels.len()
    }
}
