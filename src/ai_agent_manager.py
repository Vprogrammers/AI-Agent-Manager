#!/usr/bin/env python3
"""Project-independent Claude Code / Codex bridge with bounded Git handoff."""
import argparse
import signal
import time
import threading
import json
import os
import re
import shutil
import subprocess
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path
from collections import deque
from codex_profiles import choose_profile, profile_environment

HIGH = re.compile(r'architecture|architect|security|auth|migration|concurrency|deadlock|payment|pricing|kmp|multiplatform|arhitektur|bezbednost|sigurnost|migracij|konkurent|cena|cijena', re.I)
LOW = re.compile(r'docs?|readme|typo|comment|format|rename|documentation|dokument|komentar', re.I)
MED = re.compile(r'implement|feature|fix|bug|screen|ui|test|refactor|compose|swiftui|implementacij|ekran|poprav', re.I)
LIMIT = re.compile(r'rate.limit|usage.limit|quota|too.many.requests|429|overloaded|capacity|credit.balance|billing.limit|token.limit', re.I)
MAX_CONTEXT = 4000

def complexity(prompt):
    return 'high' if HIGH.search(prompt) else 'low' if LOW.search(prompt) else 'medium'

def model_for(tool, prompt, env):
    return env.get(f'AAM_{tool.upper()}_{complexity(prompt).upper()}_MODEL', '').strip()

def git(root, *args):
    p = subprocess.run(['git', '-C', str(root), *args], capture_output=True, text=True)
    return p.stdout.strip() if p.returncode == 0 else ''

def store_path(root):
    gd = git(root, 'rev-parse', '--absolute-git-dir')
    if not gd:
        raise ValueError('Not a Git repository')
    return Path(gd) / 'ai-agent-manager' / 'agent-bridge.json'

def save(path, state):
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + '.' + uuid.uuid4().hex + '.tmp')
    try:
        tmp.write_text(json.dumps(state, indent=2, ensure_ascii=False) + '\n')
        os.replace(tmp, path)
    finally:
        tmp.unlink(missing_ok=True)

def build_command(tool, model, session_id, prompt):
    if tool == 'claude':
        cmd = ['claude', '-p', '--output-format', 'json']
        if session_id:
            cmd += ['--resume', session_id]
        if model:
            cmd += ['--model', model]
        return cmd + [prompt]
    cmd = ['codex', 'exec']
    if session_id:
        cmd += ['resume', session_id]
    if model:
        cmd += ['--model', model]
    return cmd + ['--json', prompt]

def parse_result(tool, output):
    if tool == 'claude':
        obj = json.loads(output)
        if obj.get('is_error'):
            raise RuntimeError(str(obj.get('result', 'Claude error')))
        if not obj.get('session_id'):
            raise ValueError('Claude did not return session_id')
        return obj['session_id'], str(obj.get('result', ''))
    sid = None
    messages = []
    for line in output.splitlines():
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        if obj.get('type') == 'thread.started':
            sid = obj.get('thread_id')
        if obj.get('type') == 'item.completed':
            item = obj.get('item') or {}
            if item.get('type') == 'agent_message':
                messages.append(item.get('text', ''))
        if obj.get('type') == 'turn.failed':
            raise RuntimeError('Codex turn failed')
    if not sid:
        raise ValueError('Codex did not return thread ID')
    return sid, '\n'.join(messages)

def handoff(root, state):
    """Transfer explicit recorded messages plus actual Git facts, never invented decisions."""
    branch = git(root, 'branch', '--show-current')
    status = git(root, 'status', '--short')[:4000]
    diff = git(root, 'diff', '--stat')[:2000]
    staged = git(root, 'diff', '--cached', '--stat')[:2000]
    issue_file = root / '.ai-agent-manager' / 'HANDOFF.md'
    issue_text = issue_file.read_text(errors='replace')[-3000:] if issue_file.is_file() else 'No project handoff file found.'
    history = state.get('history', [])[-2:]
    return ('CROSS-AGENT HANDOFF: You are continuing work from a DIFFERENT AI CLI. '
            'The other provider session ID cannot be resumed here. Review repository state before any changes. '
            'Do not repeat potentially completed side-effecting operations. '
            'Do not assume tests passed unless evidence is present. Never commit/push without explicit approval.\n'
            f'Branch: {branch}\nHEAD: {git(root,"rev-parse","HEAD")}\n'
            f'Working tree:\n{status or "clean"}\nDiff stat:\n{diff}\nStaged stat:\n{staged}\n'
            f'Issue handoff:\n{issue_text}\n'
            'Recent explicit turns (may be truncated; verify against files):\n'
            + json.dumps(history, ensure_ascii=False)[-MAX_CONTEXT:])

