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
from session_optimizer import settings as optimization_settings, after_turn, analytics, cache_policy, compact_history, write_checkpoint, load_checkpoint
from aam_runtime import bounded_stream_runner, repository_fingerprint, handoff_delta, model_for_session
from aam_tui import RichTurn, rich_available, provider_model
from smart_router import route as smart_route

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
    permission = os.environ.get('AAM_PERMISSION_MODE', 'default')
    if permission not in ('default', 'read-only', 'workspace-write', 'full-access'):
        raise ValueError('Invalid AAM_PERMISSION_MODE')
    if tool == 'claude':
        cmd = ['claude', '-p', '--output-format', 'json']
        if permission == 'workspace-write':
            cmd += ['--permission-mode', 'acceptEdits']
        elif permission == 'full-access':
            cmd += ['--dangerously-skip-permissions']
        elif permission == 'read-only':
            cmd += ['--allowedTools', 'Read,Glob,Grep']
        if session_id:
            cmd += ['--resume', session_id]
        if model:
            cmd += ['--model', model]
        return cmd + [prompt]
    cmd = ['codex', 'exec']
    # The desktop engine already runs inside an OS-enforced sandbox.
    # A second macOS Seatbelt profile fails with sandbox_apply: EPERM.
    # Skip Codex's nested sandbox only when the desktop launcher explicitly
    # confirms that the outer OS sandbox is active.
    outer_sandbox = os.environ.get('AAM_OUTER_SANDBOX') == '1'
    if outer_sandbox and permission in ('read-only', 'workspace-write'):
        cmd += ['--dangerously-bypass-approvals-and-sandbox']
    elif permission == 'read-only':
        cmd += ['--sandbox', 'read-only']
    elif permission == 'workspace-write':
        cmd += ['--sandbox', 'workspace-write']
    elif permission == 'full-access':
        cmd += ['--dangerously-bypass-approvals-and-sandbox']
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

def handoff(root, state, recipient=None):
    """Transfer explicit recorded messages plus actual Git facts, never invented decisions."""
    branch = git(root, 'branch', '--show-current')
    status = git(root, 'status', '--short')[:4000]
    diff = git(root, 'diff', '--stat')[:2000]
    staged = git(root, 'diff', '--cached', '--stat')[:2000]
    issue_file = root / '.ai-agent-manager' / 'HANDOFF.md'
    issue_text = issue_file.read_text(errors='replace')[-3000:] if issue_file.is_file() else 'No project handoff file found.'
    return ('CROSS-AGENT HANDOFF: You are continuing work from a DIFFERENT AI CLI. '
            'The other provider session ID cannot be resumed here. Review repository state before any changes. '
            'Do not repeat potentially completed side-effecting operations. '
            'Do not assume tests passed unless evidence is present. Never commit/push without explicit approval.\n'
            f'Branch: {branch}\nHEAD: {git(root,"rev-parse","HEAD")}\n'
            f'Working tree:\n{status or "clean"}\nDiff stat:\n{diff}\nStaged stat:\n{staged}\n'
            f'Issue handoff:\n{issue_text}\n'
            'Recent explicit turns (may be truncated; verify against files):\n'
            + handoff_delta(state, recipient, optimization_settings(os.environ)))

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
            return {k: usage.get(k) for k in ('input_tokens', 'cache_read_input_tokens', 'cache_creation_input_tokens', 'output_tokens') if isinstance(usage.get(k), int)}
        except json.JSONDecodeError:
            pass
    return {}


def record_usage(root, tool, model, seconds, usage, status, profile='default', task=None):
    log = store_path(root).with_name('usage.jsonl')
    log.parent.mkdir(parents=True, exist_ok=True)
    entry = {'utc': datetime.now(timezone.utc).isoformat(), 'provider': tool,
             'model': model, 'duration_seconds': round(seconds, 3),
             'status': status, 'profile': profile, 'task': task, 'usage': usage}
    with log.open('a', encoding='utf-8') as f:
        f.write(json.dumps(entry, ensure_ascii=False) + '\n')


def stream_runner(cmd, cwd, tool, model, env=None, on_event=None, cancel_event=None):
    return bounded_stream_runner(cmd, cwd, tool, model, env, on_event=on_event, cancel_event=cancel_event)

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
    reset = re.search(r'resets?\s+([^"\\\\n]+?)(?=[."]|$)', details, re.I)
    result = f'{tool.capitalize()} | LIMIT REACHED'
    if reset:
        result += f' | resets {reset.group(1).strip()}'
    return result


class ProviderLimitError(RuntimeError):
    def __init__(self, summary, safe_retry=False):
        super().__init__(summary)
        self.safe_retry = safe_retry


