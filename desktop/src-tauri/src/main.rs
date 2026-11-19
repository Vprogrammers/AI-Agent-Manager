#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{path::{Path, PathBuf}, process::Command, sync::{Arc,Mutex}};
use std::collections::HashMap;
use tauri::Manager;
mod project_architect;

fn configure_agent_path() {
    // Finder/Dock launches often lack the user's interactive shell PATH.
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        for path in [home.join(".local/bin"), home.join(".npm-global/bin"), home.join(".bun/bin"), home.join(".cargo/bin")] {
            if path.is_dir() && !dirs.contains(&path) { dirs.push(path); }
        }
    }
    #[cfg(target_os = "macos")]
    for path in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        let path = PathBuf::from(path);
        if path.is_dir() && !dirs.contains(&path) { dirs.push(path); }
    }
    if let Ok(joined) = std::env::join_paths(dirs) { std::env::set_var("PATH", joined); }
}

fn aam_executable() -> Result<PathBuf, String> {
    // Tauri installs the sidecar next to the desktop executable.
    // In development, continue to support the local launcher.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let sidecar = parent.join(if cfg!(windows) { "aam-engine.exe" } else { "aam-engine" });
            if sidecar.is_file() { return Ok(sidecar); }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let local = PathBuf::from(home).join(".local/bin/aam");
        if local.is_file() { return Ok(local); }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(if cfg!(windows) { "aam.exe" } else { "aam" });
            if candidate.is_file() { return Ok(candidate); }
        }
    }
    Err("Bundled AAM engine not found. For development, run scripts/install.sh; for release, rebuild the installer.".into())
}

use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, BufReader};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GithubRepo { name_with_owner: String, is_private: bool, description: Option<String> }

#[tauri::command]
fn github_repositories() -> Result<Vec<GithubRepo>, String> {
    let result = Command::new("gh").args(["repo", "list", "--limit", "100", "--json", "nameWithOwner,isPrivate,description"]).output()
        .map_err(|_| "GitHub CLI not found. Install gh and run gh auth login.".to_string())?;
    if !result.status.success() { return Err(format!("GitHub CLI: {}", String::from_utf8_lossy(&result.stderr))); }
    serde_json::from_slice(&result.stdout).map_err(|e| format!("Invalid GitHub response: {e}"))
}

#[tauri::command]
fn github_clone(repository: String, destination: String) -> Result<String, String> {
    let parts: Vec<&str> = repository.split('/').collect();
    if parts.len() != 2 || parts.iter().any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') || *part == "." || *part == "..") {
        return Err("Invalid repository name".into());
    }
    let parent = Path::new(&destination).canonicalize().map_err(|e| format!("Destination: {e}"))?;
    if !parent.is_dir() { return Err("Choose an existing destination folder".into()); }
    let target = parent.join(parts[1]);
    if target.exists() { return Err(format!("Destination already exists: {}. Open it using Open Folder.", target.display())); }
    let output = Command::new("gh").args(["repo", "clone", &repository, "--"]).arg(&target).output()
        .map_err(|_| "GitHub CLI not found. Install gh and run gh auth login.".to_string())?;
    if !output.status.success() { return Err(format!("Clone failed: {}", String::from_utf8_lossy(&output.stderr))); }
    Ok(target.to_string_lossy().into_owned())
}



#[derive(Default)]
struct RunningRequests(Mutex<HashMap<String, Arc<tokio::sync::Notify>>>);

#[derive(Serialize)]
struct Diagnostic { name: String, status: String, detail: String }

