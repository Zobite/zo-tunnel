use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use std::os::unix::process::CommandExt;
use tracing_subscriber::EnvFilter;

mod client;
mod config;
mod tunnel_manager;
mod web_server;

#[derive(Parser, Debug)]
#[command(
    name = "zo-tunnel-client",
    about = "Zo Tunnel client — manage tunnels via local web UI",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Start the client web UI in the background.
    Start(StartArgs),

    /// Stop the running client process.
    Stop,

    /// Show current status.
    Status,

    /// Upgrade to the latest version from GitHub releases.
    Upgrade,

    /// Uninstall the client binary.
    Uninstall(UninstallArgs),

    /// Run in foreground (used internally by `start`).
    #[command(hide = true)]
    Foreground(StartArgs),
}

#[derive(Parser, Debug, Clone)]
struct StartArgs {
    /// Web UI port (default: 16200)
    #[arg(long, default_value_t = 16200)]
    port: u16,

    /// Bind address for web UI (default: 127.0.0.1)
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,
}

#[derive(Parser, Debug)]
struct UninstallArgs {
    /// Skip confirmation prompt
    #[arg(long, short)]
    yes: bool,
}

// ─── PID file management ─────────────────────────────────────────

/// Return the PID file path: `~/.zo-tunnel/client.pid`
fn pid_path() -> Result<std::path::PathBuf> {
    Ok(config::credentials_dir()?.join("client.pid"))
}

/// Write PID to file.
fn write_pid(pid: u32) -> Result<()> {
    let dir = config::credentials_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = pid_path()?;
    std::fs::write(&path, pid.to_string())?;
    Ok(())
}

/// Read PID from file. Returns None if file doesn't exist or is invalid.
fn read_pid() -> Result<Option<u32>> {
    let path = pid_path()?;
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    match content.trim().parse::<u32>() {
        Ok(pid) => Ok(Some(pid)),
        Err(_) => {
            // Invalid PID file, clean it up
            let _ = std::fs::remove_file(&path);
            Ok(None)
        }
    }
}

/// Remove PID file.
fn remove_pid() {
    if let Ok(path) = pid_path() {
        let _ = std::fs::remove_file(&path);
    }
}

/// Check if a process with the given PID exists at all.
fn is_process_alive(pid: u32) -> bool {
    // PID 0 is special in kill(2): it targets the entire calling process group.
    // Never treat it as a valid PID.
    if pid == 0 {
        return false;
    }
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Check if a process with the given PID is our zo-tunnel-client process.
///
/// Simply checking `kill(pid, 0)` is not sufficient — after a reboot or process
/// crash, the OS may reassign the same PID to an unrelated process. We verify
/// by reading `/proc/<pid>/exe` (or `/proc/<pid>/cmdline` as fallback) to
/// confirm the binary is actually zo-tunnel-client.
fn is_our_process(pid: u32) -> bool {
    // First check: is the process alive at all?
    if !is_process_alive(pid) {
        return false;
    }

    // Second check: verify it's actually zo-tunnel-client via /proc
    let exe_link = format!("/proc/{}/exe", pid);
    if let Ok(exe_path) = std::fs::read_link(&exe_link) {
        return is_zo_tunnel_binary(&exe_path.to_string_lossy());
    }

    // Fallback: check /proc/<pid>/cmdline (exe symlink may be inaccessible)
    let cmdline_path = format!("/proc/{}/cmdline", pid);
    if let Ok(cmdline) = std::fs::read(&cmdline_path) {
        // cmdline is NUL-separated bytes; first entry is the executable
        if let Some(arg0) = cmdline.split(|&b| b == 0).next() {
            if let Ok(arg0_str) = std::str::from_utf8(arg0) {
                return is_zo_tunnel_binary(arg0_str);
            }
        }
    }

    // /proc not available (non-Linux) — fall back to process-exists-only check.
    // This is less safe but better than refusing to work entirely.
    true
}

/// Check if a path string refers to a zo-tunnel-client binary.
///
/// Handles the Linux kernel's ` (deleted)` suffix that appears in `/proc/pid/exe`
/// when the binary on disk has been replaced (e.g. after `upgrade`):
///   /usr/local/bin/zo-tunnel-client           -> match
///   /usr/local/bin/zo-tunnel-client (deleted) -> match
///   /usr/local/bin/zo-tunnel-server           -> no match
///   /home/zo-tunnel-client-dev/nginx          -> no match
fn is_zo_tunnel_binary(path: &str) -> bool {
    // Strip the " (deleted)" suffix if present
    let cleaned = path.strip_suffix(" (deleted)").unwrap_or(path);
    // Extract the file basename and do exact match
    std::path::Path::new(cleaned)
        .file_name()
        .and_then(|n| n.to_str())
        == Some("zo-tunnel-client")
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Start(args)) => cmd_start(args),
        Some(Command::Stop) => cmd_stop(),
        Some(Command::Status) => cmd_status(),
        Some(Command::Upgrade) => cmd_upgrade(),
        Some(Command::Uninstall(args)) => cmd_uninstall(args),
        Some(Command::Foreground(args)) => cmd_foreground(args.bind, args.port).await,
        // Show help if no subcommand is provided
        None => {
            let mut cmd = Cli::command();
            cmd.print_help()?;
            println!();
            Ok(())
        }
    }
}

