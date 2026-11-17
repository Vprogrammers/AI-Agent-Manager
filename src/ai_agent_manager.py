#!/usr/bin/env python3
"""Project-independent Claude Code / Codex bridge with bounded Git handoff."""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path

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

def run_turn(root, tool, prompt, state, env=None, runner=None):
    env = os.environ if env is None else env
    runner = subprocess.run if runner is None else runner
    sessions = state.setdefault('sessions', {})
    sid = sessions.get(tool)
    previous = state.get('last_tool')
    switched = previous is not None and previous != tool
    actual_prompt = (handoff(root, state) + '\n\nNEW USER REQUEST:\n' + prompt) if switched else prompt
    model = model_for(tool, prompt, env)
    cmd = build_command(tool, model, sid, actual_prompt)
    completed = runner(cmd, cwd=str(root), capture_output=True, text=True)
    if completed.returncode:
        detail = (completed.stderr or completed.stdout)[-2000:]
        raise RuntimeError(f'{tool} exited {completed.returncode}: {detail}')
    new_sid, answer = parse_result(tool, completed.stdout)
    if sid and new_sid != sid:
        raise RuntimeError('Unexpected native session ID change; state was not saved')
    sessions[tool] = new_sid
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
    print('[aam] Per-turn model selection; /claude, /codex, /exit; shared Git handoff.', file=sys.stderr)
    def execute(prompt):
        nonlocal tool
        pending = state.get('pending_switch')
        if pending and pending != tool and shutil.which(pending):
            tool = pending
            print(f'[aam] Automatically switching next turn to {tool} after previous limit', file=sys.stderr)
        try:
            answer, model = run_turn(root, tool, prompt, state)
        except (RuntimeError, ValueError, json.JSONDecodeError) as e:
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
