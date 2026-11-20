//! Provider-native interactive sessions. PTY delivers original CLI permission prompts
//! without guessing approval semantics from human-readable terminal output.
use std::{collections::HashMap, io::{Read, Write}, path::Path, sync::Mutex};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

struct Session {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
}
#[derive(Default)]
pub struct InteractiveSessions(Mutex<HashMap<String, Session>>);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TerminalEvent {
    request_id: String,
    text: String,
    finished: bool,
}

fn provider_binary(provider: &str) -> Result<&'static str, String> {
    match provider {
        "claude" => Ok("claude"),
        "codex" => Ok("codex"),
        "opencode" => Ok("opencode"),
        "copilot" => Ok("copilot"),
        "antigravity" => Ok("agy"),
        _ => Err("Unknown provider".into()),
    }
}

fn provider_command(
    executable: &Path,
    workspace: &Path,
    provider: &str,
    permission: &str,
) -> Result<CommandBuilder, String> {
    if permission == "full-access" {
        return Ok(CommandBuilder::new(executable));
    }
    if !super::external_sandbox_ready(permission) {
        return Err("OS sandbox unavailable; refusing an unsandboxed CLI".into());
    }
    #[cfg(target_os = "linux")]
    {
        let mut command = CommandBuilder::new("bwrap");
        for arg in ["--die-with-parent", "--unshare-pid", "--new-session",
            "--ro-bind", "/", "/", "--dev-bind", "/dev", "/dev",
            "--proc", "/proc", "--tmpfs", "/tmp"] {
            command.arg(arg);
        }
        if permission == "workspace-write" {
            command.arg("--bind");
            command.arg(workspace);
            command.arg(workspace);
        }
        command.arg("--chdir");
        command.arg(workspace);
        command.arg("--");
        command.arg(executable);
        Ok(command)
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = CommandBuilder::new("sandbox-exec");
        command.arg("-p");
        command.arg(super::macos_sandbox_profile(workspace, permission, provider)?);
        command.arg(executable);
        Ok(command)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (executable, workspace, provider);
        Err("Interactive sandbox unavailable on this OS".into())
    }
}

/// Native provider permission flags, never inferred from terminal output.
/// Unsupported provider/permission combinations retain their interactive prompts.
fn permission_args(provider: &str, permission: &str) -> Vec<&'static str> {
    match (provider, permission) {
        ("claude", "read-only") => vec!["--permission-mode", "plan"],
        ("claude", "workspace-write") => vec!["--permission-mode", "acceptEdits"],
        ("claude", "full-access") => vec!["--permission-mode", "bypassPermissions"],
        ("codex", "read-only") => vec!["--sandbox", "read-only", "--ask-for-approval", "on-request"],
        ("codex", "workspace-write") => vec!["--sandbox", "workspace-write", "--ask-for-approval", "on-request"],
        ("codex", "full-access") => vec!["--sandbox", "danger-full-access", "--ask-for-approval", "never"],
        // Copilot can auto-approve its tools within an OS-enforced workspace sandbox.
        // The read-only mode retains provider permission prompts.
        ("copilot", "workspace-write") => vec!["--allow-all-tools"],
        ("copilot", "full-access") => vec!["--allow-all-tools", "--allow-all-paths"],
        // OpenCode / Antigravity retain the native CLI permission configuration:
        // do not invent undocumented flags or assume their persisted rules are safe.
        _ => vec![],
    }
}

