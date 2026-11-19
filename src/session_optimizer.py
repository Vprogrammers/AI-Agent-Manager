"""Conservative, opt-in session budgeting and local token analytics."""
import json
import os
from pathlib import Path
from datetime import datetime, timezone

def settings(env):
    def integer(key, default):
        try:
            return max(0, int(env.get(key, default)))
        except (TypeError, ValueError):
            return default
    return {
        'context_warning': integer('AAM_CONTEXT_WARNING_TOKENS', 100000),
        'session_rotate_tokens': integer('AAM_SESSION_ROTATE_TOKENS', 100000),
        'checkpoint_every': integer('AAM_CHECKPOINT_EVERY_TURNS', 8),
        'history_chars': integer('AAM_HANDOFF_HISTORY_CHARS', 4000),
        'summary_chars': integer('AAM_SUMMARY_CHARS', 1200),
    }

def context_observation(tool, usage):
    """Observed input tokens only; not a provider context-window measurement."""
    if tool == 'codex':
        return usage.get('input_tokens')
    if tool == 'claude':
        fields = ('input_tokens', 'cache_read_input_tokens', 'cache_creation_input_tokens')
        if not any(k in usage for k in fields):
            return None
        return sum(usage.get(k, 0) for k in fields if type(usage.get(k)) is int)
    return None

def after_turn(state, tool, usage, env, checkpoint_path=None):
    """Update estimates without interrupting or resetting a native session."""
    cfg = settings(env)
    observations = state.setdefault('context_observations', {})
    estimate = context_observation(tool, usage)
    if estimate is not None:
        observations[tool] = estimate
    count = state.get('completed_turns', 0) + 1
    state['completed_turns'] = count
    warning = bool(estimate is not None and cfg['context_warning'] and estimate >= cfg['context_warning'])
    if checkpoint_path is not None and cfg['checkpoint_every'] and count % cfg['checkpoint_every'] == 0:
        write_checkpoint(checkpoint_path, state, cfg)
    return warning

def write_checkpoint(path, state, cfg):
    """Local deterministic checkpoint; no extra model request or secrets."""
    path = Path(path)
    history = state.get('history', [])[-4:]
    data = {
        'updated_utc': datetime.now(timezone.utc).isoformat(),
        'last_tool': state.get('last_tool'),
        'sessions': state.get('sessions', {}),
        'completed_turns': state.get('completed_turns', 0),
        'last_completed': state.get('last_completed'),
        'session_models': state.get('session_models', {}),
        'recent_turns': [{'tool': x.get('tool'), 'prompt': x.get('prompt', '')[:cfg['summary_chars']],
                          'answer': x.get('answer', '')[:cfg['summary_chars']]} for x in history],
        'note': 'Deterministic bounded excerpt, not a semantic summary. Verify Git state before resuming.'
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    import uuid
    tmp = path.with_name(path.name + '.' + uuid.uuid4().hex + '.tmp')
    tmp.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')
    os.replace(tmp, path)

def compact_history(state, cfg):
    """Bounded excerpt for cross-provider handoff; native sessions remain untouched."""
    return json.dumps(state.get('history', [])[-2:], ensure_ascii=False)[-cfg['history_chars']:]

def analytics(path):
    """Aggregate reported token counters, without claiming billing savings."""
    totals = {}
    path = Path(path)
    if not path.exists():
        return totals
    for line in path.read_text().splitlines():
        try:
            entry = json.loads(line)
        except (ValueError, TypeError):
            continue
        provider = entry.get('provider', 'unknown')
        profile = entry.get('profile', 'default')
        key = provider + '/' + profile
        row = totals.setdefault(key, {'turns': 0, 'input': 0, 'cached': 0, 'output': 0, 'seconds': 0.0})
        usage = entry.get('usage') or {}
        row['turns'] += 1
        row['input'] += max(0, usage.get('input_tokens', 0)) if type(usage.get('input_tokens', 0)) is int else 0
        cached = usage.get('cached_input_tokens', usage.get('cache_read_input_tokens', 0))
        row['cached'] += max(0, cached) if type(cached) is int else 0
        row['output'] += max(0, usage.get('output_tokens', 0)) if type(usage.get('output_tokens', 0)) is int else 0
        row['seconds'] += float(entry.get('duration_seconds', 0))
    return totals

def cache_policy(state, tool, profile, prompt, env):
    """Rotate a native session after a large reported input turn."""
    if env.get('AAM_FORCE_NEW_SESSION') == '1':
        return None
    key = f'{tool}:{profile}' if tool == 'codex' else tool
    sid = state.get('sessions', {}).get(key)
    if env.get('AAM_AUTO_ROTATE_SESSIONS', '1') == '0':
        return sid
    observed = state.get('session_input_tokens', {}).get(key, 0)
    limit = settings(env)['session_rotate_tokens']
    if sid and limit and type(observed) is int and observed >= limit:
        return None
    return sid


def rotation_handoff(state, tool, prompt):
    """Preserve only bounded explicit context for follow-up prompts."""
    import re
    followup = re.search(
        r'\b(now|again|continue|previous|same|that|this|still|sada|opet|nastavi|prethodn|isto|jos|još|to|ovo|dalje)\b',
        prompt, re.I)
    if not followup:
        return prompt
    history = [item for item in state.get('history', []) if item.get('tool') == tool]
    if not history:
        return prompt
    last = history[-1]
    excerpt = json.dumps({'previous_request': last.get('prompt', '')[:1000],
                          'previous_answer': last.get('answer', '')[:1500]}, ensure_ascii=False)
    return ('Fresh session to bound token usage. Recent explicit context; verify against files: '
            + excerpt + '\nNEW USER REQUEST:\n' + prompt)


def load_checkpoint(path):
    try:
        data = json.loads(Path(path).read_text())
        return data if isinstance(data, dict) and isinstance(data.get('sessions'), dict) else None
    except (OSError, ValueError):
        return None