def usage_from_output(tool, output):
    """Extract provider-reported usage; never invent missing token counts."""
    if tool == 'codex':
        for line in reversed(output.splitlines()):
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                continue
            if obj.get('type') == 'turn.completed':
                usage = obj.get('usage') or {}
                return {k: usage.get(k) for k in ('input_tokens', 'cached_input_tokens', 'output_tokens') if isinstance(usage.get(k), int)}
    else:
        try:
            obj = json.loads(output)
            usage = obj.get('usage') or {}
            return {k: usage.get(k) for k in ('input_tokens', 'cache_read_input_tokens', 'output_tokens') if isinstance(usage.get(k), int)}
        except json.JSONDecodeError:
            pass
    return {}


def record_usage(root, tool, model, seconds, usage, status):
    log = store_path(root).with_name('usage.jsonl')
    log.parent.mkdir(parents=True, exist_ok=True)
    entry = {'utc': datetime.now(timezone.utc).isoformat(), 'provider': tool,
             'model': model, 'duration_seconds': round(seconds, 3),
             'status': status, 'usage': usage}
    with log.open('a', encoding='utf-8') as f:
        f.write(json.dumps(entry, ensure_ascii=False) + '\n')


def stream_runner(cmd, cwd, tool, model, env=None):
    """Stream progress from JSONL events, keeping stdout parseable and memory bounded."""
    start = time.monotonic()
    proc = subprocess.Popen(cmd, cwd=cwd, env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, text=True, bufsize=1,
                            start_new_session=True)
    stdout_lines = []
    errors = deque(maxlen=40)
    def read_stderr():
        for line in proc.stderr:
            errors.append(line)
    thread = threading.Thread(target=read_stderr, daemon=True)
    thread.start()
    try:
        for line in proc.stdout:
            stdout_lines.append(line)
            if tool == 'codex':
                try:
                    obj = json.loads(line)
                    event = obj.get('type', '')
                    if event.startswith('item.') or event.startswith('turn.'):
                        item = obj.get('item') or {}
                        kind = item.get('type', event)
                        print(f'[aam] {tool}/{model}: {event} {kind} ({time.monotonic()-start:.1f}s)', file=sys.stderr, flush=True)
                except json.JSONDecodeError:
                    pass
        rc = proc.wait()
    except KeyboardInterrupt:
        print('[aam] Interrupt requested; stopping active CLI process group.', file=sys.stderr)
        os.killpg(proc.pid, signal.SIGINT)
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGTERM)
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait()
        raise InterruptedError('Request interrupted; inspect Git changes before retrying')
    finally:
        thread.join(timeout=2)
    return subprocess.CompletedProcess(cmd, rc, ''.join(stdout_lines), ''.join(errors))


def limit_summary(tool, stdout, stderr):
    """Return a short, safe provider-limit message without exposing raw JSON."""
    raw = '\n'.join((stdout or '', stderr or ''))
    if not LIMIT.search(raw):
        return None
    details = raw
    try:
        payload = json.loads(stdout)
        details = str(payload.get('result') or payload.get('error') or raw)
    except (ValueError, TypeError, AttributeError):
        pass
    reset = re.search(r'resets?\s+([^"\\\\\n]+?)(?=[."]|$)', details, re.I)
    result = f'{tool.capitalize()} | LIMIT REACHED'
    if reset:
        result += f' | resets {reset.group(1).strip()}'
    return result


