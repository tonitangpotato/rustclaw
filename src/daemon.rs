//! Daemon management for RustClaw on macOS (launchd)
//!
//! Handles starting, stopping, and monitoring the RustClaw agent as a system service.

use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

const SERVICE_LABEL: &str = "com.rustclaw.agent";
const PLIST_FILENAME: &str = "com.rustclaw.agent.plist";

/// ANSI color codes for terminal output
mod colors {
    pub const GREEN: &str = "\x1b[32m";
    pub const RED: &str = "\x1b[31m";
    pub const BLUE: &str = "\x1b[34m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BOLD: &str = "\x1b[1m";
    pub const RESET: &str = "\x1b[0m";
}

/// Get the path to the LaunchAgents directory
fn launch_agents_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    Ok(PathBuf::from(home).join("Library/LaunchAgents"))
}

/// Get the path to the plist file
fn plist_path() -> Result<PathBuf> {
    Ok(launch_agents_dir()?.join(PLIST_FILENAME))
}

/// Get the path to the log directory
fn log_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    Ok(PathBuf::from(home).join(".rustclaw/logs"))
}

/// Get the stdout log path
fn stdout_log_path() -> Result<PathBuf> {
    Ok(log_dir()?.join("rustclaw.log"))
}

/// Resolve a path to its absolute form
fn resolve_absolute_path(path: &str) -> Result<PathBuf> {
    let path = Path::new(path);
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        let cwd = std::env::current_dir().context("Failed to get current directory")?;
        Ok(cwd.join(path).canonicalize().context("Failed to resolve absolute path")?)
    }
}

/// Get the absolute path to the rustclaw binary
fn binary_path() -> Result<PathBuf> {
    std::env::current_exe().context("Failed to get current executable path")
}

/// Generate the launchd plist XML content
fn generate_plist(binary: &Path, config: &Path, workspace: &Path) -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/potato".to_string());
    let log_dir = format!("{}/.rustclaw/logs", home);

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>run</string>
        <string>--config</string>
        <string>{}</string>
        <string>--workspace</string>
        <string>{}</string>
    </array>
    <key>WorkingDirectory</key>
    <string>{}</string>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{}/rustclaw.log</string>
    <key>StandardErrorPath</key>
    <string>{}/rustclaw.err</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>HOME</key>
        <string>{}</string>
        <key>PATH</key>
        <string>/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
</dict>
</plist>
"#,
        SERVICE_LABEL,
        binary.display(),
        config.display(),
        workspace.display(),
        workspace.display(),
        log_dir,
        log_dir,
        home,
    )
}

/// Check if the daemon is currently running
fn is_running() -> Result<Option<i32>> {
    // Use `launchctl list | grep <label>` for simpler parsing
    // Output format: "PID\tStatus\tLabel" or "-\tStatus\tLabel"
    let output = Command::new("sh")
        .args(["-c", &format!("launchctl list | grep {}", SERVICE_LABEL)])
        .output()
        .context("Failed to run launchctl list")?;

    if !output.status.success() || output.stdout.is_empty() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split('\t').collect();

    if parts.is_empty() {
        return Ok(None);
    }

    // First field is PID or "-" if not running
    match parts[0].trim().parse::<i32>() {
        Ok(pid) if pid > 0 => Ok(Some(pid)),
        _ => Ok(None),
    }
}

/// Read config and workspace from existing plist
fn read_plist_config() -> Result<Option<(String, String)>> {
    let plist = plist_path()?;
    if !plist.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&plist)?;

    // Parse ProgramArguments to find config and workspace
    let mut config = None;
    let mut workspace = None;
    let mut next_is_config = false;
    let mut next_is_workspace = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.contains("--config") {
            next_is_config = true;
        } else if trimmed.contains("--workspace") {
            next_is_workspace = true;
        } else if next_is_config && trimmed.starts_with("<string>") {
            config = Some(
                trimmed
                    .replace("<string>", "")
                    .replace("</string>", "")
                    .to_string(),
            );
            next_is_config = false;
        } else if next_is_workspace && trimmed.starts_with("<string>") {
            workspace = Some(
                trimmed
                    .replace("<string>", "")
                    .replace("</string>", "")
                    .to_string(),
            );
            next_is_workspace = false;
        }
    }

    match (config, workspace) {
        (Some(c), Some(w)) => Ok(Some((c, w))),
        _ => Ok(None),
    }
}

/// Get uptime from log file modification time
fn get_uptime() -> Result<Option<String>> {
    let log_path = stdout_log_path()?;
    if !log_path.exists() {
        return Ok(None);
    }

    let metadata = fs::metadata(&log_path)?;
    let modified = metadata
        .modified()
        .context("Failed to get log modification time")?;
    let now = std::time::SystemTime::now();

    if let Ok(duration) = now.duration_since(modified) {
        let secs = duration.as_secs();
        if secs < 60 {
            Ok(Some(format!("{}s", secs)))
        } else if secs < 3600 {
            Ok(Some(format!("{}m", secs / 60)))
        } else if secs < 86400 {
            let hours = secs / 3600;
            let mins = (secs % 3600) / 60;
            Ok(Some(format!("{}h {}m", hours, mins)))
        } else {
            let days = secs / 86400;
            let hours = (secs % 86400) / 3600;
            Ok(Some(format!("{}d {}h", days, hours)))
        }
    } else {
        Ok(None)
    }
}

