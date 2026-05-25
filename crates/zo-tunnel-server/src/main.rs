use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod config;
mod dashboard;
mod metrics;
mod proxy;
mod registry;
mod server;
mod traefik;

#[derive(Parser, Debug)]
#[command(
    name = "zo-tunnel-server",
    about = "Zo Tunnel server — expose local services to the internet through your VPS",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Initial setup — configure tokens and ports.
    /// Saves config to /etc/zo-tunnel/server.yaml (or ~/.config/zo-tunnel/).
    Setup(SetupArgs),

    /// Start the tunnel server using saved config.
    Start,

    /// Show current server configuration and status.
    Status,

    /// Print a ready-to-copy client connect command.
    ClientCmd(ClientCmdArgs),

    /// Upgrade to the latest version from GitHub releases.
    Upgrade,

    /// Uninstall the server binary, systemd service, and config.
    Uninstall(UninstallArgs),
}

#[derive(Parser, Debug)]
struct SetupArgs {
    /// Base domain for subdomain routing (e.g. tunnel.zobite.com).
    /// Each client will be accessible at <client_id>.<domain>.
    #[arg(long)]
    domain: String,

    /// Authentication token for tunnel clients.
    /// If omitted, a secure random token is auto-generated.
    #[arg(long)]
    token: Option<String>,

    /// Dashboard admin token.
    /// If omitted, a secure random token is auto-generated.
    #[arg(long)]
    dashboard_token: Option<String>,

    /// Port for client control connections (default: 6200)
    #[arg(long, default_value_t = 6200)]
    control_port: u16,

    /// Port for public HTTP traffic (default: 6210)
    #[arg(long, default_value_t = 6210)]
    public_port: u16,

    /// TLS certificate file (PEM)
    #[arg(long)]
    tls_cert: Option<String>,

    /// TLS private key file (PEM)
    #[arg(long)]
    tls_key: Option<String>,

    /// Overwrite existing config without asking
    #[arg(long)]
    force: bool,

    /// Enable Traefik integration — auto-create route configs per client.
    /// Specify the Traefik dynamic config directory (e.g. /etc/traefik/dynamic).
    #[arg(long)]
    traefik_dir: Option<String>,
}

#[derive(Parser, Debug)]
struct ClientCmdArgs {
    /// Client ID / tunnel name (default: my-app)
    #[arg(long, default_value = "my-app")]
    id: String,

    /// Local service address (default: localhost:3000)
    #[arg(long, default_value = "localhost:3000")]
    local: String,
}

#[derive(Parser, Debug)]
struct UninstallArgs {
    /// Skip confirmation prompt
    #[arg(long, short)]
    yes: bool,

    /// Keep config files (only remove binary and service)
    #[arg(long)]
    keep_config: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Setup(args) => cmd_setup(args),
        Command::Start => cmd_start().await,
        Command::Status => cmd_status(),
        Command::ClientCmd(args) => cmd_client_cmd(args),
        Command::Upgrade => cmd_upgrade(),
        Command::Uninstall(args) => cmd_uninstall(args),
    }
}

/// `zo-tunnel-server setup` — generate config and save to disk.
fn cmd_setup(args: SetupArgs) -> Result<()> {
    // Check for existing config
    let config_path = config::ServerConfig::config_path();
    if config_path.exists() && !args.force {
        eprintln!("⚠️  Config already exists at: {}", config_path.display());
        eprintln!("   Use --force to overwrite.");
        std::process::exit(1);
    }

    // Generate or use provided tokens
    let client_token = args
        .token
        .unwrap_or_else(|| config::ServerConfig::generate_token(24));

    let dashboard_token = args
        .dashboard_token
        .unwrap_or_else(|| config::ServerConfig::generate_token(16));

    // Build config
    let mut cfg = config::ServerConfig {
        domain: args.domain,
        control_port: args.control_port,
        public_port: args.public_port,
        ..Default::default()
    };
    cfg.auth.tokens = vec![client_token.clone()];
    cfg.dashboard_auth.token = dashboard_token.clone();

    // TLS
    if let Some(ref cert) = args.tls_cert {
        cfg.tls.enabled = true;
        cfg.tls.cert = cert.clone();
    }
    if let Some(ref key) = args.tls_key {
        cfg.tls.key = key.clone();
    }

    // Traefik integration
    if let Some(ref dir) = args.traefik_dir {
        cfg.traefik.enabled = true;
        cfg.traefik.config_dir = dir.clone();
    }

    // Save
    let saved_path = cfg.save().context("save config")?;

    // Print summary
    println!();
    println!("╔══════════════════════════════════════╗");
    println!("║     Zo Tunnel Server — Setup Done    ║");
    println!("╚══════════════════════════════════════╝");
    println!();
    println!("  Config saved to: {}", saved_path.display());
    println!();
    println!("  Domain:          *.{}", cfg.domain);
    println!("  Control port:    {}", cfg.control_port);
    println!("  Public port:     {}", cfg.public_port);
    println!("  Dashboard:       dashboard.{}", cfg.domain);
    println!("  TLS:             {}", if cfg.tls.enabled { "enabled" } else { "disabled" });
    if cfg.traefik.enabled {
        println!("  Traefik:         enabled ({})", cfg.traefik.config_dir);
    }
    println!();
    println!("  🔑 Client token:     {}", client_token);
    println!("  🔑 Dashboard token:  {}", dashboard_token);
    println!();
    println!("  ▸ Start server:  zo-tunnel-server start");
    println!("  ▸ Connect client:");
    println!("    zo-tunnel-client --server <VPS_IP>:{} \\", cfg.control_port);
    println!("      --id my-api --local localhost:3000 \\");
    println!("      --token {}", client_token);
    println!();
    println!("  ▸ Access tunnel:    http://my-api.{}", cfg.domain);
    println!("  ▸ Dashboard:        http://dashboard.{}", cfg.domain);
    println!();
    println!("  ▸ DNS setup (required):");
    println!("    Add a wildcard A record: *.{} → <VPS_IP>", cfg.domain);
    println!();

    Ok(())
}