def repository_snapshot(root):
    try:
        return repository_fingerprint(root)
    except (OSError, subprocess.SubprocessError, RuntimeError) as e:
        print(f'[aam] Repository verification unavailable: {e}', file=sys.stderr)
        return ('unverified', uuid.uuid4().hex)

def claude_rejection_reason(stdout):
    """Return a non-sensitive reason when automatic replay is unsafe."""
    try:
        obj = json.loads(stdout)
    except (ValueError, TypeError):
        return 'invalid Claude response'
    if not isinstance(obj, dict) or obj.get('is_error') is not True:
        return 'not an explicit Claude error'
    # num_turns includes the rejected request itself on some Claude CLI
    # versions. It is NOT evidence of successful tool execution.
    # Require an explicit quota error, zero usage/cost and unchanged Git
    # snapshot instead; never use num_turns as the replay gate.
    error_text = str(obj.get('result', '')) + ' ' + str(obj.get('api_error', ''))
    if not LIMIT.search(error_text):
        return 'not an explicit quota rejection'
    if obj.get('total_cost_usd', 0) != 0:
        return 'Claude reported nonzero cost'
    usage = obj.get('usage', {})
    if not isinstance(usage, dict):
        return 'invalid usage metadata'
    # Claude usage also contains metadata (e.g. cache_creation dict).
    # Only token counters indicate billable model work; metadata is not usage.
    token_fields = ('input_tokens', 'output_tokens', 'cache_creation_input_tokens',
                    'cache_read_input_tokens')
    if any(type(usage.get(key, 0)) not in (int, float) or usage.get(key, 0) != 0
           for key in token_fields):
        return 'Claude reported token usage'
    return None


def safe_claude_rejection(stdout):
    return claude_rejection_reason(stdout) is None


def claude_retry_reason(stdout, before, after):
    reason = claude_rejection_reason(stdout)
    if reason:
        return reason
    if before != after:
        return 'repository changed during failed request'
    return None


def run_turn(root, tool, prompt, state, env=None, runner=None, on_event=None, cancel_event=None):
    env = os.environ if env is None else env
    runner = stream_runner if runner is None else runner
    sessions = state.setdefault('sessions', {})
    profile = choose_profile(prompt, env.get('AAM_CODEX_PROFILE', 'coding')) if tool == 'codex' else 'default'
    session_key = f'{tool}:{profile}' if tool == 'codex' else tool
    sid = cache_policy(state, tool, profile, prompt, env)
    previous = state.get('last_tool')
    switched = previous is not None and previous != tool
    actual_prompt = (handoff(root, state, tool) + '\n\nNEW USER REQUEST:\n' + prompt) if switched else prompt
    model = model_for_session(state, session_key, sid, model_for(tool, prompt, env))
    cmd = build_command(tool, model, sid, actual_prompt)
    before = repository_snapshot(root) if tool == 'claude' else None
    start = time.monotonic()
    if tool == 'codex' and on_event is None:
        print(f'[aam] Codex environment profile: {profile}', file=sys.stderr)
    with profile_environment(profile, env) as runtime_env:
        if runner is stream_runner:
            completed = runner(cmd, cwd=str(root), tool=tool, model=model or 'default', env=runtime_env, on_event=on_event, cancel_event=cancel_event)
        else:
            completed = runner(cmd, cwd=str(root), capture_output=True, text=True, env=runtime_env)
    elapsed = time.monotonic() - start
    usage = usage_from_output(tool, completed.stdout)
    state['last_usage'] = usage
    if tool == 'claude':
        try:
            reported_model = provider_model(json.loads(completed.stdout))
            if reported_model:
                model = reported_model
        except (ValueError, TypeError):
            pass
    elif not model:
        for line in completed.stdout.splitlines():
            try:
                reported_model = provider_model(json.loads(line))
                if reported_model:
                    model = reported_model
                    break
            except (ValueError, TypeError):
                continue
    record_usage(root, tool, model or 'CLI default', elapsed, usage, 'ok' if completed.returncode == 0 else 'failed', profile, env.get('AAM_TASK_ID'))
    if on_event is None:
        print(f'[aam] {tool}/{model or "default"} | {elapsed:.1f}s | tokens: {usage or "unavailable"}', file=sys.stderr)
    if completed.returncode:
        summary = limit_summary(tool, completed.stdout, completed.stderr)
        if summary:
            reason = claude_retry_reason(completed.stdout, before, repository_snapshot(root)) if tool == 'claude' else 'provider has not confirmed zero work'
            if reason:
                print(f'[aam] Automatic retry safety check: {reason}', file=sys.stderr)
            raise ProviderLimitError(summary, safe_retry=reason is None)
        detail = (completed.stderr or completed.stdout)[-500:]
        raise RuntimeError(f'{tool} exited {completed.returncode}: {detail}')
    summary = limit_summary(tool, completed.stdout, completed.stderr) if tool == 'claude' else None
    if summary:
        reason = claude_retry_reason(completed.stdout, before, repository_snapshot(root))
        if reason:
            print(f'[aam] Automatic retry safety check: {reason}', file=sys.stderr)
        raise ProviderLimitError(summary, safe_retry=reason is None)
    new_sid, answer = parse_result(tool, completed.stdout)
    if sid and new_sid != sid:
        raise RuntimeError('Unexpected native session ID change; state was not saved')
    sessions[session_key] = new_sid
    state.setdefault('session_models', {})[session_key] = model
    state['last_tool'] = tool
    state.setdefault('history', []).append({'tool': tool, 'prompt': prompt[:2500], 'answer': answer[:3500], 'utc': datetime.now(timezone.utc).isoformat()})
    state['history'] = state['history'][-12:]
    state['last_completed'] = {'tool': tool, 'session_id': new_sid, 'utc': datetime.now(timezone.utc).isoformat()}
    warning = after_turn(state, tool, usage, env, store_path(root).with_name('checkpoint.json'))
    if warning and on_event is None:
        print('[aam] Context warning: observed input tokens exceeded threshold; consider /checkpoint and /new when safe.', file=sys.stderr)
    return answer, model or 'CLI default'

