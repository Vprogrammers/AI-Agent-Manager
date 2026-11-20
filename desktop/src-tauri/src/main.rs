#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{path::{Path, PathBuf}, process::Command, sync::{Arc,Mutex}};
use std::collections::HashMap;
use tauri::Manager;
mod project_architect;
mod approval;
mod interactive;

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
        "antigravity" => return Err("Install Antigravity CLI from https://antigravity.google/docs/cli/tutorial and retry setup.".into()),
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
fn cancel_provider_login(state: tauri::State<'_, LoginSession>) -> Result<(), String> {
    use std::io::Write;
    let mut guard = state.0.lock().map_err(|_| "Login session unavailable")?;
    let writer = guard.as_mut().ok_or("No active login session")?;
    // Ctrl-C is delivered to the foreground process via the PTY.
    writer.write_all(&[3]).map_err(|_| "Unable to interrupt login")?;
    writer.flush().map_err(|_| "Unable to interrupt login".to_string())
}

#[tauri::command]
fn open_provider_login(provider: String, app: tauri::AppHandle, state: tauri::State<'_, LoginSession>) -> Result<String, String> {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    let (binary, args): (&str, &[&str]) = match provider.as_str() {
        "claude" => ("claude", &["auth", "login"]),
        "codex" => ("codex", &["login"]),
        "antigravity" => ("agy", &[]),
        "opencode" => ("opencode", &["auth", "login"]),
        "copilot" => ("copilot", &["login"]),
        _ => return Err("Unsupported provider".into()),
    };
    let executable = find_cli(binary).ok_or("Provider CLI not found. Recheck installation first.")?;
    if state.0.lock().map_err(|_| "Login session unavailable")?.is_some() {
        return Err("A login is already in progress. Cancel the previous login first.".into());
    }
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

// Authentication checks never return credential contents or raw CLI output to the UI.
async fn auth_probe_output(executable: &std::path::Path, args: &[&str]) -> Option<std::process::Output> {
    tokio::time::timeout(std::time::Duration::from_secs(8),
        tokio::process::Command::new(executable).args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true).output()
    ).await.ok()?.ok()
}

fn opencode_has_accounts(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(accounts) => !accounts.is_empty(),
        serde_json::Value::Object(map) => {
            if let Some(accounts) = map.get("accounts") { return opencode_has_accounts(accounts); }
            if let Some(providers) = map.get("providers") { return opencode_has_accounts(providers); }
            !map.is_empty()
        }
        _ => false,
    }
}

#[cfg(test)]
mod provider_auth_tests {
    use super::*;
    #[test]
    fn opencode_requires_a_nonempty_account_list() {
        assert!(!opencode_has_accounts(&serde_json::json!([])));
        assert!(!opencode_has_accounts(&serde_json::json!({"accounts":[]})));
        assert!(opencode_has_accounts(&serde_json::json!([{"provider":"anthropic"}])));
        assert!(opencode_has_accounts(&serde_json::json!({"accounts":[{"provider":"openai"}]})));
        assert!(!opencode_has_accounts(&serde_json::Value::Null));
    }
}

async fn probe_provider_auth(id: &str, executable: &std::path::Path) -> (bool, &'static str, &'static str) {
    let (args, verified_label): (&[&str], &str) = match id {
        "claude" => (&["auth", "status"], "Claude authentication confirmed"),
        "codex" => (&["login", "status"], "Codex authentication confirmed"),
        "opencode" => (&["auth", "list", "--format", "json"], "OpenCode provider account detected"),
        "antigravity" => (&["models"], "Antigravity models available through authenticated CLI"),
        "copilot" => {
            // Copilot's own OAuth login stores a credential in the macOS keychain.
            // Query metadata only; never retrieve or log the secret itself.
            #[cfg(target_os = "macos")]
            {
                let security = std::path::Path::new("/usr/bin/security");
                if auth_probe_output(security, &["find-generic-password", "-s", "copilot-cli"])
                    .await.is_some_and(|out| out.status.success()) {
                    return (true, "ready", "Copilot CLI OAuth credential detected in macOS Keychain; entitlement checked on execution");
                }
            }
            // GitHub CLI credentials are an officially supported Copilot fallback.
            // This verifies the login, not Copilot license or organization policy.
            if let Some(gh) = find_cli("gh") {
                if auth_probe_output(&gh, &["auth", "status"]).await.is_some_and(|out| out.status.success()) {
                    return (true, "ready", "GitHub CLI login detected (Copilot fallback); entitlement checked on execution");
                }
            }
            return (false, "unknown", "Copilot credentials not detected; use Copilot login or check your provider account");
        }
        _ => return (false, "unknown", "Unsupported provider authentication probe"),
    };
    let Some(output) = auth_probe_output(executable, args).await else {
        return (false, "unknown", "Authentication probe unavailable or timed out");
    };
    if !output.status.success() {
        return if id == "claude" || id == "codex" {
            (false, "login_required", "CLI reports no active authentication")
        } else {
            (false, "unknown", "CLI could not verify authentication; sign in or check provider configuration")
        };
    }
    let authenticated = match id {
        "opencode" => serde_json::from_slice::<serde_json::Value>(&output.stdout)
            .ok().is_some_and(|value| opencode_has_accounts(&value)),
        "antigravity" => !output.stdout.iter().all(|byte| byte.is_ascii_whitespace()),
        _ => true,
    };
    if authenticated { (true, "ready", match id {
        "opencode" => "OpenCode connected provider account found (credentials not exposed)",
        "antigravity" => "Antigravity CLI returned available models",
        _ => verified_label,
    }) } else {
        (false, "unknown", "No confirmed provider account or models found")
    }
}

#[tauri::command]
async fn provider_setup_status() -> Vec<ProviderSetup> {
    let mut result = Vec::new();
    for (id, binary) in [
        ("claude", "claude"), ("codex", "codex"), ("antigravity", "agy"),
        ("opencode", "opencode"), ("copilot", "copilot"),
    ] {
        let Some(executable) = find_cli(binary) else {
            result.push(ProviderSetup { id: id.into(), installed: false, authenticated: false,
                auth_status: "missing".into(), detail: "CLI not found in PATH".into() });
            continue;
        };
        let (authenticated, auth_status, detail) = probe_provider_auth(id, &executable).await;
        result.push(ProviderSetup { id: id.into(), installed: true, authenticated,
            auth_status: auth_status.into(), detail: detail.into() });
    }
    result
}

