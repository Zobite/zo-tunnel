//! Core server — control channel, public HTTP proxy, dashboard, TCP tunnels.

use crate::config::{RoutingMode, ServerConfig};
use crate::dashboard::{self, DashboardState};
use crate::metrics::{Metrics, RateLimiter};
use crate::proxy;
use crate::registry::Registry;
use anyhow::{Context, Result};
use dashmap::DashSet;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use std::future::poll_fn;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
use zo_tunnel_protocol::*;

/// Command sent to the yamux driver task.
pub enum YamuxCmd {
    OpenStream {
        reply: oneshot::Sender<anyhow::Result<yamux::Stream>>,
    },
}

/// Handle to interact with the yamux driver — send commands to open streams.
#[derive(Clone)]
pub struct YamuxHandle {
    cmd_tx: mpsc::Sender<YamuxCmd>,
}

impl YamuxHandle {
    pub async fn open_stream(&self) -> anyhow::Result<yamux::Stream> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(YamuxCmd::OpenStream { reply: reply_tx })
            .await
            .map_err(|_| anyhow::anyhow!("yamux driver gone"))?;
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("yamux driver dropped"))?
    }
}

/// Spawns a task that drives the yamux connection.
/// Returns a handle for opening outbound streams.
fn spawn_yamux_driver(
    stream: tokio::net::TcpStream,
    mode: yamux::Mode,
    client_id: String,
) -> (YamuxHandle, tokio::task::JoinHandle<()>) {
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<YamuxCmd>(64);

    let handle = YamuxHandle { cmd_tx };

    let task = tokio::spawn(async move {
        let compat = stream.compat();
        let cfg = yamux::Config::default();
        let mut conn = yamux::Connection::new(compat, cfg, mode);

        loop {
            tokio::select! {
                // Drive the yamux connection (accept inbound streams, process keep-alive)
                result = poll_fn(|cx| conn.poll_next_inbound(cx)) => {
                    match result {
                        Some(Ok(_stream)) => {
                            tracing::debug!("Unexpected inbound stream from '{}'", client_id);
                        }
                        Some(Err(e)) => {
                            tracing::debug!("Yamux error for '{}': {}", client_id, e);
                            break;
                        }
                        None => {
                            tracing::info!("Yamux connection closed for '{}'", client_id);
                            break;
                        }
                    }
                }
                // Handle commands to open outbound streams
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(YamuxCmd::OpenStream { reply }) => {
                            let result = poll_fn(|cx| conn.poll_new_outbound(cx)).await;
                            let _ = reply.send(
                                result.map_err(|e| anyhow::anyhow!("yamux open: {}", e))
                            );
                        }
                        None => {
                            // All handles dropped
                            tracing::debug!("All yamux handles dropped for '{}'", client_id);
                            break;
                        }
                    }
                }
            }
        }
    });

    (handle, task)
}

/// Manages TCP port allocation from a configured range.
pub struct TcpPortAllocator {
    used: DashSet<u16>,
    port_start: u16,
    port_end: u16,
}

impl TcpPortAllocator {
    pub fn new(port_start: u16, port_end: u16) -> Self {
        Self {
            used: DashSet::new(),
            port_start,
            port_end,
        }
    }


    /// Allocate a free port from the range. Returns None if all ports are taken.
    pub fn allocate(&self) -> Option<u16> {
        (self.port_start..=self.port_end).find(|port| self.used.insert(*port))
    }

    /// Release a previously allocated port.
    pub fn release(&self, port: u16) {
        self.used.remove(&port);
    }

    /// Total capacity.
    pub fn capacity(&self) -> usize {
        (self.port_end - self.port_start + 1) as usize
    }
}

/// Core Zo Tunnel server.
pub struct Server {
    config: ServerConfig,
}

impl Server {
    pub fn new(config: ServerConfig) -> Self {
        Self { config }
    }