#[tauri::command]
fn desktop_diagnostics(root: String) -> Vec<Diagnostic> {
    let mut result = Vec::new();
    let engine = aam_executable();
    result.push(Diagnostic {name:"AAM engine".into(),status:if engine.is_ok() {"ready"}else{"missing"}.into(),detail:engine.map(|p|p.display().to_string()).unwrap_or_else(|e|e)});
    for (name, binary) in [("Git","git"),("Claude Code","claude"),("Codex","codex"),("GitHub CLI (optional)","gh")] {
        let found = std::env::var_os("PATH").and_then(|paths|std::env::split_paths(&paths).map(|p|p.join(binary)).find(|p|p.is_file()));
        result.push(Diagnostic {name:name.into(),status:if found.is_some(){"ready"}else{"missing"}.into(),detail:found.map(|p|p.display().to_string()).unwrap_or_else(||format!("{binary} not found in PATH"))});
    }
    if !root.is_empty() {
        let valid = Path::new(&root).is_dir() && Command::new("git").args(["-C",&root,"rev-parse","--is-inside-work-tree"]).output().map(|o|o.status.success()).unwrap_or(false);
        result.push(Diagnostic{name:"Git workspace".into(),status:if valid{"ready"}else{"missing"}.into(),detail:if valid{"Valid Git workspace"}else{"Select a valid Git repository"}.into()});
    }
    result
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeSetup {
    installed: bool,
    detail: String,
    installer: String,
}

#[tauri::command]
async fn node_setup_status() -> NodeSetup {
    let node = tokio::time::timeout(std::time::Duration::from_secs(5),
        tokio::process::Command::new("node").arg("--version")
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status()).await;
    let npm_bin = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let npm = tokio::time::timeout(std::time::Duration::from_secs(5),
        tokio::process::Command::new(npm_bin).arg("--version")
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status()).await;
    let installed = matches!(node, Ok(Ok(ref s)) if s.success()) &&
        matches!(npm, Ok(Ok(ref s)) if s.success());
    let installer = if cfg!(windows) { "winget (OpenJS.NodeJS.LTS)" }
        else if cfg!(target_os = "macos") { "Homebrew (node)" }
        else { "apt/dnf/pacman via polkit (supported Linux distributions)" };
    NodeSetup { installed, detail: if installed { "Node.js and npm available".into() }
        else { "Node.js or npm missing from application PATH".into() }, installer: installer.into() }
}

#[tauri::command]
async fn install_node_runtime() -> Result<String, String> {
    // Only invoke fixed, reviewed package-manager arguments after UI confirmation.
    // Never interpolate user input or download and execute remote scripts.
    let mut command;
    if cfg!(windows) {
        command = tokio::process::Command::new("winget");
        command.args(["install", "--id", "OpenJS.NodeJS.LTS", "--exact",
            "--source", "winget", "--accept-package-agreements", "--accept-source-agreements"]);
    } else if cfg!(target_os = "macos") {
        command = tokio::process::Command::new("brew");
        command.args(["install", "node"]);
    } else if cfg!(target_os = "linux") {
        // pkexec shows an OS-controlled authorization dialog when elevation is needed.
        if std::path::Path::new("/usr/bin/apt-get").exists() {
            command = tokio::process::Command::new("pkexec");
            command.args(["/usr/bin/apt-get", "install", "-y", "nodejs", "npm"]);
        } else if std::path::Path::new("/usr/bin/dnf").exists() {
            command = tokio::process::Command::new("pkexec");
            command.args(["/usr/bin/dnf", "install", "-y", "nodejs", "npm"]);
        } else if std::path::Path::new("/usr/bin/pacman").exists() {
            command = tokio::process::Command::new("pkexec");
            command.args(["/usr/bin/pacman", "-S", "--noconfirm", "nodejs", "npm"]);
        } else {
            return Err("No supported Linux package manager found (apt, dnf, pacman).".into());
        }
    } else {
        return Err("Unsupported operating system".into());
    }
    command.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null()).kill_on_drop(true);
    let status = tokio::time::timeout(std::time::Duration::from_secs(600), command.status())
        .await.map_err(|_| "Node.js installation timed out".to_string())?
        .map_err(|_| "Package manager unavailable. On macOS install Homebrew; on Windows install winget; on Linux install polkit.".to_string())?;
    if !status.success() { return Err("Node.js installation failed or authorization was declined. Check package manager and network access.".into()); }
    Ok("Node.js installer completed. Restart the app if PATH changed, then recheck.".into())
}

// Installation is initiated only by an explicit user click. No shell interpolation,
// elevated privileges, or credentials are handled by the application.
#[tauri::command]
async fn install_provider_cli(provider: String) -> Result<String, String> {
    let package = match provider.as_str() {
        "claude" => "@anthropic-ai/claude-code",
        "codex" => "@openai/codex",
        "gemini" => "@google/gemini-cli",
        "opencode" => "@opencode/cli",
        "copilot" => "@github/copilot",
        _ => return Err("Unsupported provider".into()),
    };
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let mut child = tokio::process::Command::new(npm);
    child.args(["install", "--global", package, "--no-audit", "--no-fund"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(180), child.output())
        .await.map_err(|_| "Installation timed out (3 minutes)".to_string())?
        .map_err(|_| "Node.js/npm is not available. Install Node.js LTS first.".to_string())?;
    if !output.status.success() {
        // Never return npm stderr: it may contain registry tokens or private paths.
        return Err("CLI installation failed. Check npm permissions and registry connectivity.".into());
    }
    Ok(format!("{provider} CLI installation completed. Recheck status; restart the app if PATH changed."))
}

// Interactive provider login runs in a real PTY embedded in the setup UI.
// OAuth browser redirects are left to the provider CLI and system browser:
// third-party identity providers may prohibit embedded webviews.
type LoginWriter = Box<dyn std::io::Write + Send>;
#[derive(Default)]
struct LoginSession(Mutex<Option<LoginWriter>>);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LoginOutput { text: String, finished: bool }

#[tauri::command]
fn focus_after_login(app: tauri::AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("Main window unavailable")?;
    if window.is_minimized().unwrap_or(false) {
        window.unminimize().map_err(|e| e.to_string())?;
    }
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

#[tauri::command]
fn login_pty_write(input: String, state: tauri::State<'_, LoginSession>) -> Result<(), String> {
    if input.len() > 4096 { return Err("Input too long".into()); }
    let mut session = state.0.lock().map_err(|_| "Login session unavailable")?;
    let writer = session.as_mut().ok_or("No active login session")?;
    use std::io::Write;
    writer.write_all(input.as_bytes()).map_err(|_| "Unable to send login input".to_string())?;
    writer.flush().map_err(|_| "Unable to flush login input".to_string())
}

#[tauri::command]
fn open_provider_login(provider: String, app: tauri::AppHandle, state: tauri::State<'_, LoginSession>) -> Result<String, String> {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    let (binary, args): (&str, &[&str]) = match provider.as_str() {
        "claude" => ("claude", &["auth", "login"]),
        "codex" => ("codex", &["login"]),
        "gemini" => ("gemini", &[]),
        "opencode" => ("opencode", &["auth", "login"]),
        "copilot" => ("copilot", &["login"]),
        _ => return Err("Unsupported provider".into()),
    };
    let executable = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).flat_map(|p| {
            let mut candidates = vec![p.join(binary)];
            if cfg!(windows) {
                for ext in ["exe", "cmd", "bat"] { candidates.push(p.join(format!("{binary}.{ext}"))); }
            }
            candidates
        }).find(|p| p.is_file())
    }).ok_or("Provider CLI not found. Recheck installation first.")?;
    let pty = native_pty_system().openpty(PtySize {
        rows: 24, cols: 100, pixel_width: 0, pixel_height: 0
    }).map_err(|_| "Could not create interactive login terminal".to_string())?;
    let mut cmd = CommandBuilder::new(executable);
    for arg in args { cmd.arg(arg); }
    let mut child = pty.slave.spawn_command(cmd).map_err(|_| "Could not launch provider login".to_string())?;
    drop(pty.slave);
    let writer = pty.master.take_writer().map_err(|_| "Could not open login input".to_string())?;
    let mut reader = pty.master.try_clone_reader().map_err(|_| "Could not open login output".to_string())?;
    {
        let mut guard = state.0.lock().map_err(|_| "Login session unavailable")?;
        if guard.is_some() { let _ = child.kill(); return Err("A login is already in progress".into()); }
        *guard = Some(writer);
    }
    let state_handle = app.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 2048];
        loop {
            match std::io::Read::read(&mut reader, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    // Do not persist PTY output; avoid exposing credentials in logs.
                    let _ = app.emit("provider-login-output", LoginOutput {
                        text: String::from_utf8_lossy(&buf[..n]).into_owned(), finished: false
                    });
                }
            }
        }
        let _ = child.wait();
        if let Some(session) = state_handle.try_state::<LoginSession>() {
            if let Ok(mut guard) = session.0.lock() { *guard = None; }
        }
        let _ = app.emit("provider-login-output", LoginOutput { text: String::new(), finished: true });
    });
    Ok("Interactive login started inside the application. Complete provider browser authorization if prompted.".into())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderSetup {
    id: String,
    installed: bool,
    authenticated: bool,
    auth_status: String,
    detail: String,
}