#[tauri::command]
fn cancel_prompt(state: tauri::State<'_, RunningRequests>, approvals: tauri::State<'_, approval::ApprovalRegistry>, request_id: String) -> Result<(), String> {
    let requests = state.0.lock().map_err(|_|"Request state unavailable")?;
    let signal = requests.get(&request_id).ok_or("Request is no longer running")?;
    approvals.cancel_request(&request_id);
    signal.notify_one();
    Ok(())
}

#[tauri::command]
fn decide_agent_approval(approvals: tauri::State<'_, approval::ApprovalRegistry>, approval_id: String, request_id: String, allow: bool) -> Result<(), String> {
    approvals.decide(&approval_id, &request_id, allow)
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
    ProviderAdapter { id: "antigravity", transport: ExecutionAdapter::SandboxedCli },
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
    if !cli_on_path(if adapter.id == "antigravity" { "agy" } else { adapter.id }) { return Err(format!("{} CLI not installed", adapter.id)); }
    if permission != "full-access" && !external_sandbox_ready(permission) {
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
        for id in ["antigravity", "opencode", "copilot"] {
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
    if permission != "full-access" && !external_sandbox_ready(permission) {
        return Err("No supported OS sandbox for this permission mode; execution blocked".into());
    }
    if !cli_on_path(if provider == "antigravity" { "agy" } else { provider }) { return Err(format!("{provider} CLI not installed")); }
    Ok(())
}

// Escape the workspace for macOS Seatbelt's quoted sandbox profile syntax.
// The profile grants writes only to the selected workspace and ephemeral /tmp.
#[cfg(target_os = "macos")]
fn macos_sandbox_profile(root: &Path, permission: &str, provider: &str) -> Result<String, String> {
    fn quoted(path: &Path) -> Result<String, String> {
        let text = path.to_str().ok_or("Sandbox path is not UTF-8")?;
        if text.chars().any(|c| c == '\0' || c == '\n' || c == '\r') {
            return Err("Unsupported sandbox path".into());
        }
        Ok(text.replace('\\', "\\\\").replace('"', "\\\""))
    }
    if permission != "read-only" && permission != "workspace-write" {
        return Err("Unsupported sandbox permission".into());
    }
    // Codex needs to persist its local session, and AAM stores session state
    // in Git metadata. Neither location is covered by the workspace path.
    let home = std::env::var_os("HOME").ok_or("HOME unavailable")?;
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from).unwrap_or_else(|| PathBuf::from(&home).join(".codex"));
    let git_dir = std::process::Command::new("git")
        .arg("-C").arg(root).args(["rev-parse", "--absolute-git-dir"])
        .output().map_err(|_| "Cannot resolve Git metadata directory")?;
    if !git_dir.status.success() { return Err("Cannot resolve Git metadata directory".into()); }
    let git_path = PathBuf::from(String::from_utf8(git_dir.stdout)
        .map_err(|_| "Invalid Git metadata path")?.trim());
    let codex = quoted(&codex_home)?;
    // The Python engine initializes provider profiles even for Claude.
    let profile_state = quoted(&PathBuf::from(&home).join(".local/state/ai-agent-manager"))?;
    let git = quoted(&git_path.join("ai-agent-manager"))?;
    // External CLIs maintain their own state outside the selected workspace.
    // Allow only the active provider's known cache/config/state locations, never HOME.
    let provider_paths: &[&str] = match provider {
        "opencode" => &[
            ".config/opencode", ".local/share/opencode",
            ".local/state/opencode", ".cache/opencode",
        ],
        "antigravity" => &[
            ".config/antigravity", ".local/share/antigravity",
            ".cache/antigravity", ".gemini",
        ],
        "copilot" => &[
            ".copilot", // Copilot CLI stores session-state and session events here.
            ".config/copilot", ".local/share/copilot",
            ".cache/copilot",
        ],
        _ => &[],
    };
    let provider_writes = provider_paths.iter()
        .map(|relative| quoted(&PathBuf::from(&home).join(relative))
            .map(|path| format!(r#"(subpath "{path}")"#)))
        .collect::<Result<Vec<_>, _>>()?.join(" ");
    let workspace = if permission == "workspace-write" {
        format!(r#"(subpath "{}")"#, quoted(root)?)
    } else { String::new() };
    Ok(format!(r#"(version 1) (deny default) (allow process*) (allow sysctl-read) (allow mach-lookup) (allow ipc*) (allow network*) (allow file-read*) (allow file-write* (subpath "/private/tmp") (subpath "/tmp") (literal "/dev/null") (subpath "{codex}") (subpath "{profile_state}") (subpath "{git}") {provider_writes} {workspace})"#))
}

// One platform isolation policy for both engine and direct CLI transports.
// On unsupported platforms, never silently launch an unrestricted child.
fn sandbox_command(executable: &Path, args: &[&str], root: &Path, permission: &str, provider: &str) -> Result<tokio::process::Command, String> {
    // Full access is an explicit user-approved opt-out of the OS sandbox.
    if permission == "full-access" {
        let mut command = tokio::process::Command::new(executable);
        command.args(args).current_dir(root);
        return Ok(command);
    }
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
        command.arg("-p").arg(macos_sandbox_profile(root, permission, provider)?).arg(executable).args(args);
        Ok(command)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (executable, args, root, permission, provider);
        Err("No supported OS sandbox; execution blocked".into())
    }
}

// Start every Unix sandbox in its own process group so cancellation can
// terminate descendants as well as the sandbox launcher.
#[cfg(unix)]
fn isolate_process_group(command: &mut tokio::process::Command) {
    use std::os::unix::process::CommandExt;
    command.as_std_mut().process_group(0);
}

#[cfg(unix)]
fn kill_process_group(pid: Option<u32>) {
    if let Some(pid) = pid {
        // Negative PID targets the entire group, not only the leader.
        let _ = std::process::Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
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

// Keep actionable CLI diagnostics while withholding lines likely to contain secrets.
fn safe_provider_diagnostic(stderr: &str) -> String {
    let mut lines = Vec::new();
    for line in stderr.lines().rev() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if trimmed.is_empty() || trimmed.len() > 350
            || ["token", "secret", "password", "authorization", "bearer",
                "api_key", "apikey", "credential", "cookie", "sk-", "ghp_"]
                .iter().any(|word| lower.contains(word)) {
            continue;
        }
        // Avoid echoing JSON configuration and user content from CLI failures.
        if trimmed.starts_with('{') || trimmed.starts_with('[') { continue; }
        lines.push(trimmed.to_string());
        if lines.len() == 3 { break; }
    }
    lines.reverse();
    if lines.is_empty() { "No safe CLI error detail available".into() }
    else { lines.join(" | ") }
}

async fn run_external_provider(app: &tauri::AppHandle, request_id: &str, root: &Path, provider: &str, prompt: &str, permission: &str, stop: Arc<tokio::sync::Notify>) -> Result<(), String> {
    let phase_started = std::time::Instant::now();
    let (binary, args): (&str, Vec<&str>) = match provider {
        "antigravity" => ("agy", vec!["--print", prompt]),
        "opencode" => ("opencode", vec!["run", prompt]),
        "copilot" => ("copilot", vec!["-p", prompt, "-s", "--no-ask-user"]),
        _ => return Err("Unsupported provider".into()),
    };
    // Resolve the executable once; avoid repeating PATH scans in preflight.
    if permission != "full-access" && !external_sandbox_ready(permission) {
        return Err("PRESTART: No supported OS sandbox for this permission mode; execution blocked".into());
    }
    let executable = find_cli(binary).ok_or(format!("PRESTART: {provider} CLI not found in PATH"))?;
    let mut command = sandbox_command(&executable, &args, root, permission, provider).map_err(|e| format!("PRESTART: {e}"))?;
    command.current_dir(root).stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    #[cfg(unix)]
    isolate_process_group(&mut command);
    let setup_ms = phase_started.elapsed().as_millis();
    let result = async {
        let mut child = command.spawn().map_err(|e| format!("PRESTART: Could not start {provider} CLI: {e}"))?;
        let spawn_ms = phase_started.elapsed().as_millis().saturating_sub(setup_ms);
        let process_started = std::time::Instant::now();
        let mut first_output_ms: Option<u128> = None;
        #[cfg(unix)]
        let group_pid = child.id();
        let stdout = child.stdout.take().ok_or("Missing CLI stdout")?;
        let stderr = child.stderr.take().ok_or("Missing CLI stderr")?;
        let stderr_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut diagnostic = String::new();
            while let Ok(Some(line)) = lines.next_line().await {
                if diagnostic.len() < 8192 { diagnostic.push_str(&line); diagnostic.push('\n'); }
            }
            diagnostic
        });
        // Read raw bytes instead of lines: many CLIs flush partial text without
        // a newline. Emit each UTF-8-safe chunk immediately to the desktop.
        use tokio::io::AsyncReadExt;
        let mut output = BufReader::new(stdout);
        let mut answer = String::new();
        let mut pending_utf8 = Vec::<u8>::new();
        let mut buffer = [0u8; 4096];
        let read_result = await_runtime(async {
            loop {
                let n = output.read(&mut buffer).await.map_err(|e| e.to_string())?;
                if n == 0 { break; }
                if first_output_ms.is_none() { first_output_ms = Some(process_started.elapsed().as_millis()); }
                pending_utf8.extend_from_slice(&buffer[..n]);
                let valid_len = match std::str::from_utf8(&pending_utf8) {
                    Ok(_) => pending_utf8.len(),
                    Err(error) if error.error_len().is_none() => error.valid_up_to(),
                    Err(_) => return Err("CLI emitted invalid UTF-8 output".into()),
                };
                if valid_len > 0 {
                    let chunk = std::str::from_utf8(&pending_utf8[..valid_len])
                        .map_err(|e| e.to_string())?;
                    if answer.len() + chunk.len() > 4_000_000 {
                        return Err("Agent response exceeded output limit".into());
                    }
                    answer.push_str(chunk);
                    let _ = app.emit("aam-event", serde_json::json!({"requestId":request_id,"data":{
                        "kind":"activity","event":{"type":"external.chunk","provider":provider,"text":chunk}
                    }}));
                    pending_utf8.drain(..valid_len);
                }
            }
            if !pending_utf8.is_empty() { return Err("CLI output ended with incomplete UTF-8".into()); }
            child.wait().await.map_err(|e| e.to_string())
        }, stop.clone()).await;
        if read_result.is_err() {
            #[cfg(unix)]
            kill_process_group(group_pid);
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        let stderr = if read_result.is_err() {
            stderr_task.abort();
            String::new()
        } else {
            // A successful CLI has already produced its answer. Do not hold the
            // response for up to two seconds waiting for diagnostics to drain.
            // Preserve the longer diagnostic window for failed executions.
            let diagnostic_wait = if read_result.as_ref().is_ok_and(|status| status.success()) {
                std::time::Duration::from_millis(100)
            } else {
                std::time::Duration::from_secs(2)
            };
            tokio::time::timeout(diagnostic_wait, stderr_task)
                .await.ok().and_then(Result::ok).unwrap_or_default()
        };
        let process_ms = process_started.elapsed().as_millis();
        let _ = app.emit("aam-event", serde_json::json!({"requestId":request_id,"data":{
            "kind":"cli_phases","provider":provider,"setupMs":setup_ms,
            "spawnMs":spawn_ms,"firstOutputMs":first_output_ms,
            "processMs":process_ms,"exitCode":read_result.as_ref().ok().and_then(|status|status.code())
        }}));
        let status = read_result?;
        if !status.success() {
            let kind = classify_agent_error(&format!("{} {}", stderr, answer.chars().take(8192).collect::<String>()));
            if kind == "rate_limit" {
                set_provider_cooldown(provider, &app.state::<ProviderCooldowns>());
                return Err(format!("{provider} rate limit detected; cooldown 5 minutes. Continuing with a repository-state review on the next provider."));
            }
            if kind == "auth_required" { return Err(format!("{provider} authentication required. Sign in again.")); }
            let exit_code = status.code().map(|code| code.to_string()).unwrap_or_else(|| "signal".into());
            eprintln!("AAM external provider failed: provider={provider}, exit={exit_code}, classification={kind}, permission={permission}, diagnostic={}", safe_provider_diagnostic(&stderr));
            let hint = match (provider, kind) {
                (_, "sandbox_permission") => "CLI was denied filesystem access. Check provider-specific cache/config permissions in the OS sandbox; do not disable isolation as a permanent workaround.",
                (_, "insufficient_credits") => "Provider has insufficient credits for the selected paid model. Choose an available model/account or add credits; this is not a login or sandbox error.",
                (_, "model_configuration") => "CLI could not use its configured model/provider. Verify the selected model and provider account.",
                (_, "network") => "CLI could not reach its provider. Check network access and provider availability.",
                ("opencode", _) => "OpenCode exited before completing the request. Test 'opencode run' in the same workspace; check configured model and CLI diagnostics.",
                ("copilot", _) => "Copilot failed. Verify 'copilot -p' works in your terminal.",
                ("antigravity", _) => "Antigravity failed. Verify 'agy --print' works non-interactively in the same workspace.",
                _ => "Provider CLI failed. Check its configuration and permissions.",
            };
            return Err(format!("{hint} (exit: {exit_code}, category: {kind}). CLI detail: {}", safe_provider_diagnostic(&stderr)));
        }
        let _ = app.emit("aam-event", serde_json::json!({"requestId":request_id,"data":{
            "kind":"result","provider":provider,"model":"CLI default","answer":answer,"usage":{}
        }}));
        Ok(())
    }.await;
    result
}

// Deterministic routing: explicit choice, primary intent, then availability.
// Verification words alone must not override an implementation or architecture task.
fn route_candidates(prompt: &str, mode: &str, permission: &str) -> Vec<&'static str> {
    let p = prompt.to_lowercase();
    let has = |terms: &[&str]| terms.iter().any(|term| p.contains(term));
    let providers = ["claude", "codex", "antigravity", "opencode", "copilot"];
    let explicit = providers.iter().copied().find(|id| {
        let markers = [
            format!("use {id}"), format!("koristi {id}"),
            format!("with {id}"), format!("sa {id}"),
            format!("via {id}"), format!("@{id}"),
        ];
        markers.iter().any(|marker| p.contains(marker))
    });
    let ordered: Vec<&str> = if let Some(id) = explicit {
        std::iter::once(id).chain(providers.iter().copied().filter(|candidate| *candidate != id)).collect()
    } else if mode == "balanced" {
        vec!["claude", "codex", "antigravity", "opencode", "copilot"]
    } else if has(&["github", "pull request", "create issue", "otvori issue"]) {
        vec!["copilot", "codex", "claude", "antigravity", "opencode"]
    } else if has(&["implement", "implementir", "popravi", "ispravi", "fix ", "izmeni", "izmijeni", "promeni", "promijeni", "dodaj", "napravi", "uradi", "sredi", "code change"]) {
        vec!["codex", "claude", "antigravity", "opencode", "copilot"]
    } else if has(&["architecture", "arhitektur", "design", "dizajn", "refactor", "planir", "roadmap"]) {
        vec!["claude", "codex", "antigravity", "opencode", "copilot"]
    } else if has(&["security", "audit", "debug", "bug", "test failure", "failing test", "regression", "grešk", "gresk"]) {
        vec!["codex", "claude", "antigravity", "opencode", "copilot"]
    } else if has(&["explain", "document", "summar", "objasn", "dokument", "saž", "saz", "analizir", "analiz", "prijedlog", "predlog"]) {
        vec!["antigravity", "claude", "codex", "opencode", "copilot"]
    } else {
        vec!["claude", "codex", "antigravity", "opencode", "copilot"]
    };
    // Auto prefers Claude for all tasks. Explicit provider selection still takes precedence;
    // Balanced retains its own ordering. Codex is a fallback, not the default coding agent.
    let ordered = if mode == "auto" && explicit.is_none() {
        std::iter::once("claude").chain(ordered.into_iter().filter(|id| *id != "claude")).collect::<Vec<_>>()
    } else { ordered };
    ordered.into_iter().filter(|id|
        permission == "workspace-write" || ["claude", "codex"].contains(id)
    ).collect()
}

#[cfg(test)]
mod routing_tests {
    use super::route_candidates;

    #[test]
    fn serbian_analysis_prefers_antigravity_and_explicit_request_is_honored() {
        assert_eq!(route_candidates("Analiziraj projekat i daj prijedlog", "auto", "workspace-write")[0], "antigravity");
        assert_eq!(route_candidates("Use Antigravity to inspect this repository", "auto", "workspace-write")[0], "antigravity");
        assert_eq!(route_candidates("Analiziraj arhitekturu", "auto", "workspace-write")[0], "claude");
    }
    #[test]
    fn task_fit_prefers_expected_provider() {
        assert_eq!(route_candidates("Review security bug", "auto", "workspace-write")[0], "codex");
        assert_eq!(route_candidates("Explain this module", "auto", "workspace-write")[0], "antigravity");
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
        assert_eq!(classify_agent_error("Insufficient balance. This request is pay-as-you-go, billed to OpenCode Models credits."), "insufficient_credits");
    }
    #[test]
    fn external_sandbox_rejects_unsupported_permissions() {
        use super::external_sandbox_ready;
        // Availability depends on installed OS tools (bwrap on Linux, sandbox-exec on macOS).
        // Assert the permission contract, not the CI runner's installed binaries.
        assert_eq!(external_sandbox_ready("read-only"), external_sandbox_ready("workspace-write"));
        assert!(!external_sandbox_ready("invalid"));
        assert!(!external_sandbox_ready("full-access"));
    }

    #[test]
    fn cooldown_excludes_provider_until_expiry() {
        use super::{ProviderCooldowns, provider_available, set_provider_cooldown};
        let cooldowns = ProviderCooldowns::default();
        assert!(provider_available("antigravity", &cooldowns));
        set_provider_cooldown("antigravity", &cooldowns);
        assert!(!provider_available("antigravity", &cooldowns));
        assert!(provider_available("codex", &cooldowns));
    }

    #[test]
    fn routing_prioritizes_primary_intent_over_generic_verification() {
        let prompt = "Procitaj handoff, proveri Git stanje, popravi valutu u formi i testiraj.";
        assert_eq!(route_candidates(prompt, "auto", "workspace-write")[0], "codex");
        assert_eq!(route_candidates("Proveri Git stanje i nastavi plan arhitekture", "auto", "workspace-write")[0], "claude");
        assert_eq!(route_candidates("Proveri Git stanje", "auto", "workspace-write")[0], "claude");
    }
    #[test]
    fn routing_respects_explicit_provider_and_balanced() {
        assert_eq!(route_candidates("Koristi Codex za arhitekturu", "auto", "workspace-write")[0], "codex");
        assert_eq!(route_candidates("Use Antigravity to debug", "balanced", "workspace-write")[0], "antigravity");
        assert_eq!(route_candidates("Implement feature", "balanced", "workspace-write")[0], "claude");
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
    else if ["insufficient balance", "insufficient credits", "billing/credits", "payment required"].iter().any(|phrase| normalized.contains(phrase)) { "insufficient_credits" }
    else if ["unauthorized", "authentication required", "login required",
        "not logged in", "invalid api key", "401"].iter()
        .any(|phrase| normalized.contains(phrase)) { "auth_required" }
    else if ["operation not permitted", "permission denied", "sandbox", "access denied", "eacces", "eperm", "read-only file system"].iter().any(|phrase| normalized.contains(phrase)) { "sandbox_permission" }
    else if ["model not found", "model is not configured", "no model", "unknown model", "provider not found", "no provider", "api key not set"].iter().any(|phrase| normalized.contains(phrase)) { "model_configuration" }
    else if ["network", "connection refused", "dns", "timed out", "enotfound"].iter().any(|phrase| normalized.contains(phrase)) { "network" }
    else { "unknown" }
}

fn set_provider_cooldown(id: &str, cooldowns: &ProviderCooldowns) {
    if let Ok(mut limits) = cooldowns.0.lock() {
        limits.insert(id.to_string(), std::time::Instant::now() + std::time::Duration::from_secs(300));
    }
}

fn find_cli(binary: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect())
        .unwrap_or_default();
    // GUI applications on macOS may not inherit the interactive shell PATH.
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/bin"));
    }
    dirs.into_iter().flat_map(|dir| {
        let mut candidates = vec![dir.join(binary)];
        if cfg!(windows) {
            for ext in ["exe", "cmd", "bat"] {
                candidates.push(dir.join(format!("{binary}.{ext}")));
            }
        }
        candidates
    }).find(|path| path.is_file())
}

fn cli_on_path(binary: &str) -> bool {
    find_cli(binary).is_some()
}

// Cache successful CLI authentication probes for one minute. Failed probes are
// never cached; provider execution still performs its own authentication checks.
static AUTH_PROBE_CACHE: std::sync::OnceLock<Mutex<HashMap<String, std::time::Instant>>> = std::sync::OnceLock::new();

fn cached_auth_ok(provider: &str) -> bool {
    let cache = AUTH_PROBE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    cache.lock().ok().and_then(|items| items.get(provider).copied())
        .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(60))
}

fn remember_auth_ok(provider: &str) {
    let cache = AUTH_PROBE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut items) = cache.lock() {
        items.insert(provider.to_owned(), std::time::Instant::now());
    }
}

async fn verified_auth_probe(provider: &str) -> Option<bool> {
    if cached_auth_ok(provider) { return Some(true); }
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
    let authenticated = matches!(result, Ok(Ok(status)) if status.success());
    if authenticated { remember_auth_ok(provider); }
    Some(authenticated)
}

#[tauri::command]
async fn run_engine_adapter(app: &tauri::AppHandle, request_id: &str, root: &Path, provider: &str, prompt: &str, permission: &str, stop: Arc<tokio::sync::Notify>) -> Result<(), String> {
    // The AAM engine is isolated by the same OS boundary as external CLIs.
    // Its child processes inherit this sandbox.
    let executable = aam_executable().map_err(|e| format!("PRESTART: {e}"))?;
    let root_arg = root.to_str().ok_or("Invalid workspace path")?;
    let args = [provider, "--root", root_arg, "--desktop-jsonl", "--ui", "classic", "--prompt", prompt];
    let mut child = sandbox_command(&executable, &args, root, permission, provider).map_err(|e| format!("PRESTART: {e}"))?;
    child.env("AAM_PERMISSION_MODE", permission);
    if provider == "claude" {
        child.env("AAM_DESKTOP_STREAM", "1");
        // Claude's interactive claude.ai login can be overridden by stale API
        // credentials inherited from a desktop launcher. Match the working CLI
        // login when the official auth status explicitly identifies claude.ai.
        let claude_ai_login = Command::new("claude").args(["auth", "status"])
            .output().ok().filter(|output| output.status.success())
            .and_then(|output| serde_json::from_slice::<serde_json::Value>(&output.stdout).ok())
            .map(|status| status.get("loggedIn").and_then(|v| v.as_bool()) == Some(true)
                && status.get("authMethod").and_then(|v| v.as_str()) == Some("claude.ai"))
            .unwrap_or(false);
        if claude_ai_login {
            child.env_remove("ANTHROPIC_API_KEY");
            child.env_remove("ANTHROPIC_AUTH_TOKEN");
        }
    }
    // Set only for the engine launched behind sandbox_command (Seatbelt/bwrap).
    if permission != "full-access" { child.env("AAM_OUTER_SANDBOX", "1"); } else { child.env_remove("AAM_OUTER_SANDBOX"); }
    child.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped()).kill_on_drop(true);
    #[cfg(unix)]
    isolate_process_group(&mut child);
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
    let mut reported_error = false;
    let mut reported_error_message = String::new();
    let mut observed_execution = false;
    let result = await_runtime(async {
        while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
                if event.get("kind").and_then(|v| v.as_str()) == Some("error") {
                    reported_error = true;
                    let message = event.get("message").and_then(|v| v.as_str()).unwrap_or("");
                    reported_error_message = message.chars().take(300).collect();
                    if classify_agent_error(message) == "rate_limit" {
                        reported_limit = true;
                    }
                }
                // Unknown events are treated as possible execution.
                let kind = event.get("kind").and_then(|v| v.as_str()).unwrap_or("");
                let activity_type = event.get("event").and_then(|v| v.get("type"))
                    .and_then(|v| v.as_str()).unwrap_or("");
                let pre_execution = match kind {
                    "provider" | "routing" | "error" => true,
                    "activity" => matches!(activity_type, "thread.started" | "turn.started"),
                    _ => false,
                };
                if !pre_execution { observed_execution = true; }
                let _ = app.emit("aam-event", serde_json::json!({"requestId": request_id, "data": event}));
            }
        }
        child.wait().await.map_err(|e| e.to_string())
    }, stop.clone()).await;
    if result.is_err() {
        #[cfg(unix)]
        kill_process_group(child.id());
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
    let stderr = if result.is_err() { stderr_task.abort(); String::new() } else { tokio::time::timeout(std::time::Duration::from_secs(2), stderr_task).await.ok().and_then(Result::ok).unwrap_or_default() };
    if reported_limit || classify_agent_error(&stderr) == "rate_limit" {
        set_provider_cooldown(provider, &app.state::<ProviderCooldowns>());
    }
    if reported_error || result.as_ref().is_ok_and(|status| !status.success()) {
        let message = if reported_limit || classify_agent_error(&stderr) == "rate_limit" {
            if reported_limit && !observed_execution && result.is_ok() {
                format!("PREEXEC: {provider} rate limit before task execution")
            } else {
                format!("{provider} rate limit detected; cooldown 5 minutes. Automatic replay blocked.")
            }
        } else if classify_agent_error(&stderr) == "auth_required" {
            format!("{provider} authentication required.")
        } else {
            // Avoid exposing stderr in UI: it can include prompts and secrets.
            // A local diagnostic log preserves the actual sandbox failure.
            let exit_status = result.as_ref().ok().map(|status| status.to_string()).unwrap_or_else(|| "interrupted".into());
            let stderr_diagnostic: String = stderr.lines()
                .filter(|line| !line.starts_with("[aam] Agent ready:"))
                .take(12).collect::<Vec<_>>().join("\\n");
            eprintln!("AAM {provider} engine failed (status={exit_status}, structured_error={reported_error_message:?}, observed_execution={observed_execution}): {stderr_diagnostic}");
            format!("{provider} engine exited unsuccessfully. See the Tauri terminal for sanitized diagnostics.")
        };
        return Err(message);
    }
    result.map(|_| ())
}

