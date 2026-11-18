"""Conservative project architecture scanner and review-before-apply generator."""
import argparse
import difflib
import json
import os
from pathlib import Path

IGNORED = {".git", "node_modules", ".gradle", "build", "dist", ".venv", "venv", ".idea"}
MARKERS = {
    "Kotlin Multiplatform": ("settings.gradle.kts", "shared/src/commonMain"),
    "Android": ("app/src/main", "androidApp/src/main"),
    "iOS": ("iosApp", "Package.swift"),
    "Python": ("pyproject.toml", "requirements.txt"),
    "JavaScript/TypeScript": ("package.json", "tsconfig.json"),
    "Rust": ("Cargo.toml",),
}
MANAGED = {
    ".ai-agent-manager/architect/PROJECT.md",
    ".ai-agent-manager/architect/agents/project-reviewer.md",
    ".ai-agent-manager/architect/skills/project-workflow/SKILL.md",
}
MAX_FILES = 12000


def scan(root):
    root = Path(root).resolve()
    if not (root / ".git").exists() and not (root / ".git").is_file():
        raise ValueError("Target must be a Git checkout")
    paths = []
    for directory, dirs, files in os.walk(root, followlinks=False):
        dirs[:] = sorted(d for d in dirs if d not in IGNORED and not d.startswith("."))
        for name in sorted(files):
            file = Path(directory) / name
            if file.is_symlink():
                continue
            paths.append(file.relative_to(root).as_posix())
            if len(paths) >= MAX_FILES:
                break
        if len(paths) >= MAX_FILES:
            break
    found = set(paths)
    stacks = [name for name, markers in MARKERS.items()
              if any(marker in found or any(p.startswith(marker + "/") for p in paths) for marker in markers)]
    return {"stack": stacks or ["Unclassified"], "files_scanned": len(paths),
            "truncated": len(paths) >= MAX_FILES,
            "instructions": [p for p in ("AGENTS.md", "CLAUDE.md", ".github/copilot-instructions.md")
                             if p in found]}


def proposals(root):
    facts = scan(root)
    stacks = ", ".join(facts["stack"])
    instructions = ", ".join(facts["instructions"]) or "None detected"
    return {
        ".ai-agent-manager/architect/PROJECT.md":
            "# Project architecture (generated)\n\n"
            f"Detected stacks: {stacks}\n\nExisting instructions: {instructions}\n\n"
            "This inventory is heuristic. Verify actual dependencies and module boundaries before changes.\n"
            "Existing agent instructions take precedence. Never overwrite user-authored files.\n",
        ".ai-agent-manager/architect/agents/project-reviewer.md":
            "# Project reviewer\n\nReview the repository before editing. Follow existing AGENTS.md, "
            "CLAUDE.md and project conventions. Inspect diffs, tests, security and compatibility. "
            "Do not claim tests passed without running them.\n",
        ".ai-agent-manager/architect/skills/project-workflow/SKILL.md":
            "# Project workflow\n\n1. Inspect relevant source and existing instructions.\n"
            "2. Propose a small change with acceptance criteria.\n"
            "3. Review the diff and run relevant tests.\n"
            "4. Request approval before applying generated configuration changes.\n",
    }


def plan(root):
    root = Path(root).resolve()
    changes = []
    for relative, desired in proposals(root).items():
        path = root / relative
        if path.is_symlink():
            raise ValueError(f"Refusing symlink: {relative}")
        old = path.read_text(encoding="utf-8") if path.exists() else ""
        if old == desired:
            continue
        # Existing files may have user modifications. Do not overwrite them.
        if path.exists():
            changes.append({"path": relative, "status": "preserved", "diff": "", "content": desired})
        else:
            diff = "".join(difflib.unified_diff([], desired.splitlines(keepends=True),
                                                fromfile="/dev/null", tofile=relative))
            changes.append({"path": relative, "status": "create", "diff": diff, "content": desired})
    return changes


def apply(root, changes, approved=False):
    if not approved:
        raise PermissionError("Explicit approval required (--approve)")
    root = Path(root).resolve()
    written = []
    for change in changes:
        if change["status"] != "create" or change["path"] not in MANAGED:
            continue
        path = root / change["path"]
        if path.is_symlink() or path.exists():
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        # Exclusive create avoids overwriting concurrent edits.
        with path.open("x", encoding="utf-8") as stream:
            stream.write(change["content"])
        written.append(change["path"])
    return written


def main(argv=None):
    parser = argparse.ArgumentParser(description="Project AI Architect: scan, preview, approve")
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--approve", action="store_true", help="Apply only new managed files")
    args = parser.parse_args(argv)
    changes = plan(args.root)
    if args.json:
        print(json.dumps({"scan": scan(args.root), "changes": [
            {k: v for k, v in change.items() if k != "content"} for change in changes
        ]}, indent=2))
    else:
        print("Detected:", ", ".join(scan(args.root)["stack"]))
        for change in changes:
            print(f"{change['status']}: {change['path']}")
            if change["diff"]:
                print(change["diff"])
    if args.approve:
        print("Created:", ", ".join(apply(args.root, changes, approved=True)) or "none")


if __name__ == "__main__":
    main()
