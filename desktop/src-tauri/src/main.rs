#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{path::Path, process::Command};
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
fn run_prompt(root: String, provider: String, prompt: String) -> Result<String, String> {
    if provider != "claude" && provider != "codex" { return Err("Invalid provider".into()); }
    if prompt.trim().is_empty() { return Err("Prompt is empty".into()); }
    let path = Path::new(&root).canonicalize().map_err(|e| format!("Workspace: {e}"))?;
    if !path.is_dir() { return Err("Workspace is not a directory".into()); }
    let status = Command::new("git").args(["-C", path.to_str().ok_or("Invalid path")?, "rev-parse", "--is-inside-work-tree"]).output().map_err(|e| format!("Git: {e}"))?;
    if !status.status.success() { return Err("Workspace must be a Git repository".into()); }
    let output = Command::new("aam").arg(&provider).arg("--root").arg(&path)
        .arg("--ui").arg("classic").arg("--prompt").arg(&prompt)
        .output().map_err(|e| format!("Cannot launch aam: {e}. Install the AAM CLI and ensure it is on PATH."))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if output.status.success() { Ok(stdout) } else {
        Err(format!("AAM exited with {}: {}", output.status, String::from_utf8_lossy(&output.stderr)))
    }
}
fn main() {
    tauri::Builder::default().plugin(tauri_plugin_dialog::init()).invoke_handler(tauri::generate_handler![run_prompt, github_repositories, github_clone])
        .run(tauri::generate_context!()).expect("Tauri application failed");
}
