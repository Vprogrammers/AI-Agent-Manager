import React from 'react';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';

type TerminalEvent = { requestId: string; text: string; finished: boolean };
type Props = {
  requestId: string;
  provider: string;
  root: string;
  permission: 'read-only' | 'workspace-write' | 'full-access';
  initialPrompt?: string;
  onFinish: () => void;
  onError: (message: string) => void;
};

/**
 * Raw PTY I/O, including ANSI rendering, cursor movement, arrow keys and mouse.
 * Approval prompts are owned by the actual CLI, not guessed from its output.
 */
export default function InteractiveTerminal({
  requestId, provider, root, permission, initialPrompt, onFinish, onError,
}: Props) {
  const container = React.useRef<HTMLDivElement>(null);
  const terminal = React.useRef<Terminal | null>(null);
  const [running, setRunning] = React.useState(true);
  const [queuedPrompt, setQueuedPrompt] = React.useState(initialPrompt ?? '');
  const send = React.useCallback((input: string) => {
    void invoke('interactive_cli_input', { requestId, input })
      .catch((error: unknown) => onError(String(error)));
  }, [requestId, onError]);

  React.useEffect(() => {
    if (!container.current) return;
    const term = new Terminal({
      cursorBlink: true, convertEol: false, scrollback: 5000,
      fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
      fontSize: 13, theme: { background: '#101820', foreground: '#e5edf5' },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(container.current);
    term.focus();
    terminal.current = term;
    const keyboard = term.onData(send);
    let disposed = false;
    let stop: (() => void) | undefined;
    const resize = new ResizeObserver(() => {
      fit.fit();
      void invoke('resize_interactive_cli', {
        requestId, rows: term.rows, cols: term.cols,
      }).catch(() => {});
    });
    resize.observe(container.current);
    const ready = async () => {
      try {
        stop = await listen<TerminalEvent>('interactive-cli-output', event => {
          if (disposed || event.payload.requestId !== requestId) return;
          if (event.payload.text) term.write(event.payload.text);
          if (event.payload.finished) {
            term.writeln('\r\n[CLI session ended]');
            setRunning(false);
            onFinish();
          }
        });
        if (disposed) { stop(); return; }
        await invoke('start_interactive_cli', {
          requestId, provider, root, permission,
        });
      } catch (error) {
        term.writeln('\r\n[Cannot start agent: ' + String(error) + ']');
        setRunning(false);
        onError(String(error));
        onFinish();
      }
    };
    void ready();
    return () => {
      disposed = true;
      stop?.();
      keyboard.dispose();
      resize.disconnect();
      term.dispose();
      terminal.current = null;
    };
  // Keep one PTY lifecycle per request ID. Parent remounts for new sessions.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requestId]);

  return <section aria-label={provider + ' interactive terminal'} style={{ padding: 12 }}>
    <p>Original {provider} CLI · native approval prompts · {permission} permissions</p>
    <div ref={container} style={{ height: 400, width: '100%', background: '#101820' }} />
    {running && <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
      <input
        aria-label="Send text to agent terminal"
        value={queuedPrompt}
        onChange={event => setQueuedPrompt(event.target.value)}
        onKeyDown={event => {
          if (event.key === 'Enter' && !event.shiftKey) {
            event.preventDefault();
            send(queuedPrompt + '\r');
            setQueuedPrompt('');
            terminal.current?.focus();
          }
        }}
        placeholder="Enter your prompt or CLI approval answer"
      />
      <button type="button" onClick={() => {
        send(queuedPrompt + '\r');
        setQueuedPrompt('');
        terminal.current?.focus();
      }}>Send to CLI</button>
      <button type="button" onClick={() => {
        void invoke('interrupt_interactive_cli', { requestId }).catch(onError);
      }}>Ctrl+C</button>
    </div>}
    <small>CLI permission menus are interactive: use arrow keys, Enter, Esc or the input above. No approval is automatically granted.</small>
  </section>;
}
