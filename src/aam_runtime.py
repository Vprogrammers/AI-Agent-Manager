"""Bounded CLI event capture, Git fingerprint and cross-agent delta."""
import hashlib
import json
import os
import signal
import subprocess
import sys
import threading
import time
from collections import deque
from pathlib import Path

MAX_LINE_BYTES = 2 * 1024 * 1024
MAX_CAPTURE_BYTES = 16 * 1024 * 1024
MAX_FILE_HASH_BYTES = 256 * 1024 * 1024
MAX_FILES = 10000

def bounded_stream_runner(cmd, cwd, tool, model, env=None, on_event=None, cancel_event=None):
    start = time.monotonic()
    proc = subprocess.Popen(cmd, cwd=cwd, env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, bufsize=0, start_new_session=True)
    errors = deque(maxlen=40)
    stdout = []
    captured = 0
    overflow = False
    def read_stderr():
        for line in iter(proc.stderr.readline, b''):
            errors.append(line[-2048:].decode('utf-8', 'replace'))
    thread = threading.Thread(target=read_stderr, daemon=True)
    thread.start()
    stop_watch = threading.Event()
    def watch_cancel():
        if cancel_event is None:
            return
        while not stop_watch.is_set():
            if cancel_event.wait(0.1):
                if not stop_watch.is_set() and proc.poll() is None:
                    os.killpg(proc.pid, signal.SIGINT)
                return
    watcher = threading.Thread(target=watch_cancel, daemon=True)
    watcher.start()
    try:
        while True:
            line = proc.stdout.readline(MAX_LINE_BYTES + 1)
            if not line:
                break
            captured += len(line)
            if len(line) > MAX_LINE_BYTES or captured > MAX_CAPTURE_BYTES:
                overflow = True
                continue
            decoded = line.decode('utf-8', 'replace')
            if tool == 'claude':
                stdout.append(decoded)
                if on_event is not None:
                    try:
                        on_event(json.loads(decoded))
                    except ValueError:
                        pass
                continue
            try:
                obj = json.loads(decoded)
            except ValueError:
                continue
            event = obj.get('type', '')
            if on_event is not None:
                on_event(obj)
            if on_event is None and event.startswith(('item.', 'turn.')):
                item = obj.get('item') or {}
                print(f'[aam] {tool}/{model}: {event} {item.get("type", event)} '
                      f'({time.monotonic()-start:.1f}s)', file=sys.stderr, flush=True)
            if event in ('thread.started', 'turn.completed', 'turn.failed') or (
                event == 'item.completed' and (obj.get('item') or {}).get('type') == 'agent_message'
            ):
                stdout.append(decoded)
        rc = proc.wait()
        if cancel_event is not None and cancel_event.is_set():
            raise InterruptedError('Request cancelled; inspect repository changes before retrying')
    except KeyboardInterrupt:
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
        raise InterruptedError('Request interrupted; inspect changes before retrying')
    finally:
        stop_watch.set()
        thread.join(timeout=2)
        proc.stdout.close()
        proc.stderr.close()
    if overflow:
        return subprocess.CompletedProcess(cmd, 2, '', 'CLI output exceeded capture limits; inspect repository before retrying')
    return subprocess.CompletedProcess(cmd, rc, ''.join(stdout), ''.join(errors))

def repository_fingerprint(root):
    root = Path(root).resolve()
    def git(*args):
        return subprocess.run(['git', '-C', str(root), *args], capture_output=True,
                              timeout=30, check=True).stdout
    head = git('rev-parse', 'HEAD').strip()
    status = git('status', '--porcelain=v1', '-z', '--untracked-files=all')
    paths = set(git('ls-files', '-m', '-o', '--exclude-standard', '-z').split(b'\0'))
    paths.discard(b'')
    digest = hashlib.sha256(head + b'\0' + status)
    for raw in sorted(paths):
        path = root / os.fsdecode(raw)
        digest.update(raw + b'\0')
        if path.is_symlink():
            digest.update(b'SYMLINK' + os.fsencode(os.readlink(path)))
        elif path.is_file():
            with path.open('rb') as f:
                while True:
                    block = f.read(1024 * 1024)
                    if not block:
                        break
                    digest.update(block)
        else:
            digest.update(b'NONREGULAR')
    digest.update(git('diff', '--cached', '--raw', '-z'))
    return digest.hexdigest()

def handoff_delta(state, recipient, cfg):
    selected = []
    for turn in reversed(state.get('history', [])):
        if turn.get('tool') == recipient:
            break
        selected.append(turn)
    selected.reverse()
    return json.dumps(selected, ensure_ascii=False)[:max(0, cfg.get('history_chars', 4000))]

def model_for_session(state, session_key, sid, selected):
    if sid:
        return state.get('session_models', {}).get(session_key, selected)
    return selected