/// Print a styled status box
fn print_status_box(
    running: bool,
    pid: Option<i32>,
    config: Option<&str>,
    workspace: Option<&str>,
    uptime: Option<&str>,
) {
    use colors::*;

    println!();
    println!("{}{}RustClaw Daemon Status{}", BOLD, BLUE, RESET);
    println!("═══════════════════════════════════════════════════════");

    if running {
        println!(
            "  {}Status:{}    {}✅ Running{}",
            BOLD, RESET, GREEN, RESET
        );
        if let Some(p) = pid {
            println!("  {}PID:{}       {}", BOLD, RESET, p);
        }
    } else {
        println!(
            "  {}Status:{}    {}❌ Stopped{}",
            BOLD, RESET, RED, RESET
        );
    }

    if let Some(c) = config {
        println!("  {}Config:{}    {}", BOLD, RESET, c);
    }
    if let Some(w) = workspace {
        println!("  {}Workspace:{} {}", BOLD, RESET, w);
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| "~".to_string());
    println!(
        "  {}Log:{}       {}/.rustclaw/logs/rustclaw.log",
        BOLD, RESET, home
    );

    if running {
        if let Some(u) = uptime {
            println!("  {}Active:{}    ~{}", BOLD, RESET, u);
        }
    }

    println!("═══════════════════════════════════════════════════════");
    println!();
}

/// Print last N lines of log
fn print_recent_logs(lines: usize) -> Result<()> {
    let log_path = stdout_log_path()?;
    if !log_path.exists() {
        println!(
            "{}No logs found at {}{}",
            colors::YELLOW,
            log_path.display(),
            colors::RESET
        );
        return Ok(());
    }

    let content = fs::read_to_string(&log_path)?;
    let all_lines: Vec<&str> = content.lines().collect();
    let start = all_lines.len().saturating_sub(lines);

    println!(
        "{}{}Recent logs ({} lines):{}",
        colors::BOLD,
        colors::BLUE,
        lines.min(all_lines.len()),
        colors::RESET
    );
    println!("───────────────────────────────────────────────────────");

    for line in &all_lines[start..] {
        println!("{}", line);
    }

    println!("───────────────────────────────────────────────────────");
    Ok(())
}

/// Start the daemon
pub fn daemon_start(config_path: &str, workspace: Option<&str>) -> Result<()> {
    use colors::*;

    // Check if already running
    if let Some(pid) = is_running()? {
        println!(
            "{}ℹ️  Daemon is already running (PID: {}){}",
            BLUE, pid, RESET
        );
        daemon_status()?;
        return Ok(());
    }

    println!("{}Starting RustClaw daemon...{}", BLUE, RESET);

    // Resolve paths
    let binary = binary_path()?;
    let config = resolve_absolute_path(config_path)?;

    // For workspace, use provided path, or directory containing config, or current dir
    let workspace_path = match workspace {
        Some(w) => resolve_absolute_path(w)?,
        None => config
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
    };

    // Ensure config file exists
    if !config.exists() {
        return Err(anyhow!(
            "Config file not found: {}",
            config.display()
        ));
    }

    // Create log directory
    let logs = log_dir()?;
    fs::create_dir_all(&logs).context("Failed to create log directory")?;

    // Ensure LaunchAgents directory exists
    let agents_dir = launch_agents_dir()?;
    fs::create_dir_all(&agents_dir).context("Failed to create LaunchAgents directory")?;

    // Generate and write plist
    let plist_content = generate_plist(&binary, &config, &workspace_path);
    let plist = plist_path()?;
    fs::write(&plist, plist_content).context("Failed to write plist file")?;

    println!("  {}✓{} Plist written to {}", GREEN, RESET, plist.display());

    // Load the service
    let output = Command::new("launchctl")
        .args(["load", plist.to_str().unwrap()])
        .output()
        .context("Failed to run launchctl load")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to load service: {}", stderr));
    }

    println!("  {}✓{} Service loaded", GREEN, RESET);

    // Wait and verify
    println!("  {}...{} Waiting for process to start", YELLOW, RESET);
    thread::sleep(Duration::from_secs(2));

    if let Some(pid) = is_running()? {
        println!(
            "\n{}✅ RustClaw daemon started successfully (PID: {}){}",
            GREEN, pid, RESET
        );
        daemon_status()?;
    } else {
        println!(
            "\n{}⚠️  Service loaded but process not running. Check logs:{}",
            YELLOW, RESET
        );
        print_recent_logs(10)?;
    }

    Ok(())
}

