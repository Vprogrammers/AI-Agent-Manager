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
    """Compatibility API: use the same generator as interactive refresh."""
    from project_architect_refresh import generated
    return generated(root)


def plan(root):
    """Unified plan for CLI and Textual, including safe updates and conflicts."""
    from project_architect_refresh import refresh_plan
    return refresh_plan(root)


def apply(root, changes, approved=False):
    from project_architect_refresh import refresh_apply
    return refresh_apply(root, changes, approved=approved)


def main(argv=None):
    parser = argparse.ArgumentParser(description="Project AI Architect: scan, preview, approve")
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--approve", action="store_true", help="Approve previewed managed changes")
    args = parser.parse_args(argv)
    changes = plan(args.root)
    if args.json:
        print(json.dumps({"scan": scan(args.root), "changes": [
            {k: v for k, v in change.items() if k not in ("content", "previous_hash")} for change in changes
        ]}, indent=2))
    else:
        print("Detected:", ", ".join(scan(args.root)["stack"]))
        for change in changes:
            print(f"{change['status']}: {change['path']}")
            if change["diff"]:
                print(change["diff"])
    if args.approve:
        print("Applied:", ", ".join(apply(args.root, changes, approved=True)) or "none")


if __name__ == "__main__":
    main()
