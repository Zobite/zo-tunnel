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
    /// Start the tunnel server. Auto-creates config on first run.
    /// Example: zo-tunnel-server start --domain tunnel.example.com
    Start(StartArgs),

    /// Stop the tunnel server service.
    Stop,

    /// Restart the tunnel server service.
    Restart,

    /// Show current server configuration and status.
    Status,

    /// View server logs (journalctl).
    Logs(LogsArgs),

    /// Print a ready-to-copy client connect command.
    ClientCmd(ClientCmdArgs),

    /// Upgrade to the latest version from GitHub releases.
    Upgrade,

    /// Uninstall the server binary, systemd service, and config.
    Uninstall(UninstallArgs),
}

#[derive(Parser, Debug)]
struct StartArgs {
    /// Base domain for subdomain routing (e.g. tunnel.zobite.com).
    /// Each client will be accessible at <client_id>.<domain>.
    /// Required on first run; optional afterwards.
    #[arg(long)]
    domain: Option<String>,

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

    /// Run in foreground mode (for debugging or systemd).
    /// Default: install and start as systemd service.
    #[arg(long)]
    foreground: bool,
}

#[derive(Parser, Debug)]
struct LogsArgs {
    /// Number of recent log lines to show (default: 50)
    #[arg(long, short, default_value_t = 50)]
    lines: u32,

    /// Follow log output in real-time
    #[arg(long, short)]
    follow: bool,
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
        Command::Start(args) => cmd_start(args).await,
        Command::Stop => cmd_stop(),
        Command::Restart => cmd_restart(),
        Command::Status => cmd_status(),
        Command::Logs(args) => cmd_logs(args),
        Command::ClientCmd(args) => cmd_client_cmd(args),
        Command::Upgrade => cmd_upgrade(),
        Command::Uninstall(args) => cmd_uninstall(args),
    }
}

/// `zo-tunnel-server start` — setup config if needed, then start server.
///
/// - First run: requires `--domain`, creates config, installs systemd service, starts it.
/// - Subsequent runs: loads existing config and starts the service.
/// - With `--foreground`: runs the server in foreground (used by systemd or for debugging).
async fn cmd_start(args: StartArgs) -> Result<()> {
    // ── Foreground mode: just run the server directly ──
    if args.foreground {
        return run_foreground().await;
    }

    // ── Ensure config exists ──
    let config_path = config::ServerConfig::resolve_config_path();
    let (cfg, config_created) = if let Some(ref path) = config_path {
        if args.domain.is_some() && !args.force {
            eprintln!("⚠️  Config already exists at: {}", path.display());
            eprintln!("   Use --force to overwrite, or just run: zo-tunnel-server start");
            std::process::exit(1);
        }

        if args.domain.is_some() && args.force {
            // Re-create config with new settings
            let cfg = create_config(&args)?;
            (cfg, true)
        } else {
            // Load existing config
            let cfg = config::ServerConfig::load(path)
                .context("load config")?;
            (cfg, false)
        }
    } else {
        // No config exists — domain is required
        if args.domain.is_none() {
            eprintln!("❌ No config found. Domain is required on first run.");
            eprintln!();
            eprintln!("  Usage: zo-tunnel-server start --domain YOUR_DOMAIN");
            eprintln!();
            eprintln!("  Example:");
            eprintln!("    zo-tunnel-server start --domain tunnel.zobite.com");
            std::process::exit(1);
        }

        let cfg = create_config(&args)?;
        (cfg, true)
    };

    // ── Install systemd service if not yet installed ──
    if !zo_tunnel_protocol::self_update::is_service_installed() {
        zo_tunnel_protocol::self_update::install_systemd_service()
            .context("install systemd service")?;
    }

    // ── Start or restart the service ──
    if zo_tunnel_protocol::self_update::is_service_active() {
        zo_tunnel_protocol::self_update::restart_service()
            .context("restart service")?;
    } else {
        zo_tunnel_protocol::self_update::start_service()
            .context("start service")?;
    }

    // ── Print summary ──
    print_summary(&cfg, config_created);

    Ok(())
}

/// Create config from StartArgs and save to disk.
fn create_config(args: &StartArgs) -> Result<config::ServerConfig> {
    let domain = args.domain.as_deref().expect("domain must be set");

    let client_token = args
        .token
        .clone()
        .unwrap_or_else(|| config::ServerConfig::generate_token(24));

    let dashboard_token = args
        .dashboard_token
        .clone()
        .unwrap_or_else(|| config::ServerConfig::generate_token(16));

    let mut cfg = config::ServerConfig {
        domain: domain.to_string(),
        control_port: args.control_port,
        public_port: args.public_port,
        ..Default::default()
    };
    cfg.auth.tokens = vec![client_token];
    cfg.dashboard_auth.token = dashboard_token;

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

    cfg.save().context("save config")?;
    Ok(cfg)
}