    pub async fn run(&self) -> Result<()> {
        let registry = Arc::new(Registry::new());
        let metrics = Arc::new(Metrics::new());
        let rate_limiter = Arc::new(RateLimiter::new(self.config.rate_limit.requests_per_second));
        let tcp_allocator = Arc::new(TcpPortAllocator::new(
            self.config.tcp_ports.port_start,
            self.config.tcp_ports.port_end,
        ));

        // ── Bind control port ──
        let control_listener = TcpListener::bind(("0.0.0.0", self.config.control_port))
            .await
            .with_context(|| format!("bind control port {}", self.config.control_port))?;
        tracing::info!("🔌 Control channel on :{}", self.config.control_port);

        // ── Bind public port ──
        let public_listener = TcpListener::bind(("0.0.0.0", self.config.public_port))
            .await
            .with_context(|| format!("bind public port {}", self.config.public_port))?;
        tracing::info!("🌐 Public HTTP on :{}", self.config.public_port);

        // ── Bind dashboard port ──
        let dashboard_listener = TcpListener::bind(("0.0.0.0", self.config.dashboard_port))
            .await
            .with_context(|| format!("bind dashboard port {}", self.config.dashboard_port))?;
        tracing::info!("📊 Dashboard on :{}", self.config.dashboard_port);

        if self.config.tcp_ports.enabled {
            tracing::info!(
                "🔌 TCP port range: {}-{} ({} ports)",
                self.config.tcp_ports.port_start,
                self.config.tcp_ports.port_end,
                tcp_allocator.capacity()
            );
        }

        // ── TLS setup (optional) ──
        let tls_acceptor = if self.config.tls.enabled {
            Some(self.setup_tls()?)
        } else {
            None
        };

        // ── Spawn control channel acceptor ──
        let reg_ctrl = registry.clone();
        let met_ctrl = metrics.clone();
        let alloc_ctrl = tcp_allocator.clone();
        let config_ctrl = self.config.clone();
        let control_task = tokio::spawn(async move {
            Self::accept_clients(control_listener, reg_ctrl, met_ctrl, alloc_ctrl, config_ctrl).await;
        });

        // ── Spawn public HTTP proxy ──
        let reg_pub = registry.clone();
        let met_pub = metrics.clone();
        let rl_pub = rate_limiter.clone();
        let routing = self.config.routing_mode.clone();
        let domain = self.config.domain.clone();
        let tls_pub = tls_acceptor.clone();
        let public_task = tokio::spawn(async move {
            Self::accept_public(public_listener, reg_pub, met_pub, rl_pub, routing, domain, tls_pub)
                .await;
        });

        // ── Spawn dashboard ──
        let dash_state = DashboardState {
            registry: registry.clone(),
            metrics: metrics.clone(),
        };
        let dashboard_task = tokio::spawn(async move {
            let app = dashboard::create_router(dash_state);
            if let Err(e) = axum::serve(dashboard_listener, app).await {
                tracing::error!("Dashboard error: {}", e);
            }
        });

        tracing::info!("✅ Zo Tunnel Server ready!");

        // ── Wait for shutdown ──
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("🛑 Shutdown signal received");
            }
            _ = control_task => {
                tracing::error!("Control task ended unexpectedly");
            }
            _ = public_task => {
                tracing::error!("Public task ended unexpectedly");
            }
            _ = dashboard_task => {
                tracing::error!("Dashboard task ended unexpectedly");
            }
        }

        Ok(())
    }

    /// Setup TLS acceptor from cert and key files.
    fn setup_tls(&self) -> Result<tokio_rustls::TlsAcceptor> {
        use std::io::BufReader;
        use tokio_rustls::rustls;

        let cert_file = std::fs::File::open(&self.config.tls.cert)
            .with_context(|| format!("open TLS cert: {}", self.config.tls.cert))?;
        let key_file = std::fs::File::open(&self.config.tls.key)
            .with_context(|| format!("open TLS key: {}", self.config.tls.key))?;

        let certs: Vec<_> = rustls_pemfile::certs(&mut BufReader::new(cert_file))
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("parse TLS certs")?;

        let key = rustls_pemfile::private_key(&mut BufReader::new(key_file))
            .context("parse TLS key")?
            .context("no private key found")?;

        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .context("build TLS config")?;

        Ok(tokio_rustls::TlsAcceptor::from(Arc::new(config)))
    }

    /// Accept and handle control channel connections from tunnel clients.
    async fn accept_clients(
        listener: TcpListener,
        registry: Arc<Registry>,
        metrics: Arc<Metrics>,
        tcp_allocator: Arc<TcpPortAllocator>,
        config: ServerConfig,
    ) {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    tracing::info!("📡 Client connecting from {}", addr);
                    metrics.total_connections.fetch_add(1, Ordering::Relaxed);

                    let reg = registry.clone();
                    let met = metrics.clone();
                    let alloc = tcp_allocator.clone();
                    let cfg = config.clone();

                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_client(stream, reg, met, alloc, cfg).await {
                            tracing::warn!("Client {} error: {:#}", addr, e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("Control accept error: {}", e);
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// Handle a single client session: auth → yamux → serve streams.
    async fn handle_client(
        mut stream: tokio::net::TcpStream,
        registry: Arc<Registry>,
        metrics: Arc<Metrics>,
        tcp_allocator: Arc<TcpPortAllocator>,
        config: ServerConfig,
    ) -> Result<()> {
        // ── Auth handshake (before yamux) ──
        let auth_msg = read_message(&mut stream)
            .await
            .context("read auth message")?;

        let (client_id, tcp_mode) = match auth_msg {
            Message::AuthReq(auth) => {
                tracing::info!("🔑 Auth from '{}' (tcp_mode={})", auth.client_id, auth.tcp_mode);

                if !config.validate_token(&auth.token) {
                    let res = Message::AuthRes(AuthRes {
                        success: false,
                        message: "Invalid token".into(),
                        public_port: None,
                        assigned_route: None,
                        tcp_port: None,
                    });
                    write_message(&mut stream, &res).await?;
                    metrics.failed_auth.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!("❌ Auth failed for '{}'", auth.client_id);
                    return Ok(());
                }

                // ── Allocate TCP port if requested ──
                let assigned_tcp_port = if auth.tcp_mode && config.tcp_ports.enabled {
                    match tcp_allocator.allocate() {
                        Some(port) => {
                            tracing::info!("🔌 Allocated TCP port {} for '{}'", port, auth.client_id);
                            Some(port)
                        }
                        None => {
                            let res = Message::AuthRes(AuthRes {
                                success: false,
                                message: "No TCP ports available".into(),
                                public_port: None,
                                assigned_route: None,
                                tcp_port: None,
                            });
                            write_message(&mut stream, &res).await?;
                            tracing::warn!("❌ No TCP ports for '{}'", auth.client_id);
                            return Ok(());
                        }
                    }
                } else {
                    None
                };

                let msg = if let Some(tcp_port) = assigned_tcp_port {
                    format!("OK — TCP port {}", tcp_port)
                } else {
                    format!("OK — HTTP route /{}", auth.client_id)
                };

                let res = Message::AuthRes(AuthRes {
                    success: true,
                    message: msg,
                    public_port: Some(config.public_port),
                    assigned_route: Some(auth.client_id.clone()),
                    tcp_port: assigned_tcp_port,
                });
                write_message(&mut stream, &res).await?;

                if let Some(tcp_port) = assigned_tcp_port {
                    tracing::info!(
                        "✅ '{}' authenticated → TCP port: {}",
                        auth.client_id,
                        tcp_port
                    );
                } else {
                    tracing::info!(
                        "✅ '{}' authenticated → route: /{}",
                        auth.client_id,
                        auth.client_id
                    );
                }

                (auth.client_id, assigned_tcp_port)
            }
            other => {
                tracing::warn!("Expected AuthReq, got {:?}", other);
                return Ok(());
            }
        };

        // ── Spawn yamux driver ──
        let (yamux_handle, driver_task) =
            spawn_yamux_driver(stream, yamux::Mode::Server, client_id.clone());

        // Register client
        let _entry = match registry.register(client_id.clone(), yamux_handle.clone(), tcp_mode) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Registration failed for '{}': {}", client_id, e);
                driver_task.abort();
                if let Some(port) = tcp_mode {
                    tcp_allocator.release(port);
                }
                return Ok(());
            }
        };

        tracing::info!(
            "🟢 Client '{}' registered (total: {})",
            client_id,
            registry.count()
        );

        // ── Spawn TCP listener if in TCP mode ──
        let tcp_listener_task = if let Some(tcp_port) = tcp_mode {
            let handle = yamux_handle.clone();
            let cid = client_id.clone();
            let met = metrics.clone();
            Some(tokio::spawn(async move {
                Self::run_tcp_tunnel(tcp_port, handle, cid, met).await;
            }))
        } else {
            None
        };

        // Wait for the yamux driver to finish (= client disconnect)
        let _ = driver_task.await;

        // Stop TCP listener if active
        if let Some(task) = tcp_listener_task {
            task.abort();
        }

        // Release TCP port
        if let Some(port) = tcp_mode {
            tcp_allocator.release(port);
            tracing::info!("🔓 Released TCP port {} from '{}'", port, client_id);
        }

        // Client disconnected — unregister
        registry.unregister(&client_id);
        tracing::info!(
            "🔴 Client '{}' disconnected (remaining: {})",
            client_id,
            registry.count()
        );

        Ok(())
    }

    /// Run a dedicated TCP listener for a single client.
    /// Accepts raw TCP connections and pipes them through yamux streams.
    async fn run_tcp_tunnel(
        port: u16,
        yamux_handle: YamuxHandle,
        client_id: String,
        metrics: Arc<Metrics>,
    ) {
        let listener = match TcpListener::bind(("0.0.0.0", port)).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("Failed to bind TCP port {} for '{}': {}", port, client_id, e);
                return;
            }
        };

        tracing::info!("🔌 TCP tunnel for '{}' listening on :{}", client_id, port);

        loop {
            let (tcp_stream, peer_addr) = match listener.accept().await {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!("TCP accept error on port {}: {}", port, e);
                    continue;
                }
            };

            tracing::info!(
                "🔗 TCP:{} ← {} → '{}'",
                port,
                peer_addr,
                client_id
            );
            metrics.total_requests.fetch_add(1, Ordering::Relaxed);

            let handle = yamux_handle.clone();
            let cid = client_id.clone();

            tokio::spawn(async move {
                if let Err(e) = Self::handle_tcp_connection(tcp_stream, handle, &cid).await {
                    tracing::debug!("TCP tunnel stream error for '{}': {}", cid, e);
                }
            });
        }
    }

    /// Handle a single TCP connection: open yamux stream → bidirectional pipe.
    async fn handle_tcp_connection(
        mut tcp_stream: tokio::net::TcpStream,
        yamux_handle: YamuxHandle,
        client_id: &str,
    ) -> Result<()> {
        // Open yamux stream to client
        let yamux_stream = yamux_handle
            .open_stream()
            .await
            .with_context(|| format!("open yamux stream to '{}'", client_id))?;

        // Convert yamux stream (futures IO) → tokio IO
        let mut compat_stream = yamux_stream.compat();

        // Bidirectional pipe: public TCP ↔ yamux ↔ client ↔ local service
        match tokio::io::copy_bidirectional(&mut tcp_stream, &mut compat_stream).await {
            Ok((up, down)) => {
                tracing::debug!("TCP stream for '{}' done: ↑{}B ↓{}B", client_id, up, down);
            }
            Err(e) => {
                tracing::debug!("TCP stream pipe error for '{}': {}", client_id, e);
            }
        }

        Ok(())
    }

    /// Accept public HTTP connections and proxy them through tunnels.
    async fn accept_public(
        listener: TcpListener,
        registry: Arc<Registry>,
        metrics: Arc<Metrics>,
        rate_limiter: Arc<RateLimiter>,
        routing_mode: RoutingMode,
        domain: Option<String>,
        tls_acceptor: Option<tokio_rustls::TlsAcceptor>,
    ) {
        loop {
            let (tcp_stream, addr) = match listener.accept().await {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Public accept error: {}", e);
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
            };

            let reg = registry.clone();
            let met = metrics.clone();
            let rl = rate_limiter.clone();
            let rm = routing_mode.clone();
            let dom = domain.clone();
            let tls = tls_acceptor.clone();

            tokio::spawn(async move {
                let result = if let Some(acceptor) = tls {
                    match acceptor.accept(tcp_stream).await {
                        Ok(tls_stream) => {
                            Self::serve_http(TokioIo::new(tls_stream), reg, met, rl, rm, dom).await
                        }
                        Err(e) => {
                            tracing::debug!("TLS accept error from {}: {}", addr, e);
                            return;
                        }
                    }
                } else {
                    Self::serve_http(TokioIo::new(tcp_stream), reg, met, rl, rm, dom).await
                };

                if let Err(e) = result {
                    tracing::debug!("HTTP error from {}: {}", addr, e);
                }
            });
        }
    }

    /// Serve a single HTTP connection.
    async fn serve_http<I>(
        io: I,
        registry: Arc<Registry>,
        metrics: Arc<Metrics>,
        rate_limiter: Arc<RateLimiter>,
        routing_mode: RoutingMode,
        domain: Option<String>,
    ) -> Result<()>
    where
        I: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
    {
        http1::Builder::new()
            .serve_connection(
                io,
                service_fn(move |req| {
                    let reg = registry.clone();
                    let met = metrics.clone();
                    let rl = rate_limiter.clone();
                    let rm = routing_mode.clone();
                    let dom = domain.clone();
                    async move { proxy::handle_proxy_request(req, reg, met, rl, rm, dom).await }
                }),
            )
            .await
            .context("serve HTTP connection")?;
        Ok(())
    }
}