#[tauri::command]
async fn provider_setup_status() -> Vec<ProviderSetup> {
    let mut result = Vec::new();
    for (id, binary, args) in [
        ("claude", "claude", vec!["auth", "status"]),
        ("codex", "codex", vec!["login", "status"]),
        ("gemini", "gemini", vec!["--version"]),
        ("opencode", "opencode", vec!["auth", "list"]),
        ("copilot", "copilot", vec!["--version"]),
    ] {
        let executable = std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths).flat_map(|p| {
                let mut candidates = vec![p.join(binary)];
                if cfg!(windows) {
                    for ext in ["exe", "cmd", "bat"] {
                        candidates.push(p.join(format!("{binary}.{ext}")));
                    }
                }
                candidates
            }).find(|p| p.is_file())
        });
        let Some(executable) = executable else {
            result.push(ProviderSetup { id: id.into(), installed: false, authenticated: false,
                auth_status: "missing".into(), detail: "CLI not found in PATH".into() });
            continue;
        };
        // Never return stdout/stderr: authentication diagnostics may contain private data.
        let status = tokio::time::timeout(
            std::time::Duration::from_secs(8),
            tokio::process::Command::new(&executable)
                .args(&args).stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null()).kill_on_drop(true).status(),
        ).await;
        // Only Claude and Codex expose the verified auth-status probes used here.
        // Other CLI exit codes are not evidence of an authenticated account.
        let (authenticated, auth_status, detail) = if ["gemini", "opencode", "copilot"].contains(&id) {
            match status {
                Ok(Ok(exit)) if exit.success() => (false, "unknown", "CLI detected; authentication must be verified in the provider"),
                Ok(Ok(_)) => (false, "unknown", "CLI detected; login status not independently verified"),
                Ok(Err(_)) => (false, "unknown", "Could not execute CLI"),
                Err(_) => (false, "unknown", "CLI probe timed out"),
            }
        } else {
            match status {
                Ok(Ok(exit)) if exit.success() => (true, "ready", "CLI authentication confirmed"),
                Ok(Ok(_)) => (false, "login_required", "CLI reports no active authentication or cannot verify it"),
                Ok(Err(_)) => (false, "unknown", "Could not execute CLI authentication check"),
                Err(_) => (false, "unknown", "Authentication check timed out"),
            }
        };
        result.push(ProviderSetup { id: id.into(), installed: true, authenticated,
            auth_status: auth_status.into(), detail: detail.into() });
    }
    result
}

#[tauri::command]
fn cancel_prompt(state: tauri::State<'_, RunningRequests>, request_id: String) -> Result<(), String> {
    let requests = state.0.lock().map_err(|_|"Request state unavailable")?;
    let signal = requests.get(&request_id).ok_or("Request is no longer running")?;
    signal.notify_one();
    Ok(())
}

