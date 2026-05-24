use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

mod client;
mod config;

#[derive(Parser, Debug)]
#[command(name = "zobite-tunnel-client", about = "Zobite Tunnel tunnel client — run on your local machine")]
struct Cli {
    /// Path to YAML config file
    #[arg(long, short, env = "ZOBITE_CONFIG")]
    config: Option<PathBuf>,

    /// Server address (host:port)
    #[arg(long, env = "ZOBITE_SERVER")]
    server: Option<String>,

    /// Local service address to forward to
    #[arg(long, env = "ZOBITE_LOCAL")]
    local: Option<String>,

    /// Client ID (tunnel name)
    #[arg(long, env = "ZOBITE_CLIENT_ID")]
    id: Option<String>,

    /// Authentication token
    #[arg(long, env = "ZOBITE_TOKEN")]
    token: Option<String>,

    /// Request a dedicated TCP port (for SSH, databases, raw TCP)
    #[arg(long)]
    tcp: bool,

    /// Disable auto-reconnect
    #[arg(long)]
    no_reconnect: bool,
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
            tcp_mode: false,
            reconnect: config::ReconnectConfig::default(),
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
    if cli.tcp {
        cfg.tcp_mode = true;
    }

    if cfg.server.is_empty() {
        eprintln!("Error: --server is required (or set in config/env)");
        std::process::exit(1);
    }

    tracing::info!("╔══════════════════════════════════════╗");
    tracing::info!("║          Zobite Tunnel Client v{}         ║", env!("CARGO_PKG_VERSION"));
    tracing::info!("╚══════════════════════════════════════╝");
    tracing::info!(
        "ID:'{}' | Server:{} | Local:{} | Mode:{} | Reconnect:{}",
        cfg.client_id,
        cfg.server,
        cfg.local_addr,
        if cfg.tcp_mode { "TCP" } else { "HTTP" },
        cfg.reconnect.enabled
    );

    let client = client::Client::new(
        cfg.server.clone(),
        cfg.local_addr.clone(),
        cfg.client_id.clone(),
        cfg.token.clone(),
        cfg.tcp_mode,
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
