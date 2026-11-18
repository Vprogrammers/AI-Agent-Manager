use std::{fs, path::{Path, PathBuf}};
use serde::{Serialize, Deserialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Proposal {
    pub path: String,
    pub action: String,
    pub reason: String,
    pub before: Option<String>,
    pub after: String,
}

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct ProjectPlan {
    pub stack: Vec<String>,
    pub proposals: Vec<Proposal>,
}

fn exists(root: &Path, names: &[&str]) -> bool {
    names.iter().any(|name| root.join(name).exists())
}

fn stack(root: &Path) -> Vec<String> {
    let mut tags = Vec::new();
    for (name, files) in [
        ("Kotlin Multiplatform", vec!["settings.gradle.kts", "shared/build.gradle.kts"]),
        ("Android", vec!["app/build.gradle.kts", "androidApp/build.gradle.kts"]),
        ("Swift / iOS", vec!["Package.swift", "iosApp", "*.xcodeproj"]),
        ("Rust", vec!["Cargo.toml"]),
        ("Python", vec!["pyproject.toml", "requirements.txt", "src/ai_agent_manager.py"]),
        ("TypeScript", vec!["tsconfig.json", "desktop/tsconfig.json"]),
        ("Node.js", vec!["package.json", "desktop/package.json"]),
        ("Firebase", vec!["firebase.json", ".firebaserc"]),
        ("Tauri", vec!["src-tauri/tauri.conf.json", "desktop/src-tauri/tauri.conf.json"]),
        ("Flutter", vec!["pubspec.yaml"]),
    ] {
        if exists(root, &files) { tags.push(name.to_string()); }
    }
    if root.join(".github/workflows").is_dir() { tags.push("GitHub Actions".into()); }
    if tags.is_empty() { tags.push("General software project".into()); }
    tags
}

fn safe_file(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.starts_with('/') || relative.contains("..") || relative.contains('\\') { return Err("Invalid proposal path".into()); }
    let path = root.join(relative);
    let mut current = root.to_path_buf();
    for part in relative.split('/') {
        if part.is_empty() || part == "." { return Err("Invalid path component".into()); }
        current.push(part);
        if current.is_symlink() { return Err("Symlink in configuration path is not allowed".into()); }
    }
    Ok(path)
}

fn propose(root: &Path, path: &str, content: String, reason: &str) -> Result<Proposal, String> {
    let target = safe_file(root, path)?;
    let before = if target.exists() {
        if !target.is_file() { return Err(format!("{path} is not a file")); }
        let metadata = fs::metadata(&target).map_err(|e| e.to_string())?;
        if metadata.len() > 128 * 1024 { return Err(format!("{path} exceeds safe configuration size")); }
        Some(fs::read_to_string(&target).map_err(|e| format!("{path}: {e}"))?)
    } else { None };
    let after = match &before {
        None => content,
        Some(old) if old.contains("<!-- AAM:BEGIN -->") && old.contains("<!-- AAM:END -->") => {
            let start = old.find("<!-- AAM:BEGIN -->").unwrap();
            let end = old[start..].find("<!-- AAM:END -->").unwrap() + start + "<!-- AAM:END -->".len();
            format!("{}{}{}", &old[..start], content.trim_end(), &old[end..])
        },
        Some(old) => format!("{}{}{}", old, if old.ends_with('\n') { "\n" } else { "\n\n" }, content),
    };
    Ok(Proposal { path: path.into(), action: if before.is_some() {"update"} else {"create"}.into(), reason: reason.into(), before, after })
}

fn managed(title: &str, stack: &[String]) -> String {
    format!("<!-- AAM:BEGIN -->\n## {title}\n\nDetected stack: {}.\n\n- Follow existing project conventions and user-owned instructions; inspect before changing code.\n- Use relevant project skills only for the current task; do not load every skill into each prompt.\n- Verify changes with the project's available build and test commands; never claim unrun tests passed.\n- Protect secrets and user changes; request approval for destructive Git or filesystem operations.\n- Preserve native agent session context when possible; avoid duplicating context in handoffs.\n<!-- AAM:END -->\n", stack.join(", "))
}

fn skill(name: &str, description: &str, steps: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\n{steps}\n")
}

pub fn plan(root: &Path) -> Result<ProjectPlan, String> {
    let root = root.canonicalize().map_err(|e| format!("Workspace: {e}"))?;
    if !root.is_dir() { return Err("Not a directory".into()); }
    let tags = stack(&root);
    let mut candidates = vec![
        ("AGENTS.md".to_string(), managed("Project instructions for Codex", &tags), "Codex project guidance".to_string()),
        ("CLAUDE.md".to_string(), managed("Project instructions for Claude Code", &tags), "Claude Code project guidance".to_string()),
    ];
    let workflow = "1. Inspect the current repository structure, configuration and existing instructions.\n2. Identify the smallest safe change and the relevant modules.\n3. Implement using established conventions, then run targeted tests.\n4. Report actual results, unresolved risks and changed files.";
    let steps = format!("{}\n\nProject technologies: {}.", workflow, tags.join(", "));
    for (folder, provider) in [(".claude/skills", "Claude"), (".agents/skills", "Codex")] {
        candidates.push((format!("{folder}/project-workflow/SKILL.md"),
            skill("project-workflow", &format!("Repository-specific implementation and verification workflow for {provider}."), &steps),
            format!("{provider} on-demand project skill")));
    }
    let agent = format!("---\nname: project-reviewer\ndescription: Review repository changes against project architecture, safety and tests.\ntools: Read, Glob, Grep\n---\n\nReview the diff against the project's existing instructions and conventions. Stack: {}. Report concrete defects and unverified assumptions. Do not modify files.\n", tags.join(", "));
    candidates.push((".claude/agents/project-reviewer.md".into(), agent, "Claude read-only review subagent".into()));
    let mut proposals = Vec::new();
    for (path, content, reason) in candidates {
        let p = propose(&root, &path, content, &reason)?;
        if p.before.as_deref() != Some(p.after.as_str()) { proposals.push(p); }
    }
    Ok(ProjectPlan { stack: tags, proposals })
}

pub fn apply(root: &Path, selected: Vec<Proposal>) -> Result<Vec<String>, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let current = plan(&root)?;
    if selected.is_empty() { return Err("No proposals selected".into()); }
    // Validate every selected proposal before touching the filesystem.
    for item in &selected {
        let expected = current.proposals.iter().find(|p| p.path == item.path)
            .ok_or_else(|| format!("Proposal is no longer valid: {}", item.path))?;
        if expected.before != item.before || expected.after != item.after || expected.action != item.action {
            return Err(format!("Configuration changed since preview: {}. Rescan before applying.", item.path));
        }
        let _ = safe_file(&root, &item.path)?;
    }
    let mut written = Vec::new();
    for item in selected {
        let path = safe_file(&root, &item.path)?;
        if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        // Re-check after directory creation to avoid replacing existing files.
        if path.exists() {
            let current_text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            if item.before.as_deref() != Some(current_text.as_str()) { return Err(format!("Changed while applying: {}", item.path)); }
            fs::write(&path, &item.after).map_err(|e| e.to_string())?;
        } else {
            if item.before.is_some() { return Err(format!("Missing during apply: {}", item.path)); }
            use std::io::Write;
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| e.to_string())?;
            file.write_all(item.after.as_bytes()).map_err(|e| e.to_string())?;
        }
        written.push(item.path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_traversal() {
        let root = std::env::temp_dir();
        assert!(safe_file(&root, "../outside").is_err());
        assert!(safe_file(&root, ".claude/../outside").is_err());
    }
    #[test]
    fn managed_block_preserves_user_text() {
        let old = "# Custom user rules\n\n<!-- AAM:BEGIN -->\nold\n<!-- AAM:END -->\n\nKeep this.";
        let start = old.find("<!-- AAM:BEGIN -->").unwrap();
        let end = old.find("<!-- AAM:END -->").unwrap()+"<!-- AAM:END -->".len();
        let new = format!("{}{}{}", &old[..start], managed("Updated", &["Rust".into()]).trim_end(), &old[end..]);
        assert!(new.contains("# Custom user rules"));
        assert!(new.contains("Keep this."));
        assert!(new.contains("Detected stack: Rust."));
    }
}
