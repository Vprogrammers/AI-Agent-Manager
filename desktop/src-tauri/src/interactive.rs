//! Native interactive CLI transport. The original provider CLI owns its prompts,
//! tool approvals and session state; the desktop only relays terminal I/O.
//! This mode is explicitly full-access, never a substitute for a sandbox.
use std::{collections::HashMap, io::{Read, Write}, path::Path, sync::Mutex};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

type Writer = Box<dyn Write + Send>;
#[derive(Default)]
pub struct InteractiveSessions(Mutex<HashMap<String, Writer>>);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TerminalEvent {
    request_id: String,
    text: String,
    finished: bool,
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
    if request_id.len() > 128 || request_id.is_empty() || !request_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("Invalid request ID".into());
    }
    // Never silently elevate a read-only/workspace-write request to full access.
    if permission != "full-access" {
        return Err("Native interactive CLI requires explicit Full access. Other modes remain sandboxed.".into());
    }
    let binary = match provider.as_str() {
        "claude" => "claude",
        "codex" => "codex",
        "opencode" => "opencode",
        "copilot" => "copilot",
        "antigravity" => "agy",
        _ => return Err("Unknown provider".into()),
    };
    let workspace = Path::new(&root).canonicalize().map_err(|e| e.to_string())?;
    if !workspace.is_dir() || !workspace.join(".git").exists() {
        return Err("A Git workspace is required".into());
    }
    let executable = super::find_cli(binary).ok_or("CLI not found in PATH")?;
    let mut sessions_guard = sessions.0.lock().map_err(|_| "Terminal state unavailable")?;
    if sessions_guard.contains_key(&request_id) {
        return Err("Terminal already running".into());
    }
    let pty = native_pty_system().openpty(PtySize {
        rows: 30, cols: 110, pixel_width: 0, pixel_height: 0
    }).map_err(|e| format!("PTY: {e}"))?;
    let mut cmd = CommandBuilder::new(executable);
    cmd.cwd(workspace);
    // Use the real interactive CLI: no -p/--print, no --no-ask-user,
    // no bypassPermissions, no fabricated tool approvals.
    let mut child = pty.slave.spawn_command(cmd).map_err(|e| format!("CLI launch: {e}"))?;
    drop(pty.slave);
    let writer = pty.master.take_writer().map_err(|e| e.to_string())?;
    let mut reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
    sessions_guard.insert(request_id.clone(), writer);
    drop(sessions_guard);
    let handle = app.clone();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 2048];
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
        if let Some(state) = handle.try_state::<InteractiveSessions>() {
            if let Ok(mut guard) = state.0.lock() { guard.remove(&request_id); }
        }
        let _ = app.emit("interactive-cli-output", TerminalEvent {
            request_id, text: String::new(), finished: true
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
    if input.len() > 8192 { return Err("Terminal input exceeds 8 KiB".into()); }
    let mut guard = sessions.0.lock().map_err(|_| "Terminal state unavailable")?;
    let writer = guard.get_mut(&request_id).ok_or("Terminal session not running")?;
    writer.write_all(input.as_bytes()).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn interrupt_interactive_cli(
    sessions: tauri::State<'_, InteractiveSessions>,
    request_id: String,
) -> Result<(), String> {
    let mut guard = sessions.0.lock().map_err(|_| "Terminal state unavailable")?;
    let writer = guard.get_mut(&request_id).ok_or("Terminal session not running")?;
    writer.write_all(&[3]).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}