#[tauri::command]
pub fn start_interactive_cli(
    app: AppHandle,
    sessions: tauri::State<'_, InteractiveSessions>,
    request_id: String,
    provider: String,
    root: String,
    permission: String,
) -> Result<(), String> {
    if request_id.is_empty() || request_id.len() > 128 ||
        !request_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("Invalid request ID".into());
    }
    if !["read-only", "workspace-write", "full-access"].contains(&permission.as_str()) {
        return Err("Unknown permission mode".into());
    }
    let executable = super::find_cli(provider_binary(&provider)?)
        .ok_or("Provider CLI not found in PATH")?;
    let workspace = Path::new(&root).canonicalize().map_err(|e| e.to_string())?;
    if !workspace.is_dir() || !workspace.join(".git").exists() {
        return Err("A Git workspace is required".into());
    }
    let mut guard = sessions.0.lock().map_err(|_| "Terminal state unavailable")?;
    if guard.contains_key(&request_id) { return Err("Request already running".into()); }
    let pty = native_pty_system().openpty(PtySize {
        rows: 30, cols: 110, pixel_width: 0, pixel_height: 0,
    }).map_err(|e| format!("PTY unavailable: {e}"))?;
    let mut cmd = provider_command(&executable, &workspace, &provider, &permission)?;
    // The left panel selects the maximum approved scope. Configure each
    // provider's OWN approval engine so approved work proceeds automatically;
    // the OS sandbox remains authoritative in restricted modes.
    for arg in permission_args(&provider, &permission) {
        cmd.arg(arg);
    }
    cmd.cwd(&workspace);
    // Do not inject bypass flags or non-interactive -p flags: the CLI owns its
    // permission prompts, and this process remains alive while the user decides.
    let mut child = pty.slave.spawn_command(cmd)
        .map_err(|e| format!("CLI failed to start: {e}"))?;
    drop(pty.slave);
    let writer = pty.master.take_writer().map_err(|e| e.to_string())?;
    let mut reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
    guard.insert(request_id.clone(), Session { writer, master: pty.master });
    drop(guard);
    let app_clone = app.clone();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let _ = app.emit("interactive-cli-output", TerminalEvent {
                        request_id: request_id.clone(),
                        text: String::from_utf8_lossy(&buffer[..n]).into_owned(),
                        finished: false,
                    });
                }
            }
        }
        let _ = child.wait();
        if let Some(state) = app_clone.try_state::<InteractiveSessions>() {
            if let Ok(mut entries) = state.0.lock() { entries.remove(&request_id); }
        }
        let _ = app.emit("interactive-cli-output", TerminalEvent {
            request_id, text: String::new(), finished: true,
        });
    });
    Ok(())
}

#[tauri::command]
pub fn interactive_cli_input(
    sessions: tauri::State<'_, InteractiveSessions>,
    request_id: String,
    input: String,
) -> Result<(), String> {
    if input.len() > 32768 { return Err("Terminal input exceeds 32 KiB".into()); }
    let mut entries = sessions.0.lock().map_err(|_| "Terminal state unavailable")?;
    let session = entries.get_mut(&request_id).ok_or("Terminal is not running")?;
    session.writer.write_all(input.as_bytes()).map_err(|e| e.to_string())?;
    session.writer.flush().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn resize_interactive_cli(
    sessions: tauri::State<'_, InteractiveSessions>,
    request_id: String,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    if !(5..=200).contains(&rows) || !(20..=400).contains(&cols) {
        return Err("Unsupported terminal dimensions".into());
    }
    let entries = sessions.0.lock().map_err(|_| "Terminal state unavailable")?;
    let session = entries.get(&request_id).ok_or("Terminal is not running")?;
    session.master.resize(PtySize {
        rows, cols, pixel_width: 0, pixel_height: 0,
    }).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn interrupt_interactive_cli(
    sessions: tauri::State<'_, InteractiveSessions>,
    request_id: String,
) -> Result<(), String> {
    let mut entries = sessions.0.lock().map_err(|_| "Terminal state unavailable")?;
    let session = entries.get_mut(&request_id).ok_or("Terminal is not running")?;
    session.writer.write_all(&[3]).map_err(|e| e.to_string())?;
    session.writer.flush().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_cli_providers_are_allowed() {
        for name in ["claude", "codex", "opencode", "copilot", "antigravity"] {
            assert!(provider_binary(name).is_ok());
        }
        assert!(provider_binary("sh").is_err());
        assert!(provider_binary("claude; echo unsafe").is_err());
        assert!(permission_args("claude", "workspace-write").contains(&"acceptEdits"));
        assert!(permission_args("codex", "workspace-write").contains(&"on-request"));
        assert!(!permission_args("codex", "read-only").contains(&"never"));
        assert!(permission_args("copilot", "workspace-write").contains(&"--allow-all-tools"));
    }
}
