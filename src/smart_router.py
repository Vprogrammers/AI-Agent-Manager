"""Deterministic, zero-LLM-call task router. Scores are heuristics, not quality guarantees."""
import re
import shutil

TASKS = {
    "analysis": re.compile(r"\b(architect(?:ure)?|design|security|audit|review|research|plan|arhitektur|analiz|bezbednost|sigurnost)\b", re.I),
    "coding": re.compile(r"\b(implement|code|fix|bug|refactor|test|build|compose|swiftui|kotlin|rust|python|implementir|poprav|testir)\b", re.I),
    "docs": re.compile(r"\b(readme|docs?|documentation|typo|comment|rename|dokument|komentar)\b", re.I),
}

def classify(prompt):
    scores = {name: len(regex.findall(prompt)) for name, regex in TASKS.items()}
    return max(scores, key=lambda key: (scores[key], {"analysis": 2, "coding": 1, "docs": 0}[key])) if any(scores.values()) else "general"

def route(prompt, state, env, available=None, mode="auto"):
    """Prefer existing native session to save context; no network or LLM call."""
    available = available if available is not None else {p for p in ("claude", "codex") if shutil.which(p)}
    available = set(available) & {"claude", "codex"}
    if not available:
        raise ValueError("Neither Claude Code nor Codex CLI is installed")
    task = classify(prompt)
    previous = state.get("last_tool")
    pending = state.get("pending_switch")
    if pending in available:
        provider, reason = pending, "Provider limit: switch to available alternative"
    elif mode == "balanced" and previous in available:
        provider, reason = previous, "Reuse native session and cached context"
    elif previous in available and state.get("sessions") and task != "analysis":
        provider, reason = previous, "Reuse native session to avoid repeated handoff"
    else:
        preferred = "claude" if task == "analysis" else "codex"
        provider = preferred if preferred in available else sorted(available)[0]
        reason = f"Rule-based {task} routing; no additional model call"
    level = "high" if task == "analysis" else "low" if task == "docs" else "medium"
    configured = env.get(f"AAM_{provider.upper()}_{level.upper()}_MODEL", "").strip()
    return {"provider": provider, "model": configured or "CLI default / existing session",
            "complexity": level, "task": task, "reason": reason, "mode": mode}
