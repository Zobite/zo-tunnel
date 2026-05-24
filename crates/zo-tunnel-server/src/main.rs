use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

mod config;
mod dashboard;
mod metrics;
mod proxy;
mod registry;
mod server;

#[derive(Parser, Debug)]
#[command(name = "zo-tunnel-server", about = "Zo Tunnel tunnel server — run on your VPS")]
struct Cli {
    /// Path to YAML config file
    #[arg(long, short, env = "ZO_CONFIG")]
    config: Option<PathBuf>,

    /// Port for client control connections
    #[arg(long, env = "ZO_CONTROL_PORT")]
    control_port: Option<u16>,

    /// Port for public traffic
    #[arg(long, env = "ZO_PUBLIC_PORT")]
    public_port: Option<u16>,

    /// Port for dashboard
    #[arg(long, env = "ZO_DASHBOARD_PORT")]
    dashboard_port: Option<u16>,

    /// Required token(s) for client authentication (comma-separated)
    #[arg(long, env = "ZO_TOKEN")]
    token: Option<String>,

    /// Routing mode: path or subdomain
    #[arg(long, env = "ZO_ROUTING_MODE")]
    routing_mode: Option<String>,

    /// Domain for subdomain routing (e.g. example.com)
    #[arg(long, env = "ZO_DOMAIN")]
    domain: Option<String>,

    /// TLS certificate file
    #[arg(long, env = "ZO_TLS_CERT")]
    tls_cert: Option<String>,

    /// TLS private key file
    #[arg(long, env = "ZO_TLS_KEY")]
    tls_key: Option<String>,

    /// Dashboard admin token (if set, dashboard requires authentication)
    #[arg(long, env = "ZO_DASHBOARD_TOKEN")]
    dashboard_token: Option<String>,

    /// Dashboard session TTL in seconds (default: 86400 = 24h)
    #[arg(long, env = "ZO_SESSION_TTL")]
    session_ttl: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // Load config: file → CLI overrides
    let mut cfg = if let Some(ref path) = cli.config {
        config::ServerConfig::load(path)
            .with_context(|| format!("load config from {:?}", path))?
    } else {
        config::ServerConfig::default()
    };

    // Apply CLI overrides
    if let Some(p) = cli.control_port {
        cfg.control_port = p;
    }
    if let Some(p) = cli.public_port {
        cfg.public_port = p;
    }
    if let Some(p) = cli.dashboard_port {
        cfg.dashboard_port = p;
    }
    if let Some(ref t) = cli.token {
        cfg.auth.tokens = t.split(',').map(|s| s.trim().to_string()).collect();
    }
    if let Some(ref mode) = cli.routing_mode {
        cfg.routing_mode = match mode.as_str() {
            "subdomain" => config::RoutingMode::Subdomain,
            _ => config::RoutingMode::Path,
        };
    }
    if let Some(ref d) = cli.domain {
        cfg.domain = Some(d.clone());
    }
    if let Some(ref cert) = cli.tls_cert {
        cfg.tls.enabled = true;
        cfg.tls.cert = cert.clone();
    }
    if let Some(ref key) = cli.tls_key {
        cfg.tls.key = key.clone();
    }
    if let Some(ref dt) = cli.dashboard_token {
        cfg.dashboard_auth.token = dt.clone();
    }
    if let Some(ttl) = cli.session_ttl {
        cfg.dashboard_auth.session_ttl_secs = ttl;
    }

    tracing::info!("╔══════════════════════════════════════╗");
    tracing::info!("║          Zo Tunnel Server v{}         ║", env!("CARGO_PKG_VERSION"));
    tracing::info!("╚══════════════════════════════════════╝");
    tracing::info!(
        "Control:{} | Public:{} | Dashboard:{} | Routing:{:?} | TLS:{}",
        cfg.control_port,
        cfg.public_port,
        cfg.dashboard_port,
        cfg.routing_mode,
        cfg.tls.enabled,
    );

    let srv = server::Server::new(cfg);
    srv.run().await.context("server run failed")?;

    Ok(())
}
