#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{path::{Path, PathBuf}, process::Command, sync::{Arc,Mutex}};
use std::collections::HashMap;
use tauri::Manager;

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

#[tauri::command]
fn cancel_prompt(state: tauri::State<'_, RunningRequests>, request_id: String) -> Result<(), String> {
    let requests = state.0.lock().map_err(|_|"Request state unavailable")?;
    let signal = requests.get(&request_id).ok_or("Request is no longer running")?;
    signal.notify_one();
    Ok(())
}

#[tauri::command]
async fn run_prompt(app: tauri::AppHandle, request_id: String, root: String, provider: String, prompt: String, permission: String) -> Result<(), String> {
    if !["claude", "codex", "auto", "balanced"].contains(&provider.as_str()) { return Err("Invalid provider".into()); }
    if prompt.trim().is_empty() { return Err("Prompt is empty".into()); }
    if !["read-only", "workspace-write", "full-access"].contains(&permission.as_str()) { return Err("Invalid permission mode".into()); }
    let path = Path::new(&root).canonicalize().map_err(|e| format!("Workspace: {e}"))?;
    if !path.is_dir() { return Err("Workspace is not a directory".into()); }
    let status = Command::new("git").args(["-C", path.to_str().ok_or("Invalid path")?, "rev-parse", "--is-inside-work-tree"]).output().map_err(|e| format!("Git: {e}"))?;
    if !status.status.success() { return Err("Workspace must be a Git repository".into()); }
    let mut child = tokio::process::Command::new(aam_executable()?);
    child.env("AAM_PERMISSION_MODE", &permission);
    child.arg(&provider).arg("--root").arg(&path).arg("--desktop-jsonl")
        .arg("--ui").arg("classic").arg("--prompt").arg(&prompt)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped()).kill_on_drop(true);
    let mut child = child.spawn().map_err(|e| format!("Cannot launch aam: {e}"))?;
    let stop = Arc::new(tokio::sync::Notify::new());
    {
        let state = app.state::<RunningRequests>();
        state.0.lock().map_err(|_|"Request state unavailable")?.insert(request_id.clone(),stop.clone());
    }
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
    let mut cancelled = false;
    let result = tokio::select! {
        outcome = tokio::time::timeout(std::time::Duration::from_secs(600), async {
            while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
                    let _ = app.emit("aam-event", serde_json::json!({"requestId": request_id, "data": event}));
                }
            }
            child.wait().await.map_err(|e| e.to_string())
        }) => outcome.map_err(|_|"Request timed out after 10 minutes".to_string()).and_then(|r|r),
        _ = stop.notified() => { cancelled = true; Err("Request stopped by user".to_string()) }
    };
    if cancelled || result.is_err() {
        // kill_on_drop terminates the AAM process; the engine's subprocess handling
        // may need separate process-group cleanup on each supported OS.
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
    {
        let state = app.state::<RunningRequests>();
        if let Ok(mut requests) = state.0.lock() { requests.remove(&request_id); };
    }
    if let Err(message) = result.as_ref() {
        let _ = app.emit("aam-event", serde_json::json!({"requestId": request_id, "data": {"kind":"process_error", "message":message}}));
    }
    let stderr = if result.is_err() { stderr_task.abort(); String::new() } else { tokio::time::timeout(std::time::Duration::from_secs(2), stderr_task).await.ok().and_then(Result::ok).unwrap_or_default() };
    if result.as_ref().is_ok_and(|status| !status.success()) {
        let _ = app.emit("aam-event", serde_json::json!({"requestId": request_id, "data": {"kind":"process_error", "message":stderr}}));
    }
    result.map(|_| ())
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

fn main() {
    configure_agent_path();
    tauri::Builder::default().manage(RunningRequests::default()).plugin(tauri_plugin_dialog::init()).invoke_handler(tauri::generate_handler![run_prompt, github_repositories, github_clone, project_ai_config, desktop_diagnostics, cancel_prompt])
        .run(tauri::generate_context!()).expect("Tauri application failed");
}
