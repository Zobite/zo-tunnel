//! Zobite Tunnel tunnel client — connects to server, accepts yamux streams, proxies to local service.

use anyhow::{bail, Context, Result};
use std::future::poll_fn;
use tokio::io;
use tokio::net::TcpStream;
use tokio_util::compat::{FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
use zobite_tunnel_protocol::*;

pub struct Client {
    server_addr: String,
    local_addr: String,
    client_id: String,
    token: String,
    tcp_mode: bool,
}

impl Client {
    pub fn new(server_addr: String, local_addr: String, client_id: String, token: String, tcp_mode: bool) -> Self {
        Self {
            server_addr,
            local_addr,
            client_id,
            token,
            tcp_mode,
        }
    }

    /// Run a single session: connect → auth → yamux → proxy streams.
    pub async fn run(&self) -> Result<()> {
        // ── Connect ──
        let mut stream = TcpStream::connect(&self.server_addr)
            .await
            .with_context(|| format!("connect to {}", self.server_addr))?;
        tracing::info!("Connected to server {}", self.server_addr);

        // ── Authenticate (raw protocol, before yamux) ──
        let auth_req = Message::AuthReq(AuthReq {
            client_id: self.client_id.clone(),
            token: self.token.clone(),
            tcp_mode: self.tcp_mode,
        });
        write_message(&mut stream, &auth_req).await?;
        tracing::debug!("Sent AuthReq");

        let auth_res = read_message(&mut stream)
            .await
            .context("read auth response")?;
        match auth_res {
            Message::AuthRes(res) => {
                if !res.success {
                    bail!("Authentication failed: {}", res.message);
                }
                if let Some(tcp_port) = res.tcp_port {
                    tracing::info!(
                        "✅ Authenticated! Dedicated TCP port: {}",
                        tcp_port
                    );
                } else {
                    tracing::info!(
                        "✅ Authenticated! HTTP port: {} | Route: {}",
                        res.public_port.unwrap_or(0),
                        res.assigned_route.as_deref().unwrap_or("-")
                    );
                }
            }
            other => {
                bail!("Expected AuthRes, got {:?}", other);
            }
        }

        // ── Create yamux session ──
        // yamux requires futures::AsyncRead/AsyncWrite, convert via compat
        let compat_stream = stream.compat();
        let yamux_config = yamux::Config::default();
        let mut conn = yamux::Connection::new(compat_stream, yamux_config, yamux::Mode::Client);

        tracing::info!("🚇 Tunnel active — waiting for connections...");

        // Drive the yamux connection, accepting incoming streams from the server.
        // Each stream = one public HTTP request being proxied.
        loop {
            let maybe_stream = poll_fn(|cx| conn.poll_next_inbound(cx)).await;
            match maybe_stream {
                Some(Ok(yamux_stream)) => {
                    let local_addr = self.local_addr.clone();

                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_tunnel_stream(yamux_stream, &local_addr).await
                        {
                            tracing::debug!("Stream error: {:#}", e);
                        }
                    });
                }
                Some(Err(e)) => {
                    tracing::error!("Yamux connection error: {}", e);
                    break;
                }
                None => {
                    tracing::info!("Yamux connection closed");
                    break;
                }
            }
        }

        tracing::info!("Tunnel session ended");
        Ok(())
    }

    /// Handle a single tunnel stream: pipe yamux ↔ local service.
    async fn handle_tunnel_stream(
        yamux_stream: yamux::Stream,
        local_addr: &str,
    ) -> Result<()> {
        // Convert yamux futures-io stream to tokio-io compatible
        let mut compat_stream = yamux_stream.compat();

        // Connect to local service
        let mut local_stream = TcpStream::connect(local_addr)
            .await
            .with_context(|| format!("connect to local {}", local_addr))?;

        tracing::debug!("🔗 Proxying stream → {}", local_addr);

        // Bidirectional pipe
        match io::copy_bidirectional(&mut compat_stream, &mut local_stream).await {
            Ok((up, down)) => {
                tracing::debug!("Stream done: ↑{}B ↓{}B", up, down);
            }
            Err(e) => {
                tracing::debug!("Stream pipe error: {}", e);
            }
        }

        Ok(())
    }
}
