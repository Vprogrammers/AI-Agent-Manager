#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{path::Path, process::Command};

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
    tauri::Builder::default().plugin(tauri_plugin_dialog::init()).invoke_handler(tauri::generate_handler![run_prompt])
        .run(tauri::generate_context!()).expect("Tauri application failed");
}
