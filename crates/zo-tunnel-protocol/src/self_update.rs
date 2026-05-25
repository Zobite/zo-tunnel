//! Self-update, uninstall, and systemd service management utilities
//! shared by server and client binaries.
//!
//! - `upgrade()` — download the latest release from GitHub and replace the running binary
//! - `uninstall()` — remove binary, config, and systemd service
//! - `install_systemd_service()` — create and enable systemd service
//! - `start_service()` / `stop_service()` / `restart_service()` — manage service lifecycle

use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = "Zobite/zo-tunnel";
const INSTALL_DIR: &str = "/usr/local/bin";

// ─── ANSI colors ─────────────────────────────────────────────────

const GREEN: &str = "\x1b[0;32m";
const BLUE: &str = "\x1b[0;34m";
const YELLOW: &str = "\x1b[1;33m";
const NC: &str = "\x1b[0m";

fn info(msg: &str) {
    eprintln!("{BLUE}▸{NC} {msg}");
}
fn ok(msg: &str) {
    eprintln!("{GREEN}✅{NC} {msg}");
}
fn warn(msg: &str) {
    eprintln!("{YELLOW}⚠️{NC}  {msg}");
}

// ─── OS / arch detection ─────────────────────────────────────────

fn detect_target() -> anyhow::Result<String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let os_label = match os {
        "linux" => "linux",
        "macos" => "darwin",
        _ => anyhow::bail!("Unsupported OS: {os}"),
    };

    let arch_label = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => anyhow::bail!("Unsupported architecture: {arch}"),
    };

    Ok(format!("{os_label}-{arch_label}"))
}

// ─── GitHub API helpers ──────────────────────────────────────────

/// Fetch the latest release tag from GitHub (e.g. "v0.4.1").
pub fn fetch_latest_version() -> anyhow::Result<String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let output = Command::new("curl")
        .args(["-sSL", "--max-time", "15", &url])
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run curl: {e}. Is curl installed?"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to fetch latest release: {stderr}");
    }

    let body = String::from_utf8_lossy(&output.stdout);

    // Simple JSON parsing — extract "tag_name": "vX.Y.Z"
    let tag = body
        .split("\"tag_name\"")
        .nth(1)
        .and_then(|s| s.split('"').nth(1))
        .ok_or_else(|| anyhow::anyhow!("Could not parse release tag from GitHub API response"))?;

    Ok(tag.to_string())
}

/// Compare two semver strings, stripping leading 'v'.
/// Returns true if `latest` is newer than `current`.
pub fn is_newer(current: &str, latest: &str) -> bool {
    let parse = |s: &str| -> (u32, u32, u32) {
        let s = s.strip_prefix('v').unwrap_or(s);
        let parts: Vec<u32> = s.split('.').filter_map(|p| p.parse().ok()).collect();
        (
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
            parts.get(2).copied().unwrap_or(0),
        )
    };
    parse(latest) > parse(current)
}

// ─── Upgrade ─────────────────────────────────────────────────────

/// Self-upgrade the binary by downloading the latest release from GitHub.
///
/// * `binary_name` — e.g. "zo-tunnel-server" or "zo-tunnel-client"
/// * `current_version` — e.g. "0.4.1" (from `env!("CARGO_PKG_VERSION")`)
pub fn upgrade(binary_name: &str, current_version: &str) -> anyhow::Result<()> {
    let current_tag = format!("v{current_version}");
    info(&format!("Current version: {current_tag}"));
    info("Checking latest release...");

    let latest_tag = fetch_latest_version()?;
    info(&format!("Latest version:  {latest_tag}"));

    if !is_newer(current_version, &latest_tag) {
        ok(&format!("Already up to date ({current_tag})"));
        return Ok(());
    }

    let target = detect_target()?;
    let tarball = format!("{binary_name}-{latest_tag}-{target}.tar.gz");
    let url = format!(
        "https://github.com/{REPO}/releases/download/{latest_tag}/{tarball}"
    );

    info(&format!("Downloading {tarball}..."));

    // Download to temp dir
    let tmp_dir = tempdir()?;
    let tar_path = tmp_dir.join(&tarball);

    let status = Command::new("curl")
        .args(["-fsSL", "-o", tar_path.to_str().unwrap(), &url])
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run curl: {e}"))?;

    if !status.success() {
        anyhow::bail!(
            "Download failed. Binary may not be available for {target}.\n\
             You can build from source: cargo install --git https://github.com/{REPO} {binary_name}"
        );
    }

    // Extract
    let status = Command::new("tar")
        .args(["-xzf", tar_path.to_str().unwrap(), "-C", tmp_dir.to_str().unwrap()])
        .status()?;

    if !status.success() {
        anyhow::bail!("Failed to extract tarball");
    }

    let extracted = tmp_dir.join(binary_name);
    if !extracted.exists() {
        anyhow::bail!("Binary not found in tarball: {binary_name}");
    }

    // Replace binary
    let install_path = PathBuf::from(INSTALL_DIR).join(binary_name);
    info(&format!("Replacing {}...", install_path.display()));

    // Try direct copy first, fall back to sudo
    if copy_with_fallback(&extracted, &install_path)? {
        set_executable(&install_path)?;
        ok(&format!("Upgraded: {current_tag} → {latest_tag}"));
    }

    println!();
    if binary_name.contains("server") {
        println!("  ▸ Restart the service to use the new version:");
        println!("    zo-tunnel-server restart");
    } else {
        println!("  ▸ Reconnect the client to use the new version.");
    }
    println!();

    // Cleanup
    let _ = std::fs::remove_dir_all(&tmp_dir);

    Ok(())
}

