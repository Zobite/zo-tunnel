use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

mod client;
mod config;

#[derive(Parser, Debug)]
#[command(name = "zo-tunnel-client", about = "Zo Tunnel tunnel client — run on your local machine")]
struct Cli {
    /// Path to YAML config file
    #[arg(long, short, env = "ZO_CONFIG")]
    config: Option<PathBuf>,

    /// Server address (host:port)
    #[arg(long, env = "ZO_SERVER")]
    server: Option<String>,

    /// Local service address to forward to
    #[arg(long, env = "ZO_LOCAL")]
    local: Option<String>,

    /// Client ID (tunnel name)
    #[arg(long, env = "ZO_CLIENT_ID")]
    id: Option<String>,

    /// Authentication token
    #[arg(long, env = "ZO_TOKEN")]
    token: Option<String>,

    /// Disable auto-reconnect
    #[arg(long)]
    no_reconnect: bool,

    /// Enable TLS for the control channel (server must also have TLS enabled)
    #[arg(long, env = "ZO_TLS")]
    tls: bool,

    /// Server name for TLS SNI/cert verification (default: hostname from --server)
    #[arg(long)]
    tls_server_name: Option<String>,

    /// Skip TLS certificate verification (DANGEROUS — only for self-signed certs)
    #[arg(long)]
    tls_skip_verify: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // Load config: file → CLI overrides
    let mut cfg = if let Some(ref path) = cli.config {
        config::ClientConfig::load(path)?
    } else {
        config::ClientConfig {
            server: String::new(),
            client_id: "default".into(),
            local_addr: "localhost:3000".into(),
            token: String::new(),
            reconnect: config::ReconnectConfig::default(),
            tls: config::ClientTlsConfig::default(),
        }
    };

    // CLI overrides
    if let Some(ref s) = cli.server {
        cfg.server = s.clone();
    }
    if let Some(ref l) = cli.local {
        cfg.local_addr = l.clone();
    }
    if let Some(ref id) = cli.id {
        cfg.client_id = id.clone();
    }
    if let Some(ref t) = cli.token {
        cfg.token = t.clone();
    }
    if cli.no_reconnect {
        cfg.reconnect.enabled = false;
    }
    if cli.tls {
        cfg.tls.enabled = true;
    }
    if let Some(ref name) = cli.tls_server_name {
        cfg.tls.server_name = name.clone();
    }
    if cli.tls_skip_verify {
        cfg.tls.skip_verify = true;
    }

    if cfg.server.is_empty() {
        eprintln!("Error: --server is required (or set in config/env)");
        std::process::exit(1);
    }

    tracing::info!("╔══════════════════════════════════════╗");
    tracing::info!("║          Zo Tunnel Client v{}         ║", env!("CARGO_PKG_VERSION"));
    tracing::info!("╚══════════════════════════════════════╝");
    tracing::info!(
        "ID:'{}' | Server:{} | Local:{} | TLS:{} | Reconnect:{}",
        cfg.client_id,
        cfg.server,
        cfg.local_addr,
        if cfg.tls.enabled { "yes" } else { "no" },
        cfg.reconnect.enabled
    );

    if cfg.tls.enabled && cfg.tls.skip_verify {
        tracing::warn!("⚠️  TLS certificate verification DISABLED — insecure!");
    }

    let client = client::Client::new(
        cfg.server.clone(),
        cfg.local_addr.clone(),
        cfg.client_id.clone(),
        cfg.token.clone(),
        cfg.tls.clone(),
    );

    // Exponential backoff reconnect loop
    let mut backoff_secs = 1u64;
    let max_backoff = cfg.reconnect.max_interval;

    loop {
        match client.run().await {
            Ok(_) => {
                tracing::info!("Session ended cleanly");
                backoff_secs = 1; // Reset backoff on clean exit
            }
            Err(e) => {
                tracing::error!("Session error: {:#}", e);
            }
        }

        if !cfg.reconnect.enabled {
            tracing::info!("Reconnect disabled, exiting");
            break;
        }

        tracing::info!("🔄 Reconnecting in {}s...", backoff_secs);
        tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;

        // Exponential backoff: 1 → 2 → 4 → 8 → ... → max
        backoff_secs = (backoff_secs * 2).min(max_backoff);
    }

    Ok(())
}