// Only Linux bubblewrap is supported for third-party CLI execution.
// Never fall back to an unsandboxed subprocess when the sandbox is unavailable.
// Single execution contract for all five providers. The legacy AAM engine
// remains the transport for Claude/Codex until its event protocol is migrated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExecutionAdapter { Engine, SandboxedCli }

#[derive(Clone, Copy, Debug)]
struct ProviderAdapter {
    id: &'static str,
    transport: ExecutionAdapter,
}

const PROVIDER_ADAPTERS: [ProviderAdapter; 5] = [
    ProviderAdapter { id: "claude", transport: ExecutionAdapter::Engine },
    ProviderAdapter { id: "codex", transport: ExecutionAdapter::Engine },
    ProviderAdapter { id: "gemini", transport: ExecutionAdapter::SandboxedCli },
    ProviderAdapter { id: "opencode", transport: ExecutionAdapter::SandboxedCli },
    ProviderAdapter { id: "copilot", transport: ExecutionAdapter::SandboxedCli },
];

fn provider_adapter(id: &str) -> Option<ProviderAdapter> {
    PROVIDER_ADAPTERS.iter().copied().find(|adapter| adapter.id == id)
}

fn validate_execution(adapter: ProviderAdapter, permission: &str) -> Result<(), String> {
    if !["read-only", "workspace-write", "full-access"].contains(&permission) {
        return Err("Invalid permission mode".into());
    }
    if !cli_on_path(adapter.id) { return Err(format!("{} CLI not installed", adapter.id)); }
    if !external_sandbox_ready(permission) {
        return Err("OS sandbox unavailable for requested permission".into());
    }
    match adapter.transport {
        ExecutionAdapter::Engine => {
            aam_executable().map(|_| ()).map_err(|_| "AAM engine unavailable".into())
        },
        ExecutionAdapter::SandboxedCli => external_provider_preflight(adapter.id, permission),
    }
}

#[cfg(test)]
mod adapter_contract_tests {
    use super::{provider_adapter, ExecutionAdapter, PROVIDER_ADAPTERS};
    #[test]
    fn every_provider_has_exactly_one_adapter() {
        let mut ids = std::collections::HashSet::new();
        for adapter in PROVIDER_ADAPTERS {
            assert!(ids.insert(adapter.id));
        }
        assert_eq!(ids.len(), 5);
    }
    #[test]
    fn engine_and_sandbox_transports_are_explicit() {
        assert_eq!(provider_adapter("claude").unwrap().transport, ExecutionAdapter::Engine);
        assert_eq!(provider_adapter("codex").unwrap().transport, ExecutionAdapter::Engine);
        for id in ["gemini", "opencode", "copilot"] {
            assert_eq!(provider_adapter(id).unwrap().transport, ExecutionAdapter::SandboxedCli);
        }
        assert!(provider_adapter("unknown").is_none());
    }
}

fn external_sandbox_ready(permission: &str) -> bool {
    if !["read-only", "workspace-write"].contains(&permission) { return false; }
    #[cfg(target_os = "linux")]
    { return cli_on_path("bwrap"); }
    #[cfg(target_os = "macos")]
    { return cli_on_path("sandbox-exec"); }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    { false }
}

fn external_provider_preflight(provider: &str, permission: &str) -> Result<(), String> {
    if !external_sandbox_ready(permission) {
        return Err("No supported OS sandbox for this permission mode; execution blocked".into());
    }
    if !cli_on_path(provider) { return Err(format!("{provider} CLI not installed")); }
    Ok(())
}