/// `zo-tunnel-client start` — spawn background process and exit.
fn cmd_start(args: StartArgs) -> Result<()> {
    // Check if already running
    if let Some(pid) = read_pid()? {
        if is_our_process(pid) {
            eprintln!("  Zo Tunnel Client is already running (PID {}).", pid);
            eprintln!();
            eprintln!("  Run 'zo-tunnel-client stop' to stop it first.");
            return Ok(());
        }
        // Stale PID file — clean up
        remove_pid();
    }

    // Get current executable path
    let exe = std::env::current_exe().context("failed to determine current executable path")?;

    // Determine log file path
    let log_dir = config::credentials_dir()?;
    std::fs::create_dir_all(&log_dir)?;
    let log_file_path = log_dir.join("client.log");

    // Open log file for stdout/stderr
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)
        .context("failed to open log file")?;
    let log_file_err = log_file
        .try_clone()
        .context("failed to clone log file handle")?;

    // Spawn the process in the background with `foreground` subcommand.
    // process_group(0) creates a new process group so the child survives
    // terminal hangup (SIGHUP) when the parent's SSH/terminal session ends.
    let child = std::process::Command::new(&exe)
        .arg("foreground")
        .arg("--bind")
        .arg(&args.bind)
        .arg("--port")
        .arg(args.port.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log_file))
        .stderr(std::process::Stdio::from(log_file_err))
        .process_group(0)
        .spawn()
        .context("failed to start background process")?;

    let pid = child.id();

    // Parent writes PID file — child will NOT write it again when spawned via `start`.
    write_pid(pid)?;

    // Brief wait to check if child crashes immediately (e.g. port already in use)
    std::thread::sleep(std::time::Duration::from_millis(300));
    if !is_process_alive(pid) {
        remove_pid();
        anyhow::bail!("Failed to start. Check logs: {}", log_file_path.display());
    }

    println!();
    println!("  Zo Tunnel Client started (PID {}).", pid);
    println!();
    println!("  Web UI:  http://{}:{}", args.bind, args.port);
    println!("  Logs:    {}", log_file_path.display());
    println!();
    println!("  Stop:    zo-tunnel-client stop");
    println!("  Status:  zo-tunnel-client status");
    println!();

    Ok(())
}

/// `zo-tunnel-client stop` — send SIGTERM to the running process.
fn cmd_stop() -> Result<()> {
    let pid = match read_pid()? {
        Some(pid) => pid,
        None => {
            eprintln!("  Zo Tunnel Client is not running (no PID file found).");
            return Ok(());
        }
    };

    if !is_our_process(pid) {
        eprintln!("  Zo Tunnel Client is not running (stale PID {}).", pid);
        remove_pid();
        return Ok(());
    }

    // Send SIGTERM for graceful shutdown
    let ret = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        remove_pid();
        anyhow::bail!("Failed to stop process {}: {}", pid, err);
    }

    // Wait for process to exit (up to 2 seconds)
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if !is_process_alive(pid) {
            remove_pid();
            println!("  Zo Tunnel Client stopped.");
            return Ok(());
        }
    }

    // Process didn't exit in time — verify it's still ours before force kill.
    // The PID could have been reused by an unrelated process in the meantime.
    if is_our_process(pid) {
        eprintln!("  Process {} did not exit in time, sending SIGKILL...", pid);
        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    remove_pid();
    println!("  Zo Tunnel Client stopped.");
    Ok(())
}

