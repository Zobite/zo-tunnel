//! Zo Tunnel tunnel client — connects to server, accepts yamux streams, proxies to local service.

use crate::config::ClientTlsConfig;
use anyhow::{bail, Context, Result};
use std::future::poll_fn;
use std::sync::Arc;
use tokio::io;
use tokio::net::TcpStream;
use tokio_util::compat::{FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
use zo_tunnel_protocol::*;

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

    /// Run a single session: connect → (optional TLS) → auth → yamux → proxy streams.
    pub async fn run(&self) -> Result<()> {
        // ── Connect TCP ──
        let stream = TcpStream::connect(&self.server_addr)
            .await
            .with_context(|| format!("connect to {}", self.server_addr))?;
        tracing::info!("Connected to server {}", self.server_addr);

        if self.tls_config.enabled {
            // ── TLS mode ──
            let tls_stream = self.tls_connect(stream).await?;
            tracing::info!("🔒 TLS handshake complete");
            self.run_session(tls_stream).await
        } else {
            // ── Plain TCP mode ──
            self.run_session(stream).await
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
            let root_store = rustls::RootCertStore::from_iter(
                webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
            );
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

    /// Run the authenticated tunnel session over any stream type.
    async fn run_session<S>(&self, mut stream: S) -> Result<()>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        // ── Authenticate (before yamux) ──
        let auth_req = Message::AuthReq(AuthReq {
            client_id: self.client_id.clone(),
            token: self.token.clone(),
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

                // Extract domain from server message (format: "OK — <client_id>.<domain>")
                let tunnel_url = res
                    .message
                    .strip_prefix("OK — ")
                    .unwrap_or(res.assigned_route.as_deref().unwrap_or("-"));

                tracing::info!("✅ Authenticated!");
                tracing::info!("┌──────────────────────────────────────────┐");
                tracing::info!("│  🌐 Tunnel: http://{}  ", tunnel_url);
                tracing::info!("└──────────────────────────────────────────┘");
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
        // Each stream = one public request being proxied.
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
    ) -> Result<tokio_rustls::rustls::client::danger::HandshakeSignatureValid, tokio_rustls::rustls::Error>
    {
        Ok(tokio_rustls::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _dss: &tokio_rustls::rustls::DigitallySignedStruct,
    ) -> Result<tokio_rustls::rustls::client::danger::HandshakeSignatureValid, tokio_rustls::rustls::Error>
    {
        Ok(tokio_rustls::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<tokio_rustls::rustls::SignatureScheme> {
        tokio_rustls::rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