// Escape the workspace for macOS Seatbelt's quoted sandbox profile syntax.
// The profile grants writes only to the selected workspace and ephemeral /tmp.
#[cfg(target_os = "macos")]
fn macos_sandbox_profile(root: &Path, permission: &str) -> Result<String, String> {
    let path = root.to_str().ok_or("Workspace path is not UTF-8")?;
    if path.chars().any(|c| c == '\0' || c == '\n' || c == '\r') {
        return Err("Unsupported workspace path".into());
    }
    let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
    if permission == "read-only" {
        return Ok(r#"(version 1) (deny default) (allow process*) (allow sysctl-read) (allow mach-lookup) (allow network*) (allow file-read*) (allow file-write* (subpath "/private/tmp") (subpath "/tmp") (literal "/dev/null"))"#.to_string());
    }
    if permission != "workspace-write" { return Err("Unsupported sandbox permission".into()); }
    Ok(format!(r#"(version 1) (deny default) (allow process*) (allow sysctl-read) (allow mach-lookup) (allow network*) (allow file-read*) (allow file-write* (subpath "{escaped}") (subpath "/private/tmp") (subpath "/tmp") (literal "/dev/null"))"#))
}

// One platform isolation policy for both engine and direct CLI transports.
// On unsupported platforms, never silently launch an unrestricted child.
fn sandbox_command(executable: &Path, args: &[&str], root: &Path, permission: &str) -> Result<tokio::process::Command, String> {
    #[cfg(target_os = "linux")]
    {
        if !cli_on_path("bwrap") { return Err("bubblewrap unavailable".into()); }
        let mut command = tokio::process::Command::new("bwrap");
        command.args(["--die-with-parent", "--unshare-pid", "--new-session",
            "--ro-bind", "/", "/", "--dev-bind", "/dev", "/dev",
            "--proc", "/proc", "--tmpfs", "/tmp"]);
        if permission == "workspace-write" {
            command.arg("--bind").arg(root).arg(root);
        } else if permission != "read-only" {
            return Err("Unsupported sandbox permission".into());
        }
        command.arg("--chdir").arg(root)
            .arg("--").arg(executable).args(args);
        Ok(command)
    }
    #[cfg(target_os = "macos")]
    {
        if !cli_on_path("sandbox-exec") { return Err("Seatbelt sandbox unavailable".into()); }
        let mut command = tokio::process::Command::new("sandbox-exec");
        command.arg("-p").arg(macos_sandbox_profile(root, permission)?).arg(executable).args(args);
        Ok(command)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (executable, args, root, permission);
        Err("No supported OS sandbox; execution blocked".into())
    }
}

// Shared execution deadline and cancellation contract for every adapter.
async fn await_runtime<T, F>(future: F, stop: Arc<tokio::sync::Notify>) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>>,
{
    tokio::select! {
        outcome = tokio::time::timeout(std::time::Duration::from_secs(600), future) =>
            outcome.map_err(|_| "Request timed out after 10 minutes".to_string())?,
        _ = stop.notified() => Err("Request cancelled".into()),
    }
}

async fn run_external_provider(app: &tauri::AppHandle, request_id: &str, root: &Path, provider: &str, prompt: &str, permission: &str, stop: Arc<tokio::sync::Notify>) -> Result<(), String> {
    let (binary, args): (&str, Vec<&str>) = match provider {
        "gemini" => ("gemini", vec!["-p", prompt, "--output-format", "json"]),
        "opencode" => ("opencode", vec!["run", prompt]),
        "copilot" => ("copilot", vec!["-p", prompt, "-s", "--no-ask-user"]),
        _ => return Err("Unsupported provider".into()),
    };
    external_provider_preflight(provider, permission)?;
    let executable = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).flat_map(|p| {
            let mut candidates = vec![p.join(binary)];
            if cfg!(windows) {
                for ext in ["exe", "cmd", "bat"] { candidates.push(p.join(format!("{binary}.{ext}"))); }
            }
            candidates
        }).find(|p| p.is_file())
    }).ok_or(format!("{provider} CLI not found in PATH"))?;
    let mut command = sandbox_command(&executable, &args, root, permission)?;
    command.current_dir(root).stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    let result = async {
        let child = command.spawn().map_err(|_| format!("PRESTART: Could not start {provider} CLI"))?;
        let output = await_runtime(async {
            child.wait_with_output().await.map_err(|_| "Agent process failed".to_string())
        }, stop.clone()).await?;
        if !output.status.success() {
            // Only use diagnostic output for classification. Never return it to the UI:
            // CLI diagnostics can contain credentials, prompts, or private paths.
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let diagnostic = format!("{} {}", stderr.chars().take(8192).collect::<String>(), stdout.chars().take(8192).collect::<String>());
            let kind = classify_agent_error(&diagnostic);
            if kind == "rate_limit" {
                set_provider_cooldown(provider, &app.state::<ProviderCooldowns>());
                return Err(format!("{provider} rate limit confirmed. Cooldown 5 minutes. Automatic replay blocked because the CLI may have modified files."));
            }
            if kind == "auth_required" { return Err(format!("{provider} authentication required. Sign in again.")); }
            return Err(format!("{provider} CLI failed. Check provider authentication and permissions."));
        }
        if output.stdout.len() > 4_000_000 { return Err("Agent response exceeded output limit".into()); }
        let text = String::from_utf8_lossy(&output.stdout);
        let answer = if provider == "gemini" {
            serde_json::from_str::<serde_json::Value>(&text).ok()
                .and_then(|v| v.get("response").and_then(|r| r.as_str()).map(str::to_owned))
                .unwrap_or_else(|| text.to_string())
        } else { text.to_string() };
        let _ = app.emit("aam-event", serde_json::json!({"requestId":request_id,"data":{
            "kind":"result","provider":provider,"model":"CLI default","answer":answer,"usage":{}
        }}));
        Ok(())
    }.await;
    result
}

// Deterministic routing: task fit first, availability second. No paid model calls.
fn route_candidates(prompt: &str, mode: &str, permission: &str) -> Vec<&'static str> {
    let p = prompt.to_ascii_lowercase();
    let ordered: Vec<&str> = if p.contains("review") || p.contains("audit") || p.contains("security") || p.contains("bug") {
        vec!["codex", "claude", "gemini", "opencode", "copilot"]
    } else if p.contains("architecture") || p.contains("design") || p.contains("refactor") || p.contains("plan") {
        vec!["claude", "gemini", "codex", "opencode", "copilot"]
    } else if p.contains("github") || p.contains("pull request") || p.contains("issue") {
        vec!["copilot", "codex", "claude", "gemini", "opencode"]
    } else if p.contains("explain") || p.contains("document") || p.contains("summar") {
        vec!["gemini", "claude", "codex", "opencode", "copilot"]
    } else {
        vec!["codex", "copilot", "claude", "opencode", "gemini"]
    };
    let mut candidates: Vec<&str> = ordered.into_iter().filter(|id|
        permission == "workspace-write" || ["claude", "codex"].contains(id)
    ).collect();
    if mode == "balanced" {
        // Balanced prefers general-purpose agents, while respecting task constraints.
        candidates.sort_by_key(|id| match *id {
            "claude" => 0, "codex" => 1, "gemini" => 2, "opencode" => 3, _ => 4
        });
    }
    candidates
}

