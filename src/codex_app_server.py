"""Codex App Server JSON-RPC adapter for real-time agent text deltas."""
import json
import os
import select
import signal
import subprocess
import threading
import time

def run_codex_app_server(cwd, prompt, session_id=None, model=None, env=None,
                         on_event=None, cancel_event=None):
    proc = subprocess.Popen(['codex', 'app-server'], cwd=cwd, env=env,
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, bufsize=0, start_new_session=True)
    errors = []
    def stderr_reader():
        for line in proc.stderr:
            errors.append(line.decode('utf-8', 'replace')[-1000:])
            if len(errors) > 20:
                errors.pop(0)
    reader = threading.Thread(target=stderr_reader, daemon=True)
    reader.start()
    sequence = 0
    def send(method, params=None, request=True):
        nonlocal sequence
        obj = {'jsonrpc': '2.0', 'method': method}
        if params is not None:
            obj['params'] = params
        if request:
            sequence += 1
            obj['id'] = sequence
        proc.stdin.write((json.dumps(obj) + '\\n').encode())
        proc.stdin.flush()
        return sequence if request else None
    def receive(deadline):
        buffer = bytearray()
        while time.monotonic() < deadline:
            if cancel_event is not None and cancel_event.is_set():
                raise InterruptedError('Request cancelled; inspect repository changes before retrying')
            if proc.poll() is not None:
                raise RuntimeError('Codex App Server exited: ' + ''.join(errors)[-500:])
            ready, _, _ = select.select([proc.stdout], [], [], 0.1)
            if not ready:
                continue
            ch = os.read(proc.stdout.fileno(), 1)
            if ch == b'\\n':
                try:
                    return json.loads(buffer)
                except ValueError:
                    buffer.clear()
            else:
                buffer.extend(ch)
                if len(buffer) > 2 * 1024 * 1024:
                    raise RuntimeError('Codex App Server event exceeds size limit')
        raise TimeoutError('Codex App Server timed out')
    def request(method, params, timeout=30):
        request_id = send(method, params)
        deadline = time.monotonic() + timeout
        while True:
            event = receive(deadline)
            if event.get('id') == request_id:
                if 'error' in event:
                    raise RuntimeError('Codex ' + method + ': ' + str(event['error']))
                return event.get('result') or {}
    try:
        request('initialize', {'clientInfo': {'name': 'ai-agent-manager', 'version': '0.3.0'},
                               'capabilities': {'experimentalApi': False}})
        send('initialized', request=False)
        if session_id:
            thread = request('thread/resume', {'threadId': session_id})
        else:
            permission = (env or os.environ).get('AAM_PERMISSION_MODE', 'workspace-write')
            sandbox = 'read-only' if permission == 'read-only' else 'workspace-write'
            if permission == 'full-access' or (env or os.environ).get('AAM_OUTER_SANDBOX') == '1':
                sandbox = 'danger-full-access'
            params = {'cwd': cwd, 'approvalPolicy': 'never', 'sandbox': sandbox}
            if model:
                params['model'] = model
            thread = request('thread/start', params)
        tid = (thread.get('thread') or {}).get('id') or thread.get('threadId') or session_id
        if not tid:
            raise RuntimeError('Codex App Server did not return thread ID')
        turn = request('turn/start', {'threadId': tid, 'input': [{'type': 'text', 'text': prompt}]})
        turn_id = (turn.get('turn') or {}).get('id')
        if not turn_id:
            raise RuntimeError('Codex App Server did not return turn ID')
        if on_event:
            on_event({'type': 'thread.started', 'thread_id': tid})
        parts = []
        usage = {}
        deadline = time.monotonic() + 1800
        while True:
            event = receive(deadline)
            method = event.get('method', '')
            params = event.get('params') or {}
            if params.get('threadId') not in (None, tid):
                continue
            if method == 'item/agentMessage/delta':
                delta = params.get('delta', '')
                if isinstance(delta, str) and delta:
                    parts.append(delta)
                    if on_event:
                        on_event({'type': 'item.agent_message.delta', 'delta': delta})
            elif method == 'thread/tokenUsage/updated':
                token = params.get('tokenUsage') or {}
                last = token.get('last') or {}
                usage = {k: last[v] for k, v in [('input_tokens', 'inputTokens'),
                         ('cached_input_tokens', 'cachedInputTokens'), ('output_tokens', 'outputTokens')]
                         if isinstance(last.get(v), int)}
            elif method == 'turn/completed':
                completed_turn = params.get('turn') or {}
                if completed_turn.get('id') != turn_id:
                    continue
                status = completed_turn.get('status')
                if status not in ('completed', None):
                    raise RuntimeError('Codex turn ended with status: ' + str(status))
                answer = ''.join(parts)
                if not answer:
                    raise RuntimeError('Codex completed without streamed agent text')
                events = [{'type': 'thread.started', 'thread_id': tid},
                          {'type': 'item.completed', 'item': {'type': 'agent_message', 'text': answer}},
                          {'type': 'turn.completed', 'usage': usage}]
                return subprocess.CompletedProcess(['codex', 'app-server'], 0,
                                                   '\\n'.join(json.dumps(x) for x in events), '')
    finally:
        if proc.poll() is None:
            os.killpg(proc.pid, signal.SIGTERM)
            try:
                proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait()
        reader.join(timeout=1)
        proc.stdin.close()
        proc.stdout.close()
        proc.stderr.close()