/// `zo-tunnel-server start` — load saved config and run server.
async fn cmd_start() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config_path = config::ServerConfig::resolve_config_path()
        .unwrap_or_else(config::ServerConfig::config_path);

    let cfg = config::ServerConfig::load(&config_path)
        .context("load config")?;

    tracing::info!("╔══════════════════════════════════════╗");
    tracing::info!("║          Zo Tunnel Server v{}         ║", env!("CARGO_PKG_VERSION"));
    tracing::info!("╚══════════════════════════════════════╝");

    tracing::info!(
        "Domain:*.{} | Control:{} | Public:{} | TLS:{}",
        cfg.domain, cfg.control_port, cfg.public_port, cfg.tls.enabled,
    );
    tracing::info!("Config: {}", config_path.display());

    let srv = server::Server::new(cfg);
    srv.run().await.context("server run failed")?;

    Ok(())
}

/// `zo-tunnel-server status` — show current config.
fn cmd_status() -> Result<()> {
    let config_path = match config::ServerConfig::resolve_config_path() {
        Some(p) => p,
        None => {
            println!("❌ No config found.");
            println!("   Run `zo-tunnel-server setup --domain <domain>` first.");
            return Ok(());
        }
    };

    let cfg = config::ServerConfig::load(&config_path)
        .context("load config")?;

    println!();
    println!("╔══════════════════════════════════════╗");
    println!("║    Zo Tunnel Server — Status         ║");
    println!("╚══════════════════════════════════════╝");
    println!();
    println!("  Config:          {}", config_path.display());
    println!("  Domain:          *.{}", cfg.domain);
    println!("  Control port:    {}", cfg.control_port);
    println!("  Public port:     {}", cfg.public_port);
    println!("  Dashboard:       dashboard.{}", cfg.domain);
    println!("  TLS:             {}", if cfg.tls.enabled { "enabled" } else { "disabled" });
    println!("  Client tokens:   {} configured", cfg.auth.tokens.len());
    println!("  Dashboard auth:  {}", if cfg.dashboard_auth_enabled() { "enabled" } else { "disabled" });
    if cfg.traefik.enabled {
        println!("  Traefik:         enabled ({})", cfg.traefik.config_dir);
    } else {
        println!("  Traefik:         disabled");
    }
    println!();

    for (i, token) in cfg.auth.tokens.iter().enumerate() {
        let masked = if token.len() > 8 {
            format!("{}...{}", &token[..4], &token[token.len()-4..])
        } else {
            "****".into()
        };
        println!("  Token #{}: {}", i + 1, masked);
    }
    println!();

    Ok(())
}

/// `zo-tunnel-server client-cmd` — print a ready-to-copy client connect command.
fn cmd_client_cmd(args: ClientCmdArgs) -> Result<()> {
    let config_path = match config::ServerConfig::resolve_config_path() {
        Some(p) => p,
        None => {
            println!("❌ No config found.");
            println!("   Run `zo-tunnel-server setup --domain <domain>` first.");
            return Ok(());
        }
    };

    let cfg = config::ServerConfig::load(&config_path)
        .context("load config")?;

    if cfg.auth.tokens.is_empty() {
        println!("❌ No client tokens configured.");
        println!("   Run `zo-tunnel-server setup --domain <domain>` to generate one.");
        return Ok(());
    }

    let server_addr = if cfg.domain.is_empty() {
        format!("<VPS_IP>:{}", cfg.control_port)
    } else {
        format!("{}:{}", cfg.domain, cfg.control_port)
    };

    let token = &cfg.auth.tokens[0];
    let scheme = if cfg.tls.enabled { "https" } else { "http" };

    println!();
    println!("╔══════════════════════════════════════╗");
    println!("║   Zo Tunnel — Client Connect Command ║");
    println!("╚══════════════════════════════════════╝");
    println!();
    println!("  Copy and run on your local machine:");
    println!();
    println!("  zo-tunnel-client --server {} --token {} --id {} --local {}", server_addr, token, args.id, args.local);
    println!();

    if !cfg.domain.is_empty() {
        println!("  ▸ Access tunnel:  {}://{}.{}", scheme, args.id, cfg.domain);
        println!("  ▸ Dashboard:      {}://dashboard.{}", scheme, cfg.domain);
    }
    println!();
    println!("  💡 Customize --id and --local to match your app.");
    println!("     Example: --id my-api --local localhost:8080");
    println!();

    Ok(())
}

/// `zo-tunnel-server upgrade` — self-upgrade from GitHub releases.
fn cmd_upgrade() -> Result<()> {
    zo_tunnel_protocol::self_update::upgrade(
        "zo-tunnel-server",
        env!("CARGO_PKG_VERSION"),
    )
}

/// `zo-tunnel-server uninstall` — remove binary, service, and config.
fn cmd_uninstall(args: UninstallArgs) -> Result<()> {
    zo_tunnel_protocol::self_update::uninstall(
        "zo-tunnel-server",
        zo_tunnel_protocol::self_update::Component::Server,
        args.yes,
        args.keep_config,
    )
}