// ─── Uninstall ───────────────────────────────────────────────────

/// Component type for uninstall behavior.
pub enum Component {
    Server,
    Client,
}

/// Uninstall the binary and associated files.
///
/// * `binary_name` — e.g. "zo-tunnel-server" or "zo-tunnel-client"
/// * `component` — Server or Client (determines extra cleanup)
/// * `yes` — skip confirmation prompt
/// * `keep_config` — if true, preserve config files (server only)
pub fn uninstall(
    binary_name: &str,
    component: Component,
    yes: bool,
    keep_config: bool,
) -> anyhow::Result<()> {
    let install_path = PathBuf::from(INSTALL_DIR).join(binary_name);

    // Show what will be removed
    println!();
    eprintln!("{YELLOW}⚠️  This will:{NC}");

    match component {
        Component::Server => {
            println!("  • Stop and disable the zo-tunnel systemd service");
            println!("  • Remove {}", install_path.display());
            if !keep_config {
                println!("  • Remove config directory /etc/zo-tunnel/");
            }
            println!("  • Remove systemd service file");
        }
        Component::Client => {
            println!("  • Remove {}", install_path.display());
        }
    }
    println!();

    if !yes {
        eprint!("  Continue? (y/N): ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("  Cancelled.");
            return Ok(());
        }
    }

    // Server: stop and remove systemd service
    if matches!(component, Component::Server) {
        info("Stopping zo-tunnel service...");
        let _ = Command::new("sudo")
            .args(["systemctl", "stop", "zo-tunnel"])
            .status();
        let _ = Command::new("sudo")
            .args(["systemctl", "disable", "zo-tunnel"])
            .status();

        info("Removing systemd service...");
        let service_path = Path::new("/etc/systemd/system/zo-tunnel.service");
        if service_path.exists() {
            remove_with_sudo(service_path)?;
            let _ = Command::new("sudo")
                .args(["systemctl", "daemon-reload"])
                .status();
        }
        ok("Systemd service removed");
    }

    // Remove binary
    info(&format!("Removing {}...", install_path.display()));
    if install_path.exists() {
        remove_with_sudo(&install_path)?;
        ok("Binary removed");
    } else {
        warn(&format!("Binary not found at {}", install_path.display()));
    }

    // Server: remove config
    if matches!(component, Component::Server) && !keep_config {
        let config_dirs = ["/etc/zo-tunnel"];
        for dir in &config_dirs {
            let p = Path::new(dir);
            if p.exists() {
                info(&format!("Removing {}...", p.display()));
                remove_dir_with_sudo(p)?;
                ok(&format!("{dir} removed"));
            }
        }
    }

    println!();
    ok(&format!("Zo Tunnel {binary_name} has been uninstalled."));
    println!();

    Ok(())
}

// ─── Systemd service management ──────────────────────────────────

const SERVICE_NAME: &str = "zo-tunnel";
const SERVICE_PATH: &str = "/etc/systemd/system/zo-tunnel.service";

/// Generate the systemd unit file content.
fn systemd_unit_content(binary_path: &str) -> String {
    format!(
        "[Unit]\n\
         Description=Zo Tunnel Server\n\
         After=network.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={binary_path} start --foreground\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         StandardOutput=journal\n\
         StandardError=journal\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n"
    )
}

/// Install and enable the systemd service for zo-tunnel-server.
/// Creates the service file and runs `systemctl daemon-reload && enable`.
pub fn install_systemd_service() -> anyhow::Result<()> {
    let binary_path = format!("{}/zo-tunnel-server", INSTALL_DIR);

    // Also accept the binary at the current executable path
    let exec_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or(binary_path);

    let unit = systemd_unit_content(&exec_path);

    // Write service file (needs root)
    let service_path = Path::new(SERVICE_PATH);
    match std::fs::write(service_path, &unit) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            info("Need elevated privileges to install service...");
            let status = Command::new("sudo")
                .args(["tee", SERVICE_PATH])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .spawn()
                .and_then(|mut child| {
                    use std::io::Write;
                    if let Some(ref mut stdin) = child.stdin {
                        stdin.write_all(unit.as_bytes())?;
                    }
                    child.wait()
                })?;
            if !status.success() {
                anyhow::bail!("Failed to write systemd service file");
            }
        }
        Err(e) => return Err(e.into()),
    }

    // Reload systemd
    let _ = Command::new("sudo")
        .args(["systemctl", "daemon-reload"])
        .status();

    // Enable service
    let _ = Command::new("sudo")
        .args(["systemctl", "enable", SERVICE_NAME])
        .status();

    ok("Systemd service installed and enabled");
    Ok(())
}