// Auto/Balanced advances on provider limits even after a process started.
// The next provider receives a continuation request to inspect existing changes,
// rather than blindly repeating the original instructions.
fn retry_after_failure(automatic: bool, _permission: &str, error: &str) -> bool {
    automatic && (retry_before_spawn(error) || error.starts_with("PREEXEC:") || error.contains("rate limit detected"))
}

fn retry_before_spawn(error: &str) -> bool {
    error.starts_with("PRESTART:")
}

// Reject automatic handoff when a provider may have modified repository files.
fn workspace_fingerprint(root: &Path) -> Result<Vec<u8>, String> {
    let status = Command::new("git").arg("-C").arg(root)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output().map_err(|e| format!("Cannot inspect workspace: {e}"))?;
    if !status.status.success() { return Err("Cannot inspect Git status".into()); }
    let diff = Command::new("git").arg("-C").arg(root)
        .args(["diff", "--binary", "HEAD", "--"])
        .output().map_err(|e| format!("Cannot inspect workspace diff: {e}"))?;
    if !diff.status.success() { return Err("Cannot inspect Git diff".into()); }
    let head = Command::new("git").arg("-C").arg(root)
        .args(["rev-parse", "HEAD"])
        .output().map_err(|e| format!("Cannot inspect Git HEAD: {e}"))?;
    if !head.status.success() { return Err("Cannot inspect Git HEAD".into()); }
    let mut fingerprint = head.stdout;
    fingerprint.extend_from_slice(&status.stdout);
    fingerprint.extend_from_slice(&diff.stdout);
    Ok(fingerprint)
}

