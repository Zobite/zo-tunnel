//! Client registry — tracks connected tunnel clients.

use crate::server::YamuxHandle;
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Handle to a connected client's yamux session.
pub struct ClientEntry {
    pub client_id: String,
    pub handle: YamuxHandle,
    pub connected_at: Instant,
    pub metrics: ClientMetrics,
    pub supports_heartbeat: bool,
}

/// Per-client traffic metrics.
pub struct ClientMetrics {
    pub bytes_in: AtomicU64,
    pub bytes_out: AtomicU64,
    pub requests: AtomicU64,
    pub active_streams: AtomicU64,
}

impl ClientMetrics {
    pub fn new() -> Self {
        Self {
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            requests: AtomicU64::new(0),
            active_streams: AtomicU64::new(0),
        }
    }
}

/// Serializable client info for the dashboard API.
#[derive(serde::Serialize, Clone)]
pub struct ClientInfo {
    pub client_id: String,
    pub connected_at_secs: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub total_requests: u64,
    pub active_streams: u64,
}

/// Thread-safe client registry.
pub struct Registry {
    clients: DashMap<String, Arc<ClientEntry>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            clients: DashMap::new(),
        }
    }

    /// Register a new client. Returns error if client_id already exists or is reserved.
    pub fn register(
        &self,
        client_id: String,
        handle: YamuxHandle,
        supports_heartbeat: bool,
    ) -> anyhow::Result<Arc<ClientEntry>> {
        use dashmap::mapref::entry::Entry;

        // Reject reserved subdomains
        if crate::config::RESERVED_SUBDOMAINS.contains(&client_id.as_str()) {
            anyhow::bail!(
                "Client ID '{}' is reserved (conflicts with system subdomain)",
                client_id
            );
        }

        let entry = Arc::new(ClientEntry {
            client_id: client_id.clone(),
            handle,
            connected_at: Instant::now(),
            metrics: ClientMetrics::new(),
            supports_heartbeat,
        });

        match self.clients.entry(client_id.clone()) {
            Entry::Occupied(_) => {
                anyhow::bail!("Client '{}' already registered", client_id);
            }
            Entry::Vacant(vacant) => {
                vacant.insert(entry.clone());
                Ok(entry)
            }
        }
    }

    /// Unregister a client.
    pub fn unregister(&self, client_id: &str) {
        self.clients.remove(client_id);
    }

    /// Get a client entry by ID.
    pub fn get(&self, client_id: &str) -> Option<Arc<ClientEntry>> {
        self.clients.get(client_id).map(|e| e.value().clone())
    }

    /// List all connected clients.
    pub fn list(&self) -> Vec<ClientInfo> {
        self.clients
            .iter()
            .map(|entry| {
                let e = entry.value();
                ClientInfo {
                    client_id: e.client_id.clone(),
                    connected_at_secs: e.connected_at.elapsed().as_secs(),
                    bytes_in: e.metrics.bytes_in.load(Ordering::Relaxed),
                    bytes_out: e.metrics.bytes_out.load(Ordering::Relaxed),
                    total_requests: e.metrics.requests.load(Ordering::Relaxed),
                    active_streams: e.metrics.active_streams.load(Ordering::Relaxed),
                }
            })
            .collect()
    }

    /// Count connected clients.
    pub fn count(&self) -> usize {
        self.clients.len()
    }
}