/// Print server summary after start.
fn print_summary(cfg: &config::ServerConfig, config_created: bool) {
    let server_ip = detect_server_ip();

    println!();
    println!("╔══════════════════════════════════════╗");
    println!("║       Zo Tunnel Server — Started     ║");
    println!("╚══════════════════════════════════════╝");
    println!();

    if config_created {
        println!("  Config saved to: {}", config::ServerConfig::config_path().display());
        println!();
    }

    println!("  Domain:          *.{}", cfg.domain);
    println!("  Control port:    {}", cfg.control_port);
    println!("  Public port:     {}", cfg.public_port);
    println!("  Dashboard:       dashboard.{}", cfg.domain);
    println!("  TLS:             {}", if cfg.tls.enabled { "enabled" } else { "disabled" });
    if cfg.traefik.enabled {
        println!("  Traefik:         enabled ({})", cfg.traefik.config_dir);
    }
    println!();

    // Always show tokens and connect info
    let client_token = cfg.auth.tokens.first().map(|s| s.as_str()).unwrap_or("(none)");
    let dashboard_token = &cfg.dashboard_auth.token;
    let scheme = if cfg.tls.enabled { "https" } else { "http" };

    println!("  🔑 Client token:     {}", client_token);
    println!("  🔑 Dashboard token:  {}", dashboard_token);
    println!();
    println!("  ▸ Connect client:");
    println!("    zo-tunnel-client --server {}:{} \\", server_ip, cfg.control_port);
    println!("      --id my-api --local localhost:3000 \\");
    println!("      --token {}", client_token);
    println!();
    println!("  ▸ Access tunnel:    {}://my-api.{}", scheme, cfg.domain);
    println!("  ▸ Dashboard:        {}://dashboard.{}", scheme, cfg.domain);
    println!();

    if config_created {
        println!("  ▸ DNS setup (required):");
        println!("    Add a wildcard A record: *.{} → {}", cfg.domain, server_ip);
        println!();
    }

    println!("  ▸ Service management:");
    println!("    zo-tunnel-server stop");
    println!("    zo-tunnel-server restart");
    println!("    zo-tunnel-server logs -f");
    println!("    zo-tunnel-server status");
    println!();
}

/// Detect the server's public/primary IP address.
fn detect_server_ip() -> String {
    // Try `hostname -I` first (returns space-separated IPs, first is primary)
    if let Ok(output) = std::process::Command::new("hostname").arg("-I").output() {
        if output.status.success() {
            let ips = String::from_utf8_lossy(&output.stdout);
            if let Some(ip) = ips.split_whitespace().next() {
                if !ip.is_empty() {
                    return ip.to_string();
                }
            }
        }
    }
    "<VPS_IP>".to_string()
}

/// Run the server in foreground mode (used by systemd or for debugging).
async fn run_foreground() -> Result<()> {
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

/// `zo-tunnel-server stop` — stop the systemd service.
fn cmd_stop() -> Result<()> {
    if !zo_tunnel_protocol::self_update::is_service_active() {
        println!("ℹ️  Service is not running.");
        return Ok(());
    }
    zo_tunnel_protocol::self_update::stop_service()
        .context("stop service")?;
    println!("  Zo Tunnel Server stopped.");
    Ok(())
}

/// `zo-tunnel-server restart` — restart the systemd service.
fn cmd_restart() -> Result<()> {
    if !zo_tunnel_protocol::self_update::is_service_installed() {
        eprintln!("❌ Service not installed. Run `zo-tunnel-server start --domain <domain>` first.");
        std::process::exit(1);
    }
    zo_tunnel_protocol::self_update::restart_service()
        .context("restart service")?;
    println!("  Zo Tunnel Server restarted.");
    Ok(())
}

/// `zo-tunnel-server logs` — view server logs via journalctl.
fn cmd_logs(args: LogsArgs) -> Result<()> {
    let mut cmd_args = vec![
        "-u".to_string(),
        "zo-tunnel".to_string(),
        "-n".to_string(),
        args.lines.to_string(),
        "--no-pager".to_string(),
    ];

    if args.follow {
        cmd_args.push("-f".to_string());
    }

    let status = std::process::Command::new("journalctl")
        .args(&cmd_args)
        .status()
        .context("Failed to run journalctl. Is systemd available?")?;

    if !status.success() {
        anyhow::bail!("journalctl exited with error");
    }

    Ok(())
}

/// `zo-tunnel-server status` — show current config.
fn cmd_status() -> Result<()> {
    let config_path = match config::ServerConfig::resolve_config_path() {
        Some(p) => p,
        None => {
            println!("❌ No config found.");
            println!("   Run `zo-tunnel-server start --domain <domain>` first.");
            return Ok(());
        }
    };

    let cfg = config::ServerConfig::load(&config_path)
        .context("load config")?;

    // Check service status
    let service_status = if zo_tunnel_protocol::self_update::is_service_active() {
        "🟢 running"
    } else if zo_tunnel_protocol::self_update::is_service_installed() {
        "🔴 stopped"
    } else {
        "⚪ not installed"
    };

    println!();
    println!("╔══════════════════════════════════════╗");
    println!("║    Zo Tunnel Server — Status         ║");
    println!("╚══════════════════════════════════════╝");
    println!();
    println!("  Service:         {}", service_status);
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
            println!("   Run `zo-tunnel-server start --domain <domain>` first.");
            return Ok(());
        }
    };

    let cfg = config::ServerConfig::load(&config_path)
        .context("load config")?;

    if cfg.auth.tokens.is_empty() {
        println!("❌ No client tokens configured.");
        println!("   Run `zo-tunnel-server start --domain <domain>` to generate one.");
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
