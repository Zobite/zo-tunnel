//! Zo Tunnel tunnel client — connects to server, accepts yamux streams, proxies to local service.

use crate::config::ClientTlsConfig;
use anyhow::{bail, Context, Result};
use std::future::poll_fn;
use std::sync::Arc;
use tokio::io;
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio_util::compat::{FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
use tokio_util::sync::CancellationToken;
use zo_tunnel_protocol::*;

/// Real-time tunnel status, shared with the web UI.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(tag = "state")]
pub enum TunnelStatus {
    #[serde(rename = "stopped")]
    #[default]
    Stopped,
    #[serde(rename = "connecting")]
    Connecting,
    #[serde(rename = "connected")]
    Connected {
        route: String,
        #[serde(skip)]
        since: std::time::Instant,
    },
    #[serde(rename = "error")]
    Error { message: String },
}

pub struct Client {
    server_addr: String,
    local_addr: String,
    client_id: String,
    token: String,
    tls_config: ClientTlsConfig,
}

impl Client {
    pub fn new(
        server_addr: String,
        local_addr: String,
        client_id: String,
        token: String,
        tls_config: ClientTlsConfig,
    ) -> Self {
        Self {
            server_addr,
            local_addr,
            client_id,
            token,
            tls_config,
        }
    }

    /// Run a single session with cancellation support and status reporting.
    /// Used by TunnelManager for hot-reload control.
    pub async fn run_cancellable(
        &self,
        cancel: CancellationToken,
        status_tx: watch::Sender<TunnelStatus>,
    ) -> Result<()> {
        let _ = status_tx.send(TunnelStatus::Connecting);

        tokio::select! {
            result = self.run_with_status(&status_tx) => {
                if let Err(ref e) = result {
                    let _ = status_tx.send(TunnelStatus::Error {
                        message: format!("{:#}", e),
                    });
                }
                result
            }
            _ = cancel.cancelled() => {
                tracing::info!("Tunnel '{}' cancelled", self.client_id);
                let _ = status_tx.send(TunnelStatus::Stopped);
                Ok(())
            }
        }
    }

    /// Run with status reporting (internal).
    async fn run_with_status(&self, status_tx: &watch::Sender<TunnelStatus>) -> Result<()> {
        // ── Connect TCP ──
        let stream = TcpStream::connect(&self.server_addr)
            .await
            .with_context(|| format!("connect to {}", self.server_addr))?;

        // Set TCP keepalive for early dead-connection detection
        if let Err(e) = Self::configure_tcp_keepalive(&stream) {
            tracing::warn!("⚠️  TCP keepalive failed: {}", e);
        }

        tracing::info!("Connected to server {}", self.server_addr);

        if self.tls_config.enabled {
            let tls_stream = self.tls_connect(stream).await?;
            tracing::info!("🔒 TLS handshake complete");
            self.run_session_with_status(tls_stream, status_tx).await
        } else {
            self.run_session_with_status(stream, status_tx).await
        }
    }

    /// Establish a TLS connection over an existing TCP stream.
    async fn tls_connect(
        &self,
        stream: TcpStream,
    ) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
        use tokio_rustls::rustls;

        let config = if self.tls_config.skip_verify {
            // DANGEROUS: Accept any certificate (for self-signed certs in dev)
            let crypto_provider = rustls::crypto::ring::default_provider();
            rustls::ClientConfig::builder_with_provider(Arc::new(crypto_provider))
                .with_safe_default_protocol_versions()
                .context("build TLS protocol versions")?
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoVerifier))
                .with_no_client_auth()
        } else {
            // Production: verify against Mozilla root CAs
            let root_store =
                rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let crypto_provider = rustls::crypto::ring::default_provider();
            rustls::ClientConfig::builder_with_provider(Arc::new(crypto_provider))
                .with_safe_default_protocol_versions()
                .context("build TLS protocol versions")?
                .with_root_certificates(root_store)
                .with_no_client_auth()
        };

        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));

        // Resolve server name for SNI
        let server_name = self.resolve_server_name()?;
        tracing::debug!("TLS SNI: {:?}", server_name);

        connector
            .connect(server_name, stream)
            .await
            .context("TLS handshake failed")
    }

    /// Resolve the server name for TLS SNI.
    /// Priority: --tls-server-name > hostname from --server
    fn resolve_server_name(&self) -> Result<tokio_rustls::rustls::pki_types::ServerName<'static>> {
        use tokio_rustls::rustls::pki_types::ServerName;

        let name = if !self.tls_config.server_name.is_empty() {
            self.tls_config.server_name.clone()
        } else {
            // Extract hostname from server address (strip port)
            self.server_addr
                .split(':')
                .next()
                .unwrap_or(&self.server_addr)
                .to_string()
        };

        ServerName::try_from(name.clone())
            .map_err(|_| anyhow::anyhow!(
                "Invalid TLS server name '{}'. If connecting by IP, use --tls-server-name to specify the domain.",
                name
            ))
    }

    /// Run authenticated tunnel session with status reporting.
    async fn run_session_with_status<S>(
        &self,
        mut stream: S,
        status_tx: &watch::Sender<TunnelStatus>,
    ) -> Result<()>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        // ── Authenticate (before yamux) ──
        let auth_req = Message::AuthReq(AuthReq {
            client_id: self.client_id.clone(),
            token: self.token.clone(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        });
        write_message(&mut stream, &auth_req).await?;

        let auth_res = read_message(&mut stream)
            .await
            .context("read auth response")?;
        match auth_res {
            Message::AuthRes(res) => {
                if !res.success {
                    bail!("Authentication failed: {}", res.message);
                }

                let tunnel_url = res
                    .message
                    .strip_prefix("OK — ")
                    .unwrap_or(res.assigned_route.as_deref().unwrap_or("-"));

                let route = format!("http://{}", tunnel_url);
                let _ = status_tx.send(TunnelStatus::Connected {
                    route: route.clone(),
                    since: std::time::Instant::now(),
                });

                tracing::info!("✅ Authenticated! Route: {}", route);
            }
            other => {
                bail!("Expected AuthRes, got {:?}", other);
            }
        }

        // ── Create yamux session ──
        let compat_stream = stream.compat();
        let yamux_config = yamux::Config::default();
        let mut conn = yamux::Connection::new(compat_stream, yamux_config, yamux::Mode::Client);

        tracing::info!(
            "🚇 Tunnel '{}' active — waiting for connections...",
            self.client_id
        );

        loop {
            let maybe_stream = poll_fn(|cx| conn.poll_next_inbound(cx)).await;
            match maybe_stream {
                Some(Ok(yamux_stream)) => {
                    let local_addr = self.local_addr.clone();
                    tokio::spawn(async move {
                        let mut yamux_stream = yamux_stream;
                        // Read stream type marker with timeout to avoid blocking
                        let mut marker = [0u8; 1];
                        let marker_result =
                            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                                use futures::io::AsyncReadExt;
                                yamux_stream.read_exact(&mut marker).await
                            })
                            .await;

                        match marker_result {
                            Ok(Ok(_)) if marker[0] == STREAM_TYPE_HEARTBEAT => {
                                // Heartbeat ping from server — reply with pong
                                use futures::io::AsyncWriteExt;
                                let _ = yamux_stream.write_all(&[STREAM_TYPE_HEARTBEAT]).await;
                                let _ = yamux_stream.close().await;
                                tracing::trace!("💓 Heartbeat pong sent");
                            }
                            Ok(Ok(_)) => {
                                // STREAM_TYPE_PROXY or any other — forward to local service
                                if let Err(e) =
                                    Self::handle_tunnel_stream(yamux_stream, &local_addr).await
                                {
                                    tracing::debug!("Stream error: {:#}", e);
                                }
                            }
                            Ok(Err(e)) => {
                                tracing::debug!("Stream marker read error: {:#}", e);
                            }
                            Err(_) => {
                                tracing::debug!("Stream marker read timed out");
                            }
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

        tracing::info!("Tunnel '{}' session ended", self.client_id);
        Ok(())
    }

    /// Handle a single tunnel stream: pipe yamux ↔ local service.
    async fn handle_tunnel_stream(yamux_stream: yamux::Stream, local_addr: &str) -> Result<()> {
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

    /// Configure TCP keepalive on the server socket.
    fn configure_tcp_keepalive(stream: &TcpStream) -> Result<()> {
        use socket2::SockRef;
        use std::time::Duration;

        let sock = SockRef::from(stream);
        let keepalive = socket2::TcpKeepalive::new()
            .with_time(Duration::from_secs(HEARTBEAT_INTERVAL_SECS))
            .with_interval(Duration::from_secs(5));

        #[cfg(target_os = "linux")]
        let keepalive = keepalive.with_retries(3);

        sock.set_tcp_keepalive(&keepalive)
            .context("set TCP keepalive")?;
        Ok(())
    }
}

// ─── No-verify TLS (for self-signed certs) ──────────────────────

/// A TLS certificate verifier that accepts any certificate.
/// ONLY for development with self-signed certificates.
#[derive(Debug)]
struct NoVerifier;

impl tokio_rustls::rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[tokio_rustls::rustls::pki_types::CertificateDer<'_>],
        _server_name: &tokio_rustls::rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: tokio_rustls::rustls::pki_types::UnixTime,
    ) -> Result<tokio_rustls::rustls::client::danger::ServerCertVerified, tokio_rustls::rustls::Error>
    {
        Ok(tokio_rustls::rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _dss: &tokio_rustls::rustls::DigitallySignedStruct,
    ) -> Result<
        tokio_rustls::rustls::client::danger::HandshakeSignatureValid,
        tokio_rustls::rustls::Error,
    > {
        Ok(tokio_rustls::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _dss: &tokio_rustls::rustls::DigitallySignedStruct,
    ) -> Result<
        tokio_rustls::rustls::client::danger::HandshakeSignatureValid,
        tokio_rustls::rustls::Error,
    > {
        Ok(tokio_rustls::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<tokio_rustls::rustls::SignatureScheme> {
        tokio_rustls::rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
