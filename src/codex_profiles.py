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
    with tempfile.TemporaryDirectory(prefix="aam-codex-") as directory:
        home = Path(directory)
        auth = Path.home() / ".codex" / "auth.json"
        if auth.is_file():
            shutil.copy2(auth, home / "auth.json")
            (home / "auth.json").chmod(0o600)
        (home / "config.toml").write_text("")
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