/// Run in foreground — the actual web server + tunnel logic.
/// Called by `zo-tunnel-client foreground` (spawned by `start`) or bare `zo-tunnel-client`.
async fn cmd_foreground(bind: String, port: u16) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Zo Tunnel Client v{}", env!("CARGO_PKG_VERSION"));

    // Determine PID ownership before writing.
    // - If spawned by `start`: parent already wrote our PID — skip.
    // - If another zo-tunnel-client is running: refuse to overwrite its PID
    //   (prevents orphaning the existing instance if we crash on bind).
    // - Otherwise (no file, stale PID): write our PID.
    match read_pid().unwrap_or(None) {
        Some(pid) if pid == std::process::id() => {
            // Parent already wrote our PID via `start`
        }
        Some(pid) if is_our_process(pid) => {
            anyhow::bail!(
                "Another Zo Tunnel Client is already running (PID {}). \
                 Run 'zo-tunnel-client stop' first.",
                pid
            );
        }
        _ => {
            // No PID file, stale PID, or not our process — claim it
            write_pid(std::process::id())?;
        }
    }

    // From here on, any error exit must clean up the PID file.
    match run_foreground(bind, port).await {
        Ok(()) => Ok(()),
        Err(e) => {
            remove_pid();
            Err(e)
        }
    }
}

/// Inner foreground logic, separated so the caller can clean up PID on error.
async fn run_foreground(bind: String, port: u16) -> Result<()> {
    // ── Create app state ──
    let app_state = web_server::AppState::new(bind.clone(), port).await;

    // If credentials exist, auto-start tunnels
    if app_state.is_connected().await {
        tracing::info!("Using saved credentials from ~/.zo-tunnel/");
        if let Some(mgr) = app_state.manager().await {
            mgr.start_all().await;
            tracing::info!("Auto-started {} tunnel(s)", mgr.running_count());
        }
    } else {
        tracing::info!("No credentials found — open the web UI to connect.");
    }

    // ── Start web server ──
    let bind_addr = format!("{}:{}", bind, port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to bind to {}: {}", bind_addr, e))?;

    tracing::info!("Web UI: http://{}", bind_addr);

    let router = web_server::create_router(app_state.clone());

    let shutdown = async {
        // Listen for both SIGTERM (from `stop`) and SIGINT (Ctrl+C)
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to register SIGTERM handler");

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Received SIGINT, shutting down...");
            }
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM, shutting down...");
            }
        }
    };

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|e| anyhow::anyhow!("Web server error: {}", e))?;

    // Graceful cleanup
    if let Some(mgr) = app_state.manager().await {
        mgr.stop_all();
    }

    remove_pid();
    tracing::info!("Goodbye.");

    Ok(())
}

/// `zo-tunnel-client status`
fn cmd_status() -> Result<()> {
    eprintln!("Zo Tunnel Client v{}", env!("CARGO_PKG_VERSION"));
    eprintln!();

    // Check running state
    match read_pid().unwrap_or(None) {
        Some(pid) if is_our_process(pid) => {
            eprintln!("  Status:  running (PID {})", pid);
        }
        Some(_) => {
            eprintln!("  Status:  stopped (stale PID file)");
            remove_pid();
        }
        None => {
            eprintln!("  Status:  stopped");
        }
    }

    match config::SavedCredentials::load()? {
        Some(creds) => {
            eprintln!("  Server:  {}", creds.server);
            eprintln!("  Token:   {}", creds.masked_token());
            eprintln!(
                "  TLS:     {}",
                if creds.tls.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            );

            match config::TunnelsConfig::load() {
                Ok(cfg) => {
                    eprintln!("  Tunnels: {} configured", cfg.tunnels.len());
                }
                Err(_) => {
                    eprintln!("  Tunnels: 0 configured");
                }
            }
        }
        None => {
            eprintln!("  Not connected.");
            eprintln!("  Run 'zo-tunnel-client start' and open the web UI to connect.");
        }
    }

    eprintln!();
    Ok(())
}

/// `zo-tunnel-client upgrade`
fn cmd_upgrade() -> Result<()> {
    zo_tunnel_protocol::self_update::upgrade("zo-tunnel-client", env!("CARGO_PKG_VERSION"))
}

/// `zo-tunnel-client uninstall`
fn cmd_uninstall(args: UninstallArgs) -> Result<()> {
    if config::SavedCredentials::exists() {
        eprintln!("  Saved credentials at ~/.zo-tunnel/ will also be removed.");
    }

    // Run uninstall first (may prompt for confirmation).
    // Only stop the running client AFTER the user confirms.
    zo_tunnel_protocol::self_update::uninstall(
        "zo-tunnel-client",
        zo_tunnel_protocol::self_update::Component::Client,
        args.yes,
        false,
    )?;

    // User confirmed — now stop the running instance
    if let Some(pid) = read_pid().unwrap_or(None) {
        if is_our_process(pid) {
            eprintln!("  Stopping running client (PID {})...", pid);
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            for _ in 0..10 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if !is_process_alive(pid) {
                    break;
                }
            }
            if is_our_process(pid) {
                unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            remove_pid();
        }
    }

    if let Ok(dir) = config::credentials_dir() {
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
            eprintln!("  Removed saved credentials from {}", dir.display());
        }
    }

    Ok(())
}