def show_agent_ready(tool, root, clear=False):
    """Show the active provider without changing non-interactive output."""
    if not sys.stdout.isatty():
        return
    if clear and os.environ.get('AAM_NO_CLEAR') != '1':
        print('\033[2J\033[H', end='', flush=True)
    print(f'AI-Agent-Manager | {tool.upper()} READY')
    print(f'Project: {root}')
    print('Commands: /claude /codex /new /status /context /analytics /checkpoint /recover /exit')
    print('-' * 64, flush=True)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('tool', choices=['claude', 'codex', 'auto', 'balanced'])
    p.add_argument('--root', type=Path, default=Path.cwd())
    p.add_argument('--new', action='store_true')
    p.add_argument('--prompt')
    p.add_argument('--desktop-jsonl', action='store_true', help='Emit structured desktop events on stdout')
    p.add_argument('--ui', choices=['classic', 'rich', 'textual'], default=None,
                   help='UI mode (default: textual in interactive terminals, classic otherwise)')
    ns = p.parse_args()
    if ns.ui is None:
        ns.ui = 'textual' if ns.prompt is None and sys.stdin.isatty() and sys.stdout.isatty() else 'classic'
    root = ns.root.resolve()
    if ns.ui == 'rich' and not rich_available():
        p.error('Rich UI requires: python3 -m pip install rich')
    try:
        path = store_path(root)
    except ValueError as e:
        p.error(str(e))
    if ns.tool in ('claude', 'codex') and not shutil.which(ns.tool):
        p.error(f'{ns.tool} executable not found')
    state = {} if ns.new or not path.exists() else json.loads(path.read_text())
    tool = ns.tool if ns.tool in ('claude', 'codex') else 'codex'
    if ns.ui != 'textual':
        show_agent_ready(tool, root, clear=ns.prompt is None)
    if not sys.stdout.isatty():
        print('[aam] Agent ready: ' + tool, file=sys.stderr)
    def emit_desktop(kind, **data):
        print(json.dumps({'kind': kind, **data}, ensure_ascii=False), flush=True)

    def execute(prompt, on_event=None, on_result=None, cancel_event=None):
        nonlocal tool
        if ns.tool in ('auto', 'balanced'):
            decision = smart_route(prompt, state, os.environ, mode=ns.tool, root=root)
            tool = decision['provider']
            if ns.desktop_jsonl:
                emit_desktop('routing', **decision)
            elif on_event is None:
                print('[aam] Router: ' + json.dumps(decision), file=sys.stderr)
        pending = state.get('pending_switch')
        if pending and pending != tool and shutil.which(pending):
            tool = pending
            if ns.desktop_jsonl:
                emit_desktop('provider', provider=tool)
            if on_event is None:
                print(f'[aam] Automatically switching to {tool} after previous limit', file=sys.stderr)
                show_agent_ready(tool, root)
        for attempt in range(2):
            if ns.desktop_jsonl:
                emit_desktop('provider', provider=tool)
            try:
                if ns.ui == 'rich' and sys.stdout.isatty() and ns.prompt is None:
                    view = RichTurn(tool, model_for(tool, prompt, os.environ))
                    with view:
                        answer, model = run_turn(root, tool, prompt, state, on_event=view.event)
                else:
                    view = None
                    answer, model = run_turn(root, tool, prompt, state, on_event=on_event, cancel_event=cancel_event)
            except (RuntimeError, ValueError, json.JSONDecodeError, InterruptedError) as e:
                message = str(e)
                if on_event is None:
                    print(f'[aam] {message}', file=sys.stderr)
                if isinstance(e, ProviderLimitError) or LIMIT.search(message):
                    other = 'codex' if tool == 'claude' else 'claude'
                    if shutil.which(other):
                        state['pending_switch'] = other
                        save(path, state)
                        if (attempt == 0 and isinstance(e, ProviderLimitError)
                                and e.safe_retry and tool == 'claude'):
                            if on_event is None:
                                print('[aam] Claude rejected before execution. Switching to Codex; retrying the same request once.', file=sys.stderr)
                            tool = other
                            if ns.desktop_jsonl:
                                emit_desktop('provider', provider=tool)
                            if on_event is None:
                                show_agent_ready(tool, root)
                            continue
                        print(f'[aam] Next request uses {other}; automatic retry blocked because prior work cannot be ruled out.', file=sys.stderr)
                    else:
                        print(f'[aam] Limit reached; {other} CLI not installed.', file=sys.stderr)
                if ns.desktop_jsonl:
                    emit_desktop('error', message=message, provider=tool, usage=state.get('last_usage', {}), pending_provider=state.get('pending_switch'))
                return 1
            state.pop('pending_switch', None)
            save(path, state)
            if ns.desktop_jsonl:
                emit_desktop('result', provider=tool, model=model, usage=state.get('last_usage', {}), answer=answer)
            if on_result is not None:
                on_result(tool, model, state.get('last_usage', {}), answer)
            elif view is not None:
                view.finish(tool, model, state.get('last_usage', {}), answer)
            else:
                print(f'[aam] {tool} | {model}', file=sys.stderr)
                print(answer)
            return 0
        return 1
    if ns.prompt is not None:
        if ns.desktop_jsonl:
            return execute(ns.prompt, on_event=lambda event: emit_desktop('activity', provider=tool, event=event))
        return execute(ns.prompt)
    if ns.ui == 'textual':
        if ns.prompt is not None:
            p.error('--ui textual requires an interactive terminal (omit --prompt)')
        try:
            from aam_textual import AgentApp
        except ImportError as error:
            p.error(f'Textual UI requires: python3 -m pip install textual ({error})')
        def tui_execute(prompt, event, cancel_event=None):
            output = []
            code = execute(prompt, on_event=event, cancel_event=cancel_event,
                           on_result=lambda agent, model, usage, answer:
                               output.append((agent, model, usage, answer)))
            return output[0] if code == 0 and output else None
        def tui_command(command):
            nonlocal tool
            if command in ('/claude', '/codex'):
                candidate = command[1:]
                if shutil.which(candidate):
                    tool = candidate
                    state.pop('pending_switch', None)
                    app.tool = tool
                    app.model_name = model_for(tool, '', os.environ) or 'CLI default'
                    app.refresh_status()
                else:
                    app.append(__import__('textual.widgets', fromlist=['Static']).Static(
                        f'{candidate} CLI is not installed'))
            elif command in ('/architect', '/architect-apply'):
                from project_architect_refresh import refresh_plan, refresh_apply
                from textual.widgets import Static
                try:
                    if command == '/architect':
                        app.architect_preview = refresh_plan(root)
                        changes = app.architect_preview
                        summary = '\n'.join(f"{c['status']}: {c['path']}" for c in changes)
                        diffs = '\n'.join(c['diff'] for c in changes if c['diff'])
                        app.append(Static('Architect preview (no files changed):\n' + summary + '\n' + diffs))
                        app.append(Static('Review changes, then enter /architect-apply to approve. Conflicts are preserved.'))
                    elif app.architect_preview is None:
                        app.append(Static('Run /architect first to preview the exact changes.'))
                    else:
                        changes = app.architect_preview
                        written = refresh_apply(root, changes, approved=True)
                        app.architect_preview = None
                        app.append(Static('Architect applied: ' + (', '.join(written) or 'no changes') +
                                          '\nConflicts preserved: ' + ', '.join(c['path'] for c in changes if c['status'] == 'conflict')))
                except (OSError, ValueError, RuntimeError) as error:
                    app.architect_preview = None
                    app.append(Static('Architect error: ' + str(error)))
            elif command == '/diff':
                diff = git(root, 'diff', '--stat') or 'No unstaged changes'
                staged = git(root, 'diff', '--cached', '--stat') or 'No staged changes'
                app.append(__import__('textual.widgets', fromlist=['Static']).Static(
                    'Unstaged:\n' + diff + '\nStaged:\n' + staged))
            elif command == '/history':
                from textual.widgets import Static
                turns = state.get('history', [])[-12:]
                if not turns:
                    app.append(Static('No saved turns yet'))
                for turn in turns:
                    app.append(Static(f"{turn.get('tool', '?').upper()} · {turn.get('utc', '')}\n"
                                      f"› {turn.get('prompt', '')}\n{turn.get('answer', '')}"))
            elif command == '/updates':
                from aam_updates import check_updates
                from textual.widgets import Static
                app.append(Static(check_updates(root)))
            elif command == '/tokens':
                from textual.widgets import Static
                app.append(Static('Last turn: ' + json.dumps(state.get('last_usage', {})) +
                                  '\nSession totals (completed turns): ' + json.dumps(app.session_usage)))
            elif command == '/status':
                app.append(__import__('textual.widgets', fromlist=['Static']).Static(
                    json.dumps({'agent':tool,'sessions':list(state.get('sessions',{}))})))
            elif command == '/new':
                state.clear()
                save(path, state)
                app.append(__import__('textual.widgets', fromlist=['Static']).Static(
                    'New session: saved references cleared'))
            elif command == '/checkpoint':
                write_checkpoint(path.with_name('checkpoint.json'), state, optimization_settings(os.environ))
                app.append(__import__('textual.widgets', fromlist=['Static']).Static('Checkpoint saved'))
            elif command == '/recover':
                app.append(__import__('textual.widgets', fromlist=['Static']).Static(
                    json.dumps(load_checkpoint(path.with_name('checkpoint.json')) or {}, indent=2)))
            elif command == '/analytics':
                app.append(__import__('textual.widgets', fromlist=['Static']).Static(
                    json.dumps(analytics(path.with_name('usage.jsonl')), indent=2)))
            elif command == '/context':
                app.append(__import__('textual.widgets', fromlist=['Static']).Static(
                    json.dumps(state.get('context_observations', {}))))
            else:
                app.append(__import__('textual.widgets', fromlist=['Static']).Static(
                    'Unknown command: ' + command))
        app = AgentApp(root, tool, tui_execute, tui_command,
                       model_for(tool, '', os.environ) or 'CLI default')
        app.run()
        return 0
    architect_preview = None
    while True:
        try:
            prompt = input(f'{tool}> ').strip()
        except (EOFError, KeyboardInterrupt):
            print()
            break
        if prompt in ('/architect', '/architect-apply'):
            from project_architect_refresh import refresh_plan, refresh_apply
            try:
                if prompt == '/architect':
                    architect_preview = refresh_plan(root)
                    for item in architect_preview:
                        print(f"{item['status']}: {item['path']}")
                        if item['diff']:
                            print(item['diff'])
                    print('Review changes, then enter /architect-apply to approve.')
                elif architect_preview is None:
                    print('Run /architect first to preview the exact changes.')
                else:
                    written = refresh_apply(root, architect_preview, approved=True)
                    architect_preview = None
                    print('Architect applied:', ', '.join(written) or 'none')
            except (OSError, ValueError, RuntimeError) as error:
                architect_preview = None
                print('Architect error:', error, file=sys.stderr)
            continue
        if prompt == '/recover':
            recovered = load_checkpoint(path.with_name('checkpoint.json'))
            print(json.dumps(recovered or {'error': 'No valid checkpoint'}, ensure_ascii=False, indent=2))
            continue
        if prompt == '/analytics':
            print(json.dumps(analytics(path.with_name('usage.jsonl')), indent=2))
            continue
        if prompt == '/checkpoint':
            write_checkpoint(path.with_name('checkpoint.json'), state, optimization_settings(os.environ))
            print('[aam] Local checkpoint saved; native session unchanged.', file=sys.stderr)
            continue
        if prompt == '/context':
            print(json.dumps({'observed_input_tokens': state.get('context_observations', {}),
                              'note': 'Observed input is not exact active context size.'}, indent=2))
            continue
        if prompt in ('/exit', '/quit'):
            break
        if prompt == '/new':
            state.clear()
            save(path, state)
            print('[aam] New bridge session (native session references cleared)', file=sys.stderr)
            show_agent_ready(tool, root, clear=True)
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
                show_agent_ready(tool, root)
            continue
        if prompt:
            execute(prompt)
    return 0

if __name__ == '__main__':
    sys.exit(main())
