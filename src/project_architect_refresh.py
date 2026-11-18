"""Project-aware analysis and conflict-safe refresh of generated AAM configuration."""
import ast
import difflib
import hashlib
import json
import os
import re
from pathlib import Path

from project_ai_architect import scan, MANAGED

STATE = ".ai-agent-manager/architect/.manifest.json"
SKIP = {".git", ".venv", "venv", "node_modules", "build", "dist", ".gradle", ".idea", "__pycache__"}
MAX_SOURCE = 250
MAX_BYTES = 64000


def _hash(text):
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def analyze(root):
    """Bounded, offline source analysis; does not run project code or call a model."""
    root = Path(root).resolve()
    facts = scan(root)
    modules = set()
    symbols = []
    dependencies = set()
    count = 0
    for directory, dirs, files in os.walk(root, followlinks=False):
        dirs[:] = sorted(d for d in dirs if d not in SKIP and not d.startswith("."))
        for name in sorted(files):
            path = Path(directory) / name
            if path.is_symlink() or path.suffix not in (".py", ".kt", ".kts", ".swift", ".ts", ".tsx", ".js"):
                continue
            if count >= MAX_SOURCE:
                break
            try:
                if path.stat().st_size > MAX_BYTES:
                    continue
                source = path.read_text(encoding="utf-8")
            except (OSError, UnicodeError):
                continue
            count += 1
            relative = path.relative_to(root).as_posix()
            modules.add(str(path.relative_to(root).parent))
            if path.suffix == ".py":
                try:
                    tree = ast.parse(source)
                except SyntaxError:
                    continue
                for node in tree.body:
                    if isinstance(node, (ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)):
                        symbols.append(f"{relative}: {node.name}")
                    if isinstance(node, ast.Import):
                        dependencies.update(alias.name.split(".")[0] for alias in node.names)
                    if isinstance(node, ast.ImportFrom) and node.module:
                        dependencies.add(node.module.split(".")[0])
            else:
                for match in re.finditer(r"(?m)^\\s*(?:public |internal |private |export |default |open |data |sealed )*(?:class|interface|object|struct|protocol|fun|function)\\s+([A-Za-z_][\\w]*)", source):
                    symbols.append(f"{relative}: {match.group(1)}")
                for match in re.finditer(r"(?m)^\\s*(?:import|implementation|api)\\s*[ (\\\"]+([\\w.:-]+)", source):
                    dependencies.add(match.group(1))
        if count >= MAX_SOURCE:
            break
    return {**facts, "source_files_analyzed": count, "modules": sorted(modules)[:50],
            "symbols": sorted(symbols)[:70], "dependencies": sorted(dependencies)[:60],
            "analysis_truncated": count >= MAX_SOURCE}


def generated(root):
    facts = analyze(root)
    stack = ", ".join(facts["stack"])
    modules = ", ".join(facts["modules"]) or "Not detected"
    symbols = "\n".join("- " + symbol for symbol in facts["symbols"]) or "- None detected"
    deps = ", ".join(facts["dependencies"]) or "Not detected"
    instructions = ", ".join(facts["instructions"]) or "None detected"
    return {
        ".ai-agent-manager/architect/PROJECT.md":
            f"# Project architecture (generated)\n\nStacks: {stack}\n\nModules: {modules}\n\n"
            f"Dependencies (static import hints): {deps}\n\nExisting instructions: {instructions}\n\n"
            f"Top-level declarations (sample):\n{symbols}\n\n"
            "Static analysis is incomplete. Follow existing project instructions and verify actual APIs.\n",
        ".ai-agent-manager/architect/agents/project-reviewer.md":
            f"# Project reviewer\n\nDetected stack: {stack}.\n\n"
            "Inspect actual modules, public APIs, tests and security boundaries before editing. "
            "Follow AGENTS.md and CLAUDE.md when present. Never assert unexecuted tests passed.\n",
        ".ai-agent-manager/architect/skills/project-workflow/SKILL.md":
            f"# Project workflow\n\nProject stack: {stack}.\n\n"
            "1. Read existing project instructions and relevant implementation.\n"
            "2. Propose a minimal, testable change.\n"
            "3. Preview diffs and verify dependencies.\n"
            "4. Require approval before applying generated configurations.\n",
    }