#[cfg(test)]
mod routing_tests {
    use super::route_candidates;

    #[test]
    fn task_fit_prefers_expected_provider() {
        assert_eq!(route_candidates("Review security bug", "auto", "workspace-write")[0], "codex");
        assert_eq!(route_candidates("Explain this module", "auto", "workspace-write")[0], "gemini");
        assert_eq!(route_candidates("Create GitHub pull request", "auto", "workspace-write")[0], "copilot");
    }

    #[test]
    fn permission_filter_excludes_uncontrolled_providers() {
        for permission in ["read-only", "full-access"] {
            assert_eq!(route_candidates("Explain this", "auto", permission).len(), 2);
            assert!(route_candidates("Explain this", "auto", permission)
                .iter().all(|id| ["claude", "codex"].contains(id)));
        }
    }

    #[test]
    fn classifies_provider_limits_without_exposing_output() {
        use super::classify_agent_error;
        for text in ["HTTP 429", "Rate limit exceeded", "RESOURCE_EXHAUSTED", "quota exceeded", "too many requests"] {
            assert_eq!(classify_agent_error(text), "rate_limit");
        }
        assert_eq!(classify_agent_error("401 Unauthorized"), "auth_required");
        assert_eq!(classify_agent_error("Compilation failed"), "unknown");
    }
    #[test]
    fn external_sandbox_rejects_unsupported_permissions() {
        use super::external_sandbox_ready;
        assert!(!external_sandbox_ready("read-only"));
        assert!(!external_sandbox_ready("full-access"));
    }

    #[test]
    fn cooldown_excludes_provider_until_expiry() {
        use super::{ProviderCooldowns, provider_available, set_provider_cooldown};
        let cooldowns = ProviderCooldowns::default();
        assert!(provider_available("gemini", &cooldowns));
        set_provider_cooldown("gemini", &cooldowns);
        assert!(!provider_available("gemini", &cooldowns));
        assert!(provider_available("codex", &cooldowns));
    }

    #[test]
    fn balanced_is_stable() {
        assert_eq!(route_candidates("Explain this", "balanced", "workspace-write")[0], "claude");
    }
}

// Rate-limit cooldowns are process-local. They never imply that a failed edit is safe to replay.
#[derive(Default)]
struct ProviderCooldowns(Mutex<HashMap<String, std::time::Instant>>);

fn provider_available(id: &str, cooldowns: &ProviderCooldowns) -> bool {
    let Ok(mut limits) = cooldowns.0.lock() else { return false; };
    limits.retain(|_, until| *until > std::time::Instant::now());
    !limits.contains_key(id)
}

fn classify_agent_error(message: &str) -> &'static str {
    let normalized = message.to_ascii_lowercase();
    if ["rate limit", "rate_limit", "429", "quota exceeded", "resource_exhausted",
        "usage limit", "too many requests", "credits exhausted", "limit reached"]
        .iter().any(|phrase| normalized.contains(phrase)) { "rate_limit" }
    else if ["unauthorized", "authentication required", "login required",
        "not logged in", "invalid api key", "401"].iter()
        .any(|phrase| normalized.contains(phrase)) { "auth_required" }
    else { "unknown" }
}

fn set_provider_cooldown(id: &str, cooldowns: &ProviderCooldowns) {
    if let Ok(mut limits) = cooldowns.0.lock() {
        limits.insert(id.to_string(), std::time::Instant::now() + std::time::Duration::from_secs(300));
    }
}

fn cli_on_path(binary: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            if dir.join(binary).is_file() { return true; }
            cfg!(windows) && ["exe", "cmd", "bat"].iter()
                .any(|ext| dir.join(format!("{binary}.{ext}")).is_file())
        })
    })
}

async fn verified_auth_probe(provider: &str) -> Option<bool> {
    let args: &[&str] = match provider {
        "claude" => &["auth", "status"],
        "codex" => &["login", "status"],
        // Do not mistake --version or auth-list exit status for an authenticated session.
        _ => return None,
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(8),
        tokio::process::Command::new(provider).args(args)
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null()).kill_on_drop(true).status()).await;
    Some(matches!(result, Ok(Ok(status)) if status.success()))
}