/// Stop the daemon
pub fn daemon_stop() -> Result<()> {
    use colors::*;

    let plist = plist_path()?;

    if !plist.exists() {
        println!(
            "{}ℹ️  No daemon service installed (plist not found){}",
            BLUE, RESET
        );
        return Ok(());
    }

    // Check if running
    let was_running = is_running()?.is_some();

    println!("{}Stopping RustClaw daemon...{}", BLUE, RESET);

    // Unload the service
    let output = Command::new("launchctl")
        .args(["unload", plist.to_str().unwrap()])
        .output()
        .context("Failed to run launchctl unload")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Ignore "not loaded" errors
        if !stderr.contains("Could not find specified service") {
            return Err(anyhow!("Failed to unload service: {}", stderr));
        }
    }

    // Verify stopped
    thread::sleep(Duration::from_millis(500));

    if is_running()?.is_none() {
        if was_running {
            println!("{}✅ RustClaw daemon stopped{}", GREEN, RESET);
        } else {
            println!(
                "{}ℹ️  Daemon was not running (service unloaded){}",
                BLUE, RESET
            );
        }
    } else {
        println!(
            "{}⚠️  Service unloaded but process still running{}",
            YELLOW, RESET
        );
    }

    Ok(())
}

/// Show daemon status
pub fn daemon_status() -> Result<()> {
    let pid = is_running()?;
    let running = pid.is_some();

    let (config, workspace) = read_plist_config()?.unzip();
    let uptime = if running { get_uptime()? } else { None };

    print_status_box(
        running,
        pid,
        config.as_deref(),
        workspace.as_deref(),
        uptime.as_deref(),
    );

    Ok(())
}

/// Restart the daemon
pub fn daemon_restart(config_path: &str, workspace: Option<&str>) -> Result<()> {
    use colors::*;

    println!("{}Restarting RustClaw daemon...{}", BLUE, RESET);

    // Stop if running (ignore errors)
    let _ = daemon_stop();

    // Small delay
    thread::sleep(Duration::from_millis(500));

    // Start with new config
    daemon_start(config_path, workspace)
}

/// View daemon logs
pub fn daemon_logs(follow: bool, lines: usize) -> Result<()> {
    let log_path = stdout_log_path()?;

    if !log_path.exists() {
        println!(
            "{}No logs found at {}{}",
            colors::YELLOW,
            log_path.display(),
            colors::RESET
        );
        return Ok(());
    }

    if follow {
        // Use tail -f for following
        let status = Command::new("tail")
            .args(["-f", "-n", &lines.to_string(), log_path.to_str().unwrap()])
            .status()
            .context("Failed to run tail")?;

        if !status.success() {
            return Err(anyhow!("tail exited with error"));
        }
    } else {
        print_recent_logs(lines)?;
    }

    Ok(())
}

/// Install daemon service (write plist but don't start)
pub fn daemon_install(config_path: &str, workspace: Option<&str>) -> Result<()> {
    use colors::*;

    let plist = plist_path()?;
    if plist.exists() {
        println!(
            "{}ℹ️  Service already installed at {}{}",
            BLUE,
            plist.display(),
            RESET
        );
        println!("Use '{}rustclaw daemon start{}' to start it.", BOLD, RESET);
        return Ok(());
    }

    println!("{}Installing RustClaw daemon service...{}", BLUE, RESET);

    // Resolve paths
    let binary = binary_path()?;
    let config = resolve_absolute_path(config_path)?;

    let workspace_path = match workspace {
        Some(w) => resolve_absolute_path(w)?,
        None => config
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
    };

    // Ensure config file exists
    if !config.exists() {
        return Err(anyhow!("Config file not found: {}", config.display()));
    }

    // Create directories
    let logs = log_dir()?;
    fs::create_dir_all(&logs)?;

    let agents_dir = launch_agents_dir()?;
    fs::create_dir_all(&agents_dir)?;

    // Generate and write plist
    let plist_content = generate_plist(&binary, &config, &workspace_path);
    fs::write(&plist, plist_content)?;

    println!("{}✅ Service installed at {}{}", GREEN, plist.display(), RESET);
    println!();
    println!("To start the daemon:");
    println!("  {}rustclaw daemon start{}", BOLD, RESET);
    println!();
    println!("The service will auto-start on login (RunAtLoad: true).");

    Ok(())
}

/// Uninstall daemon service
pub fn daemon_uninstall() -> Result<()> {
    use colors::*;

    // Stop first if running
    if is_running()?.is_some() {
        println!("{}Stopping daemon before uninstall...{}", BLUE, RESET);
        daemon_stop()?;
    }

    let plist = plist_path()?;
    if !plist.exists() {
        println!(
            "{}ℹ️  No daemon service installed (plist not found){}",
            BLUE, RESET
        );
        return Ok(());
    }

    // Make sure it's unloaded
    let _ = Command::new("launchctl")
        .args(["unload", plist.to_str().unwrap()])
        .output();

    // Delete plist
    fs::remove_file(&plist).context("Failed to delete plist file")?;

    println!(
        "{}✅ RustClaw daemon service uninstalled{}",
        GREEN, RESET
    );
    println!();
    println!(
        "{}Note:{} Log files are preserved at ~/.rustclaw/logs/",
        YELLOW, RESET
    );

    Ok(())
}