def run_turn(root, tool, prompt, state, env=None, runner=None):
    env = os.environ if env is None else env
    runner = stream_runner if runner is None else runner
    sessions = state.setdefault('sessions', {})
    profile = choose_profile(prompt, env.get('AAM_CODEX_PROFILE', 'coding')) if tool == 'codex' else 'default'
    session_key = f'{tool}:{profile}' if tool == 'codex' else tool
    sid = sessions.get(session_key)
    previous = state.get('last_tool')
    switched = previous is not None and previous != tool and not sid
    actual_prompt = (handoff(root, state) + '\n\nNEW USER REQUEST:\n' + prompt) if switched else prompt
    model = model_for(tool, prompt, env)
    cmd = build_command(tool, model, sid, actual_prompt)
    start = time.monotonic()
    if tool == 'codex':
        print(f'[aam] Codex environment profile: {profile}', file=sys.stderr)
    with profile_environment(profile, env) as runtime_env:
        if runner is stream_runner:
            completed = runner(cmd, cwd=str(root), tool=tool, model=model or 'default', env=runtime_env)
        else:
            completed = runner(cmd, cwd=str(root), capture_output=True, text=True, env=runtime_env)
    elapsed = time.monotonic() - start
    usage = usage_from_output(tool, completed.stdout)
    record_usage(root, tool, model or 'CLI default', elapsed, usage, 'ok' if completed.returncode == 0 else 'failed')
    print(f'[aam] {tool}/{model or "default"} | {elapsed:.1f}s | tokens: {usage or "unavailable"}', file=sys.stderr)
    if completed.returncode:
        summary = limit_summary(tool, completed.stdout, completed.stderr)
        if summary:
            raise RuntimeError(summary)
        detail = (completed.stderr or completed.stdout)[-500:]
        raise RuntimeError(f'{tool} exited {completed.returncode}: {detail}')
    summary = limit_summary(tool, completed.stdout, completed.stderr) if tool == 'claude' else None
    if summary:
        raise RuntimeError(summary)
    new_sid, answer = parse_result(tool, completed.stdout)
    if sid and new_sid != sid:
        raise RuntimeError('Unexpected native session ID change; state was not saved')
    sessions[session_key] = new_sid
    state['last_tool'] = tool
    state.setdefault('history', []).append({'tool': tool, 'prompt': prompt[:2500], 'answer': answer[:3500], 'utc': datetime.now(timezone.utc).isoformat()})
    state['history'] = state['history'][-4:]
    return answer, model or 'CLI default'

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('tool', choices=['claude', 'codex'])
    p.add_argument('--root', type=Path, default=Path.cwd())
    p.add_argument('--new', action='store_true')
    p.add_argument('--prompt')
    ns = p.parse_args()
    root = ns.root.resolve()
    try:
        path = store_path(root)
    except ValueError as e:
        p.error(str(e))
    if not shutil.which(ns.tool):
        p.error(f'{ns.tool} executable not found')
    state = {} if ns.new or not path.exists() else json.loads(path.read_text())
    tool = ns.tool
    print('[aam] Per-turn model selection; /claude, /codex, /new, /status, /exit; shared Git handoff.', file=sys.stderr)
    def execute(prompt):
        nonlocal tool
        pending = state.get('pending_switch')
        if pending and pending != tool and shutil.which(pending):
            tool = pending
            print(f'[aam] Automatically switching next turn to {tool} after previous limit', file=sys.stderr)
        try:
            answer, model = run_turn(root, tool, prompt, state)
        except (RuntimeError, ValueError, json.JSONDecodeError, InterruptedError) as e:
            message = str(e)
            print(f'[aam] {message}', file=sys.stderr)
            if LIMIT.search(message):
                other = 'codex' if tool == 'claude' else 'claude'
                if shutil.which(other):
                    state['pending_switch'] = other
                    save(path, state)
                    print(f'[aam] Limit suspected. Next request automatically routes to {other}. '
                          'Inspect repository changes before continuing; failed work is NOT retried automatically.', file=sys.stderr)
                else:
                    print(f'[aam] Limit suspected; {other} CLI not installed.', file=sys.stderr)
            return 1
        state.pop('pending_switch', None)
        save(path, state)
        print(f'[aam] {tool} | {model}', file=sys.stderr)
        print(answer)
        return 0
    if ns.prompt is not None:
        return execute(ns.prompt)
    while True:
        try:
            prompt = input(f'{tool}> ').strip()
        except (EOFError, KeyboardInterrupt):
            print()
            break
        if prompt in ('/exit', '/quit'):
            break
        if prompt == '/new':
            state.clear()
            save(path, state)
            print('[aam] New bridge session (native session references cleared)', file=sys.stderr)
            continue
        if prompt == '/status':
            print(json.dumps({'agent': tool, 'sessions': list(state.get('sessions', {})), 'last_tool': state.get('last_tool'), 'pending_switch': state.get('pending_switch')}, ensure_ascii=False))
            continue
        if prompt in ('/claude', '/codex'):
            candidate = prompt[1:]
            if not shutil.which(candidate):
                print(f'[aam] {candidate} executable not found', file=sys.stderr)
            else:
                tool = candidate
                state.pop('pending_switch', None)
                print(f'[aam] Next turn uses {tool}', file=sys.stderr)
            continue
        if prompt:
            execute(prompt)
    return 0

if __name__ == '__main__':
    sys.exit(main())
