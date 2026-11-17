"""Opt-in Codex runtime profiles. Global Codex config remains untouched."""
import os
import shutil
import tempfile
from contextlib import contextmanager
from pathlib import Path

PROFILES = ("default", "minimal", "coding", "full")

@contextmanager
def profile_environment(profile, base=None):
    if profile not in PROFILES:
        raise ValueError(f"Unknown profile: {profile}")
    env = dict(os.environ if base is None else base)
    if profile in ("default", "full"):
        yield env
        return
    # Keep native sessions across turns without modifying global ~/.codex.
    base = Path(env.get("AAM_PROFILE_HOME", str(Path.home() / ".local" / "state" / "ai-agent-manager" / "codex")))
    home = base / profile
    home.mkdir(parents=True, exist_ok=True, mode=0o700)
    home.chmod(0o700)
    auth = Path.home() / ".codex" / "auth.json"
    target = home / "auth.json"
    if auth.is_file() and (not target.exists() or auth.stat().st_mtime_ns > target.stat().st_mtime_ns):
        shutil.copy2(auth, target)
        target.chmod(0o600)
    config = home / "config.toml"
    if not config.exists():
        config.write_text("")
        config.chmod(0o600)
    env["CODEX_HOME"] = str(home)
    yield env

def choose_profile(prompt, requested="auto"):
    if requested != "auto":
        if requested not in PROFILES:
            raise ValueError(f"Unknown profile: {requested}")
        return requested
    value = prompt.lower()
    if any(x in value for x in ("figma", "github", "mcp", "plugin", "browser", "external")):
        return "full"
    if any(x in value for x in ("implement", "fix ", "refactor", "test", "code", "build", "file", "repo", "implementir", "poprav")):
        return "coding"
    return "minimal"