#[tauri::command]
async fn run_engine_adapter(app: &tauri::AppHandle, request_id: &str, root: &Path, provider: &str, prompt: &str, permission: &str, stop: Arc<tokio::sync::Notify>) -> Result<(), String> {
    // The AAM engine is isolated by the same OS boundary as external CLIs.
    // Its child processes inherit this sandbox.
    let executable = aam_executable()?;
    let root_arg = root.to_str().ok_or("Invalid workspace path")?;
    let args = [provider, "--root", root_arg, "--desktop-jsonl", "--ui", "classic", "--prompt", prompt];
    let mut child = sandbox_command(&executable, &args, root, permission)?;
    child.env("AAM_PERMISSION_MODE", permission);
    child.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped()).kill_on_drop(true);
    let mut child = child.spawn().map_err(|e| format!("PRESTART: Cannot launch aam: {e}"))?;
    let stdout = child.stdout.take().ok_or("Missing CLI stdout")?;
    let stderr = child.stderr.take().ok_or("Missing CLI stderr")?;
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut log = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            if log.len() < 16384 { log.push_str(&line); log.push('\n'); }
        }
        log
    });
    let mut lines = BufReader::new(stdout).lines();
    let mut reported_limit = false;
    let result = await_runtime(async {
        while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
                if event.get("kind").and_then(|v| v.as_str()) == Some("error") {
                    let message = event.get("message").and_then(|v| v.as_str()).unwrap_or("");
                    if classify_agent_error(message) == "rate_limit" {
                        reported_limit = true;
                    }
                }
                let _ = app.emit("aam-event", serde_json::json!({"requestId": request_id, "data": event}));
            }
        }
        child.wait().await.map_err(|e| e.to_string())
    }, stop.clone()).await;
    if result.is_err() {
        // kill_on_drop terminates the AAM process; the engine's subprocess handling
        // may need separate process-group cleanup on each supported OS.
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
    if let Err(message) = result.as_ref() {
        let _ = app.emit("aam-event", serde_json::json!({"requestId": request_id, "data": {"kind":"process_error", "message":message}}));
    }
    let stderr = if result.is_err() { stderr_task.abort(); String::new() } else { tokio::time::timeout(std::time::Duration::from_secs(2), stderr_task).await.ok().and_then(Result::ok).unwrap_or_default() };
    if reported_limit || classify_agent_error(&stderr) == "rate_limit" {
        set_provider_cooldown(provider, &app.state::<ProviderCooldowns>());
    }
    if result.as_ref().is_ok_and(|status| !status.success()) {
        let message = if reported_limit || classify_agent_error(&stderr) == "rate_limit" {
            format!("{provider} rate limit detected; cooldown 5 minutes. Automatic replay blocked.")
        } else if classify_agent_error(&stderr) == "auth_required" {
            format!("{provider} authentication required.")
        } else { format!("{provider} engine exited unsuccessfully. Check CLI diagnostics.") };
        let _ = app.emit("aam-event", serde_json::json!({"requestId": request_id, "data": {"kind":"process_error", "message":message}}));
        return Err(message);
    }
    result.map(|_| ())
}

// A failure is replay-safe only if it happened before the agent process started.
// Runtime errors are deliberately not eligible: even read-only prompts can
// invoke external tools with side effects.
fn retry_before_spawn(error: &str) -> bool {
    error.starts_with("PRESTART:")
}

#[cfg(test)]
mod execution_fallback_tests {
    use super::retry_before_spawn;
    #[test]
    fn only_prestart_errors_allow_fallback() {
        assert!(retry_before_spawn("PRESTART: executable missing"));
        for message in ["Agent timed out", "rate limit", "Request cancelled",
            "authentication required", "CLI failed", "spawned process failed"] {
            assert!(!retry_before_spawn(message));
        }
    }
}

async fn execute_adapter(app: &tauri::AppHandle, request_id: &str, root: &Path, adapter: ProviderAdapter, prompt: &str, permission: &str) -> Result<(), String> {
    validate_execution(adapter, permission)?;
    let stop = Arc::new(tokio::sync::Notify::new());
    {
        let state = app.state::<RunningRequests>();
        let mut requests = state.0.lock().map_err(|_| "Request state unavailable")?;
        if requests.contains_key(request_id) { return Err("Request already running".into()); }
        requests.insert(request_id.to_string(), stop.clone());
    }
    let result = match adapter.transport {
        ExecutionAdapter::Engine =>
            run_engine_adapter(app, request_id, root, adapter.id, prompt, permission, stop.clone()).await,
        ExecutionAdapter::SandboxedCli =>
            run_external_provider(app, request_id, root, adapter.id, prompt, permission, stop.clone()).await,
    };
    if let Ok(mut requests) = app.state::<RunningRequests>().0.lock() {
        requests.remove(request_id);
    }
    if let Err(ref message) = result {
        let _ = app.emit("aam-event", serde_json::json!({"requestId":request_id,"data":{
            "kind":"process_error","provider":adapter.id,"message":message
        }}));
    }
    result
}