// A provider handoff must account for edits made by the previous provider.
fn continuation_prompt(original: &str) -> String {
    format!("You are taking over an IN-PROGRESS task from another agent. Continue from the CURRENT workspace, not from scratch. Inspect git status, git diff (staged and unstaged), recent commits, and relevant files first. Preserve all existing edits, including uncommitted work. Determine what has already been completed; do not repeat completed edits or external side effects (publishing, deleting, payments, API writes). If an operation cannot be verified, ask for confirmation before repeating it. Keep the original project instructions and constraints. Original request:\n{original}")
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
    #[test]
    fn workspace_fingerprint_detects_partial_edits() {
        use std::process::Command;
        let root = std::env::temp_dir().join(format!("aam-handoff-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| Command::new("git").arg("-C").arg(&root)
            .args(args).status().unwrap().success();
        assert!(git(&["init", "-q"]));
        assert!(git(&["config", "user.email", "test@example.invalid"]));
        assert!(git(&["config", "user.name", "Test"]));
        std::fs::write(root.join("tracked.txt"), "before").unwrap();
        assert!(git(&["add", "."]));
        assert!(git(&["commit", "-qm", "baseline"]));
        let baseline = super::workspace_fingerprint(&root).unwrap();
        std::fs::write(root.join("tracked.txt"), "after").unwrap();
        assert_ne!(baseline, super::workspace_fingerprint(&root).unwrap());
        std::fs::write(root.join("tracked.txt"), "before").unwrap();
        assert_eq!(baseline, super::workspace_fingerprint(&root).unwrap());
        std::fs::write(root.join("new.txt"), "untracked").unwrap();
        assert_ne!(baseline, super::workspace_fingerprint(&root).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn continuation_preserves_original_request_and_requires_state_review() {
        let prompt = super::continuation_prompt("Update src/main.rs");
        assert!(prompt.contains("Inspect git status and diff first"));
        assert!(prompt.contains("do not repeat completed edits"));
        assert!(prompt.contains("Update src/main.rs"));
    }
    #[test]
    fn fallback_requires_proven_pre_execution_failure() {
        use super::retry_after_failure;
        assert!(retry_after_failure(true, "read-only", "PREEXEC: provider rejected request"));
        assert!(retry_after_failure(true, "workspace-write", "PRESTART: failed to spawn"));
        assert!(retry_after_failure(true, "workspace-write", "claude rate limit detected; cooldown 5 minutes. Automatic replay blocked."));
        assert!(retry_after_failure(true, "workspace-write", "copilot rate limit detected; cooldown 5 minutes. Continuing with a repository-state review on the next provider."));
        assert!(!retry_after_failure(false, "workspace-write", "claude rate limit detected; cooldown 5 minutes. Automatic replay blocked."));
        assert!(!retry_after_failure(true, "workspace-write", "CLI failed"));
        assert!(!retry_after_failure(true, "read-only", "CLI failed"));
        assert!(!retry_after_failure(false, "read-only", "CLI failed"));
        assert!(!retry_after_failure(true, "read-only", "Request cancelled"));
        assert!(!retry_after_failure(true, "read-only", "Request timed out after 10 minutes"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_permissions_limit_workspace_writes() {
        use super::macos_sandbox_profile;
        use std::path::Path;
        let root = Path::new("/tmp/aam-test");
        let read_only = macos_sandbox_profile(root, "read-only", "codex").unwrap();
        let write = macos_sandbox_profile(root, "workspace-write", "codex").unwrap();
        assert!(!read_only.contains(r#"(subpath "/tmp/aam-test")"#));
        assert!(write.contains(r#"(subpath "/tmp/aam-test")"#));
        assert!(macos_sandbox_profile(root, "full-access", "codex").is_err());
        let opencode = macos_sandbox_profile(root, "workspace-write", "opencode").unwrap();
        assert!(opencode.contains(".local/share/opencode"));
        assert!(opencode.contains(".cache/opencode"));
        assert!(!write.contains(".local/share/opencode"));
        let copilot = macos_sandbox_profile(root, "workspace-write", "copilot").unwrap();
        assert!(copilot.contains(".copilot"));
        assert!(!write.contains(".copilot"));
        assert!(!opencode.contains(".copilot"));

    }
    #[test]
    fn unsupported_permissions_fail_closed() {
        assert!(!super::external_sandbox_ready("full-access"));
        assert!(!super::external_sandbox_ready("unknown"));
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
    result
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StagedAttachment { name: String, path: String, size: u64, kind: String }

#[tauri::command]
fn stage_prompt_attachment(root: String, source: String) -> Result<StagedAttachment, String> {
    let workspace = Path::new(&root).canonicalize().map_err(|e| format!("Workspace: {e}"))?;
    if !workspace.is_dir() || !workspace.join(".git").exists() { return Err("Select a Git workspace first".into()); }
    let source = Path::new(&source).canonicalize().map_err(|e| format!("Attachment: {e}"))?;
    if !source.is_file() { return Err("Attachment must be a regular file".into()); }
    let name = source.file_name().and_then(|v| v.to_str()).ok_or("Invalid attachment name")?.to_string();
    let extension = source.extension().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase();
    let kind = if ["png","jpg","jpeg","webp"].contains(&extension.as_str()) { "image" }
        else if ["pdf","txt","md","json","yaml","yml","xml","csv","kt","kts","swift","ts","tsx","js","jsx","py","rs","java","go","html","css","sh","toml"].contains(&extension.as_str()) { "document" }
        else { return Err("Unsupported attachment format".into()); };
    let size = source.metadata().map_err(|e| e.to_string())?.len();
    if size == 0 || size > 20 * 1024 * 1024 { return Err("Attachment must be between 1 byte and 20 MiB".into()); }
    let folder = workspace.join(".git").join("ai-agent-manager").join("attachments");
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let unique = format!("{}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos(), name);
    let target = folder.join(unique);
    std::fs::copy(&source, &target).map_err(|e| format!("Cannot stage attachment: {e}"))?;
    Ok(StagedAttachment { name, path: target.to_string_lossy().into_owned(), size, kind: kind.into() })
}

#[tauri::command]
async fn run_prompt(app: tauri::AppHandle, request_id: String, root: String, provider: String, prompt: String, permission: String) -> Result<(), String> {
    let end_to_end_started = std::time::Instant::now();
    if !["claude", "codex", "antigravity", "opencode", "copilot", "auto", "balanced"].contains(&provider.as_str()) { return Err("Invalid provider".into()); }
    if prompt.trim().is_empty() { return Err("Prompt is empty".into()); }
    if !["read-only", "workspace-write", "full-access"].contains(&permission.as_str()) { return Err("Invalid permission mode".into()); }
    let path = Path::new(&root).canonicalize().map_err(|e| format!("Workspace: {e}"))?;
    if !path.is_dir() { return Err("Workspace is not a directory".into()); }
    let workspace_validation_ms = end_to_end_started.elapsed().as_millis();
    let git_started = std::time::Instant::now();
    let status = Command::new("git").args(["-C", path.to_str().ok_or("Invalid path")?, "rev-parse", "--is-inside-work-tree"]).output().map_err(|e| format!("Git: {e}"))?;
    let git_validation_ms = git_started.elapsed().as_millis();
    if !status.status.success() { return Err("Workspace must be a Git repository".into()); }
    let request_started = std::time::Instant::now();
    let automatic = provider == "auto" || provider == "balanced";
    let candidates: Vec<&str> = if automatic {
        route_candidates(&prompt, &provider, &permission)
    } else {
        vec![provider.as_str()]
    };
    let routing_ms = request_started.elapsed().as_millis();
    let mut last_preflight_error = String::from("No compatible agent CLI available");
    let mut continue_after_limit = false;
    for candidate in candidates {
        let Some(adapter) = provider_adapter(candidate) else { continue; };
        if automatic && !provider_available(candidate, &app.state::<ProviderCooldowns>()) {
            continue;
        }
        if let Err(message) = validate_execution(adapter, &permission) {
            last_preflight_error = message;
            continue;
        }
        let preflight_started = std::time::Instant::now();
        if verified_auth_probe(candidate).await == Some(false) {
            last_preflight_error = format!("{candidate} requires authentication");
            continue;
        }
        let preflight_ms = preflight_started.elapsed().as_millis();
        let _ = app.emit("aam-event", serde_json::json!({"requestId":request_id,"data":{
            "kind":"routing","reason":format!("Executing {candidate}; preflight passed"),
            "provider":candidate,"model":"CLI default"
        }}));
        // Provider limits trigger a continuation on the next available provider.
        let handoff_prompt;
        let active_prompt = if continue_after_limit {
            handoff_prompt = continuation_prompt(&prompt);
            handoff_prompt.as_str()
        } else { prompt.as_str() };
        let fingerprint_started = std::time::Instant::now();
        let before = workspace_fingerprint(&path)?;
        let fingerprint_ms = fingerprint_started.elapsed().as_millis();
        let execution_started = std::time::Instant::now();
        let outcome = execute_adapter(&app, &request_id, &path, adapter, active_prompt, &permission).await;
        let _ = app.emit("aam-event", serde_json::json!({"requestId":request_id,"data":{
            "kind":"timing","provider":candidate,
            "preflightMs":preflight_ms,
            "executionMs":execution_started.elapsed().as_millis(),
            "totalMs":end_to_end_started.elapsed().as_millis(),
            "workspaceValidationMs":workspace_validation_ms,
            "gitValidationMs":git_validation_ms,
            "routingMs":routing_ms,
            "fingerprintMs":fingerprint_ms,
            "backendElapsedMs":end_to_end_started.elapsed().as_millis(),
            "validationMs":workspace_validation_ms + git_validation_ms,
            "preExecutionMs":end_to_end_started.elapsed().as_millis().saturating_sub(execution_started.elapsed().as_millis())
        }}));
        match outcome {
            Err(message) if retry_after_failure(automatic, &permission, &message) => {
                if message.contains("rate limit detected") {
                    let after = workspace_fingerprint(&path)?;
                    // Partial edits are a reason to inspect state, not a reason to
                    // prohibit switching agents. Never replay the original task.
                    continue_after_limit = true;
                    if before != after {
                        let _ = app.emit("aam-event", serde_json::json!({"requestId":request_id,"data":{
                            "kind":"routing","provider":candidate,
                            "reason":"Workspace changed during previous agent run; switching with a state-review continuation, not replaying the task"
                        }}));
                    }
                }
                last_preflight_error = message;
                let _ = app.emit("aam-event", serde_json::json!({"requestId":request_id,"data":{
                    "kind":"routing","provider":candidate,
                    "reason":"Provider unavailable; continuing with next available provider"
                }}));
                continue;
            }
            outcome => {
                if let Err(ref message) = outcome {
                    let _ = app.emit("aam-event", serde_json::json!({"requestId":request_id,"data":{
                        "kind":"process_error","provider":candidate,"message":message
                    }}));
                }
                return outcome;
            },
        }
    }
    let _ = app.emit("aam-event", serde_json::json!({"requestId":request_id,"data":{
        "kind":"process_error","message":last_preflight_error
    }}));
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

#[tauri::command]
fn relevant_prompt_files(root: String, prompt: String) -> Result<Vec<String>, String> {
    let root = Path::new(&root).canonicalize().map_err(|e| e.to_string())?;
    let output = Command::new("git").arg("-C").arg(&root).args(["ls-files", "-z"])
        .output().map_err(|e| e.to_string())?;
    if !output.status.success() { return Err("Not a Git repository".into()); }
    let terms: Vec<String> = prompt.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|t| t.len() >= 4).map(|t| t.to_ascii_lowercase()).collect();
    let mut matches: Vec<(usize,String)> = output.stdout.split(|b| *b == 0).filter_map(|bytes| {
        let path = String::from_utf8(bytes.to_vec()).ok()?;
        let lower = path.to_ascii_lowercase();
        if [".env", "secret", "credential", "node_modules/", "build/", "dist/"].iter().any(|x| lower.contains(x)) { return None; }
        let score = terms.iter().filter(|term| lower.contains(term.as_str())).count();
        if score == 0 { None } else { Some((score,path)) }
    }).collect();
    matches.sort_by(|a,b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    Ok(matches.into_iter().take(8).map(|(_,path)| path).collect())
}

#[tauri::command]
fn clear_agent_session(state: tauri::State<'_, RunningRequests>, root: String) -> Result<(), String> {
    if !state.0.lock().map_err(|_| "Request state unavailable")?.is_empty() {
        return Err("Stop the running agent before clearing its session".into());
    }
    let root = Path::new(&root).canonicalize().map_err(|e| format!("Workspace: {e}"))?;
    let git_dir = Command::new("git").arg("-C").arg(&root)
        .args(["rev-parse", "--absolute-git-dir"]).output()
        .map_err(|e| format!("Git: {e}"))?;
    if !git_dir.status.success() { return Err("Workspace must be a Git repository".into()); }
    let git_path = String::from_utf8(git_dir.stdout).map_err(|e| e.to_string())?;
    let session = Path::new(git_path.trim()).join("ai-agent-manager").join("agent-bridge.json");
    if session.exists() {
        std::fs::remove_file(&session).map_err(|e| format!("Cannot clear session: {e}"))?;
    }
    Ok(())
}

fn main() {
    configure_agent_path();
    tauri::Builder::default().manage(RunningRequests::default()).manage(LoginSession::default())
        .manage(ProviderCooldowns::default()).manage(approval::ApprovalRegistry::default()).manage(interactive::InteractiveSessions::default()).plugin(tauri_plugin_dialog::init()).invoke_handler(tauri::generate_handler![interactive::start_interactive_cli, interactive::interactive_cli_input, interactive::interrupt_interactive_cli, stage_prompt_attachment, relevant_prompt_files, clear_agent_session, decide_agent_approval, run_prompt, github_repositories, github_clone, project_ai_config, desktop_diagnostics, provider_setup_status, node_setup_status, install_node_runtime, install_provider_cli, open_provider_login, login_pty_write, focus_after_login, cancel_provider_login, cancel_prompt, architect_preview, architect_apply])
        .run(tauri::generate_context!()).expect("Tauri application failed");
}
