#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{path::{Path, PathBuf}, process::Command};

fn aam_executable() -> Result<PathBuf, String> {
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
    Err("AAM CLI not found. Install it with scripts/install.sh, then restart the app.".into())
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


#[tauri::command]
async fn run_prompt(app: tauri::AppHandle, request_id: String, root: String, provider: String, prompt: String, permission: String) -> Result<(), String> {
    if provider != "claude" && provider != "codex" { return Err("Invalid provider".into()); }
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
    let result = tokio::time::timeout(std::time::Duration::from_secs(600), async {
        while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
            if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
                let _ = app.emit("aam-event", serde_json::json!({"requestId": request_id, "data": event}));
            }
        }
        child.wait().await.map_err(|e| e.to_string())
    }).await.map_err(|_| "Request timed out after 10 minutes; inspect changes before retrying".to_string())??;
    let stderr = stderr_task.await.unwrap_or_default();
    if !result.success() {
        let _ = app.emit("aam-event", serde_json::json!({"requestId": request_id, "data": {"kind":"process_error", "message":stderr}}));
    }
    Ok(())
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
    tauri::Builder::default().plugin(tauri_plugin_dialog::init()).invoke_handler(tauri::generate_handler![run_prompt, github_repositories, github_clone, project_ai_config])
        .run(tauri::generate_context!()).expect("Tauri application failed");
}