async fn run_prompt(app: tauri::AppHandle, request_id: String, root: String, provider: String, prompt: String, permission: String) -> Result<(), String> {
    if !["claude", "codex", "gemini", "opencode", "copilot", "auto", "balanced"].contains(&provider.as_str()) { return Err("Invalid provider".into()); }
    if prompt.trim().is_empty() { return Err("Prompt is empty".into()); }
    if !["read-only", "workspace-write", "full-access"].contains(&permission.as_str()) { return Err("Invalid permission mode".into()); }
    let path = Path::new(&root).canonicalize().map_err(|e| format!("Workspace: {e}"))?;
    if !path.is_dir() { return Err("Workspace is not a directory".into()); }
    let status = Command::new("git").args(["-C", path.to_str().ok_or("Invalid path")?, "rev-parse", "--is-inside-work-tree"]).output().map_err(|e| format!("Git: {e}"))?;
    if !status.status.success() { return Err("Workspace must be a Git repository".into()); }
    let automatic = provider == "auto" || provider == "balanced";
    let candidates: Vec<&str> = if automatic {
        route_candidates(&prompt, &provider, &permission)
    } else {
        vec![provider.as_str()]
    };
    let mut last_preflight_error = String::from("No compatible agent CLI available");
    for candidate in candidates {
        let Some(adapter) = provider_adapter(candidate) else { continue; };
        if automatic && !provider_available(candidate, &app.state::<ProviderCooldowns>()) {
            continue;
        }
        if let Err(message) = validate_execution(adapter, &permission) {
            last_preflight_error = message;
            continue;
        }
        if verified_auth_probe(candidate).await == Some(false) {
            last_preflight_error = format!("{candidate} requires authentication");
            continue;
        }
        let _ = app.emit("aam-event", serde_json::json!({"requestId":request_id,"data":{
            "kind":"routing","reason":format!("Executing {candidate}; preflight passed"),
            "provider":candidate,"model":"CLI default"
        }}));
        // An adapter that has started may have modified files or emitted effects.
        // Never replay such a task automatically, even if it reports a rate limit.
        match execute_adapter(&app, &request_id, &path, adapter, &prompt, &permission).await {
            Err(message) if automatic && retry_before_spawn(&message) => {
                last_preflight_error = message;
                continue;
            }
            outcome => return outcome,
        }
    }
    Err(last_preflight_error)
}


#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AiConfigItem {
    name: String,
    path: String,
    kind: String,
    claude: String,
    codex: String,
    claude_reason: String,
    codex_reason: String,
}

fn add_ai_file(items: &mut Vec<AiConfigItem>, root: &Path, file: &Path, kind: &str, claude: (&str, &str), codex: (&str, &str)) {
    if !file.is_file() || items.len() >= 150 { return; }
    let path = file.strip_prefix(root).unwrap_or(file).to_string_lossy().into_owned();
    items.push(AiConfigItem {
        name: file.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        path, kind: kind.into(),
        claude: claude.0.into(), codex: codex.0.into(),
        claude_reason: claude.1.into(), codex_reason: codex.1.into(),
    });
}

#[tauri::command]
fn project_ai_config(root: String) -> Result<Vec<AiConfigItem>, String> {
    let root = Path::new(&root).canonicalize().map_err(|e| format!("Workspace: {e}"))?;
    if !root.is_dir() { return Err("Workspace is not a directory".into()); }
    let mut items = Vec::new();
    add_ai_file(&mut items, &root, &root.join("CLAUDE.md"), "instructions",
        ("supported", "Claude Code recognizes CLAUDE.md; actual loading depends on CLI settings."),
        ("unsupported", "Codex does not natively use CLAUDE.md as its project instruction file."));
    add_ai_file(&mut items, &root, &root.join("AGENTS.md"), "instructions",
        ("unsupported", "Claude Code does not natively use AGENTS.md as its project instruction file."),
        ("supported", "Codex recognizes AGENTS.md; actual loading depends on CLI settings."));
    for (folder, claude, codex) in [
        (".claude/skills", ("supported", "Claude Code skill location; invocation is conditional."), ("unsupported", "Not a standard Codex skill directory.")),
        (".agents/skills", ("unknown", "Claude Code support depends on version and configuration."), ("supported", "Codex project skill location; invocation is conditional.")),
    ] {
        let directory = root.join(folder);
        if !directory.is_dir() { continue; }
        if let Ok(entries) = std::fs::read_dir(&directory) {
            for entry in entries.flatten().take(100) {
                let skill = entry.path().join("SKILL.md");
                add_ai_file(&mut items, &root, &skill, "skill", claude, codex);
            }
        }
    }
    for (folder, claude, codex) in [
        (".claude/commands", ("supported", "Claude Code custom command; use depends on CLI version."), ("unsupported", "Claude commands are not Codex commands.")),
    ] {
        let directory = root.join(folder);
        if let Ok(entries) = std::fs::read_dir(directory) {
            for entry in entries.flatten().take(100) {
                if entry.path().extension().and_then(|s| s.to_str()) == Some("md") {
                    add_ai_file(&mut items, &root, &entry.path(), "command", claude, codex);
                }
            }
        }
    }
    Ok(items)
}


#[tauri::command]
fn architect_preview(root: String) -> Result<project_architect::ProjectPlan, String> {
    project_architect::plan(Path::new(&root))
}

#[tauri::command]
fn architect_apply(root: String, proposals: Vec<project_architect::Proposal>) -> Result<Vec<String>, String> {
    project_architect::apply(Path::new(&root), proposals)
}

fn main() {
    configure_agent_path();
    tauri::Builder::default().manage(RunningRequests::default()).manage(LoginSession::default())
        .manage(ProviderCooldowns::default()).plugin(tauri_plugin_dialog::init()).invoke_handler(tauri::generate_handler![run_prompt, github_repositories, github_clone, project_ai_config, desktop_diagnostics, provider_setup_status, node_setup_status, install_node_runtime, install_provider_cli, open_provider_login, login_pty_write, focus_after_login, cancel_prompt, architect_preview, architect_apply])
        .run(tauri::generate_context!()).expect("Tauri application failed");
}