/// Start the zo-tunnel systemd service.
pub fn start_service() -> anyhow::Result<()> {
    let status = Command::new("sudo")
        .args(["systemctl", "start", SERVICE_NAME])
        .status()?;
    if !status.success() {
        anyhow::bail!("Failed to start {SERVICE_NAME} service");
    }
    ok("Service started");
    Ok(())
}

/// Stop the zo-tunnel systemd service.
pub fn stop_service() -> anyhow::Result<()> {
    let status = Command::new("sudo")
        .args(["systemctl", "stop", SERVICE_NAME])
        .status()?;
    if !status.success() {
        anyhow::bail!("Failed to stop {SERVICE_NAME} service");
    }
    ok("Service stopped");
    Ok(())
}

/// Restart the zo-tunnel systemd service.
pub fn restart_service() -> anyhow::Result<()> {
    let status = Command::new("sudo")
        .args(["systemctl", "restart", SERVICE_NAME])
        .status()?;
    if !status.success() {
        anyhow::bail!("Failed to restart {SERVICE_NAME} service");
    }
    ok("Service restarted");
    Ok(())
}

/// Check if the zo-tunnel systemd service is active.
pub fn is_service_active() -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", SERVICE_NAME])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check if the systemd service file exists.
pub fn is_service_installed() -> bool {
    Path::new(SERVICE_PATH).exists()
}

// ─── File helpers ────────────────────────────────────────────────

/// Create a temporary directory inside /tmp.
fn tempdir() -> anyhow::Result<PathBuf> {
    let name = format!(
        "zo-tunnel-upgrade-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let path = std::env::temp_dir().join(name);
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

/// Copy file, falling back to sudo if permission denied.
/// Handles "Text file busy" (ETXTBSY) by removing the old file first —
/// the running process keeps its inode reference, so unlink is safe.
fn copy_with_fallback(src: &Path, dst: &Path) -> anyhow::Result<bool> {
    // First, try to remove the old file to avoid ETXTBSY when replacing
    // a currently-executing binary. The OS keeps the inode alive until
    // the running process exits, so this is safe.
    if dst.exists() {
        match std::fs::remove_file(dst) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                let _ = Command::new("sudo")
                    .args(["rm", "-f", dst.to_str().unwrap()])
                    .status();
            }
            Err(_) => {} // ignore other errors, copy will fail with a clear message
        }
    }

    match std::fs::copy(src, dst) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            info("Need elevated privileges to install...");
            let status = Command::new("sudo")
                .args(["cp", src.to_str().unwrap(), dst.to_str().unwrap()])
                .status()?;
            if !status.success() {
                anyhow::bail!("Failed to copy binary with sudo");
            }
            Ok(true)
        }
        Err(e) => Err(e.into()),
    }
}

/// chmod +x, falling back to sudo.
fn set_executable(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)) {
        Ok(_) => Ok(()),
        Err(_) => {
            let _ = Command::new("sudo")
                .args(["chmod", "+x", path.to_str().unwrap()])
                .status();
            Ok(())
        }
    }
}

/// Remove a file, falling back to sudo.
fn remove_with_sudo(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            let status = Command::new("sudo")
                .args(["rm", "-f", path.to_str().unwrap()])
                .status()?;
            if !status.success() {
                anyhow::bail!("Failed to remove {} with sudo", path.display());
            }
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Remove a directory recursively, falling back to sudo.
fn remove_dir_with_sudo(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            let status = Command::new("sudo")
                .args(["rm", "-rf", path.to_str().unwrap()])
                .status()?;
            if !status.success() {
                anyhow::bail!("Failed to remove {} with sudo", path.display());
            }
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

// ─── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer() {
        assert!(is_newer("0.4.1", "v0.5.0"));
        assert!(is_newer("0.4.1", "v0.4.2"));
        assert!(is_newer("0.4.1", "v1.0.0"));
        assert!(!is_newer("0.4.1", "v0.4.1"));
        assert!(!is_newer("0.5.0", "v0.4.1"));
        assert!(!is_newer("1.0.0", "v0.9.9"));
    }

    #[test]
    fn test_is_newer_with_v_prefix() {
        assert!(is_newer("v0.4.1", "v0.5.0"));
        assert!(!is_newer("v0.5.0", "v0.4.1"));
        assert!(!is_newer("v0.4.1", "v0.4.1"));
    }

    #[test]
    fn test_detect_target() {
        // Should not fail on the current platform
        let target = detect_target();
        assert!(target.is_ok());
        let t = target.unwrap();
        assert!(
            t == "linux-amd64"
                || t == "linux-arm64"
                || t == "darwin-amd64"
                || t == "darwin-arm64"
        );
    }
}