def _manifest(root):
    path = Path(root) / STATE
    if not path.is_file() or path.is_symlink():
        return {}
    try:
        obj = json.loads(path.read_text(encoding="utf-8"))
        return obj if isinstance(obj, dict) else {}
    except (ValueError, OSError):
        return {}


def _legacy_generated(root):
    """Exact output of the initial standalone generator; safe to adopt only if unchanged."""
    facts = scan(root)
    stack = ", ".join(facts["stack"])
    instructions = ", ".join(facts["instructions"]) or "None detected"
    return {
        ".ai-agent-manager/architect/PROJECT.md":
            "# Project architecture (generated)\\n\\n"
            f"Detected stacks: {stack}\\n\\nExisting instructions: {instructions}\\n\\n"
            "This inventory is heuristic. Verify actual dependencies and module boundaries before changes.\\n"
            "Existing agent instructions take precedence. Never overwrite user-authored files.\\n",
        ".ai-agent-manager/architect/agents/project-reviewer.md":
            "# Project reviewer\\n\\nReview the repository before editing. Follow existing AGENTS.md, "
            "CLAUDE.md and project conventions. Inspect diffs, tests, security and compatibility. "
            "Do not claim tests passed without running them.\\n",
        ".ai-agent-manager/architect/skills/project-workflow/SKILL.md":
            "# Project workflow\\n\\n1. Inspect relevant source and existing instructions.\\n"
            "2. Propose a small change with acceptance criteria.\\n"
            "3. Review the diff and run relevant tests.\\n"
            "4. Request approval before applying generated configuration changes.\\n",
    }


def refresh_plan(root):
    """Only refresh a file if its bytes match the last approved generation."""
    root = Path(root).resolve()
    saved = _manifest(root)
    legacy = _legacy_generated(root)
    changes = []
    for relative, desired in generated(root).items():
        path = root / relative
        if path.is_symlink():
            changes.append({"path": relative, "status": "conflict", "diff": "", "content": desired})
            continue
        existing = path.read_text(encoding="utf-8") if path.is_file() else None
        if existing == desired:
            status = "unchanged"
        elif existing is None:
            status = "create"
        elif saved.get(relative) == _hash(existing) or (relative not in saved and existing == legacy.get(relative)):
            status = "update"
        else:
            status = "conflict"
        diff = ""
        if status in ("create", "update"):
            diff = "".join(difflib.unified_diff((existing or "").splitlines(keepends=True),
                        desired.splitlines(keepends=True), fromfile=relative + " (current)",
                        tofile=relative + " (proposed)"))
        changes.append({"path": relative, "status": status, "diff": diff,
                        "content": desired, "previous_hash": _hash(existing) if existing is not None else None})
    return changes


def refresh_apply(root, changes, approved=False):
    if not approved:
        raise PermissionError("Explicit approval required")
    root = Path(root).resolve()
    # Validate every candidate before writing anything.
    for item in changes:
        relative = item["path"]
        if relative not in MANAGED:
            raise ValueError("Unmanaged path")
        target = root / relative
        if target.is_symlink() or any(parent.is_symlink() for parent in target.parents if parent != root and root in parent.parents):
            raise ValueError("Symlink path blocked")
        old = target.read_text(encoding="utf-8") if target.is_file() else None
        if item["status"] == "create" and old is not None:
            raise RuntimeError("Project changed since preview")
        if item["status"] == "update" and (old is None or _hash(old) != item["previous_hash"]):
            raise RuntimeError("Project changed since preview")
    manifest_path = root / STATE
    if manifest_path.is_symlink() or any(p.is_symlink() for p in manifest_path.parents if p != root and root in p.parents):
        raise ValueError("Manifest symlink blocked")
    manifest = _manifest(root)
    applied = []
    for item in changes:
        if item["status"] not in ("create", "update"):
            continue
        target = root / item["path"]
        target.parent.mkdir(parents=True, exist_ok=True)
        if item["status"] == "create":
            with target.open("x", encoding="utf-8") as stream:
                stream.write(item["content"])
        else:
            target.write_text(item["content"], encoding="utf-8")
        manifest[item["path"]] = _hash(item["content"])
        applied.append(item["path"])
    if applied:
        manifest_path.parent.mkdir(parents=True, exist_ok=True)
        manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return applied
