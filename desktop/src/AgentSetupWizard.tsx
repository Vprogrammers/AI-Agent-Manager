import React from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

type ProviderId = 'claude' | 'codex' | 'gemini' | 'opencode' | 'copilot';
type NodeStatus = { installed: boolean; detail: string; installer: string };

type ProviderStatus = {
  id: ProviderId;
  installed: boolean;
  authenticated: boolean;
  authStatus: 'ready' | 'missing' | 'login_required' | 'unknown';
  detail: string;
};

const setup = {
  claude: {
    name: 'Claude Code',
    docs: 'https://code.claude.com/docs/en/setup',
    install: 'npm install -g @anthropic-ai/claude-code',
    login: 'claude auth login',
  },
  codex: {
    name: 'Codex',
    docs: 'https://developers.openai.com/codex/cli',
    install: 'npm install -g @openai/codex',
    login: 'codex login',
  },
  gemini: {
    name: 'Gemini CLI',
    docs: 'https://google-gemini.github.io/gemini-cli/docs/get-started/',
    install: 'npm install -g @google/gemini-cli',
    login: 'gemini',
  },
  opencode: {
    name: 'OpenCode',
    docs: 'https://opencode.ai/docs/cli/providers/',
    install: 'npm install -g @opencode/cli',
    login: 'opencode auth login',
  },
  copilot: {
    name: 'GitHub Copilot CLI',
    docs: 'https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/install-copilot-cli',
    install: 'npm install -g @github/copilot',
    login: 'copilot login',
  },
} as const;

export default function AgentSetupWizard() {
  const [statuses, setStatuses] = React.useState<ProviderStatus[]>([]);
  const [node, setNode] = React.useState<NodeStatus | null>(null);
  const [checking, setChecking] = React.useState(true);
  const [error, setError] = React.useState('');
  const [open, setOpen] = React.useState(false);
  const [dismissed, setDismissed] = React.useState(false);
  const [working, setWorking] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState('');
  const [authPending, setAuthPending] = React.useState(false);
  const [authProvider, setAuthProvider] = React.useState<ProviderId | null>(null);
  const [terminalOutput, setTerminalOutput] = React.useState('');
  const [terminalInput, setTerminalInput] = React.useState('');
  const [terminalOpen, setTerminalOpen] = React.useState(false);

  const refresh = React.useCallback(async () => {
    setChecking(true);
    setError('');
    try {
      const [runtime, items] = await Promise.all([invoke<NodeStatus>('node_setup_status'), invoke<ProviderStatus[]>('provider_setup_status')]);
      setNode(runtime);
      setStatuses(items);
      if (!dismissed && !items.some(item => item.authenticated)) setOpen(true);
    } catch (e) {
      setError(String(e));
      setOpen(true);
    } finally {
      setChecking(false);
    }
  }, [dismissed]);

  React.useEffect(() => { void refresh(); }, [refresh]);
  React.useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<{ text: string; finished: boolean }>('provider-login-output', event => {
      if (disposed) return;
      if (event.payload.text) setTerminalOutput(previous => (previous + event.payload.text).slice(-16000));
      if (event.payload.finished) {
        setTerminalOpen(false);
        // The PTY exiting does not prove login succeeded. Recheck explicitly.
        void invoke<ProviderStatus[]>('provider_setup_status').then(setStatuses).catch(() => {});
        void invoke('focus_after_login').catch(() => {});
      }
    }).then(fn => { if (disposed) fn(); else unlisten = fn; }).catch(e => setError(String(e)));
    return () => { disposed = true; unlisten?.(); };
  }, []);
  const sendTerminalInput = async () => {
    try {
      await invoke('login_pty_write', { input: terminalInput + '\\r' });
      setTerminalInput('');
    } catch (e) { setError(String(e)); }
  };
  const ready = statuses.some(item => item.authenticated);
  const needsSetup = statuses.some(item => !item.authenticated);
  const installNode = async () => {
    if (!window.confirm('Install Node.js and npm using ' + (node?.installer || 'the OS package manager') + '? This downloads software and may request administrator authorization.')) return;
    setWorking('node'); setError(''); setNotice('');
    try {
      setNotice(await invoke<string>('install_node_runtime'));
      await refresh();
    } catch (e) { setError(String(e)); }
    finally { setWorking(null); }
  };
  const perform = async (id: ProviderId, action: 'install' | 'login') => {
    if (action === 'install' && !window.confirm('Install ' + setup[id].name + ' globally using npm? This downloads software from the npm registry and changes your system environment.')) return;
    setWorking(id); setError(''); setNotice('');
    if (action === 'login') { setTerminalOutput(''); setTerminalOpen(true); }
    try {
      const message = await invoke<string>(action === 'install' ? 'install_provider_cli' : 'open_provider_login', { provider: id });
      if (action === 'install') {
        // The user has explicitly requested installation; launch the provider's
        // interactive login immediately afterwards, without another manual step.
        try {
          const login = await invoke<string>('open_provider_login', { provider: id });
          setNotice(message + ' ' + login);
          setTerminalOutput(''); setTerminalOpen(true);
          setAuthProvider(id);
          setAuthProvider(id);
        setAuthPending(true);
        } catch (loginError) {
          setNotice(message + ' Login could not be started automatically; use Open login to retry.');
          setTerminalOpen(false);
          setError(String(loginError));
        }
      } else {
        setNotice(message);
        setAuthPending(true);
      }
      await refresh();
    } catch (e) { setTerminalOpen(false); setError(String(e)); }
    finally { setWorking(null); }
  };

  React.useEffect(() => {
    if (!authPending) return;
    let active = true;
    let attempts = 0;
    const timer = window.setInterval(async () => {
      if (!active || attempts++ >= 60) {
        window.clearInterval(timer);
        if (active) setAuthPending(false);
        return;
      }
      try {
        const items = await invoke<ProviderStatus[]>('provider_setup_status');
        if (!active) return;
        setStatuses(items);
        if (items.some(item => item.id === authProvider && item.authenticated)) {
          setAuthPending(false);
          setNotice('Authentication verified. Agent is ready.');
          void invoke('focus_after_login').catch(() => { /* OS may deny foreground activation. */ });
          window.clearInterval(timer);
        }
      } catch { /* A manual recheck remains available. */ }
    }, 3000);
    return () => { active = false; window.clearInterval(timer); };
  }, [authPending, authProvider]);

  return <>
    <button type="button" className="setup-launch" onClick={() => { setOpen(true); void refresh(); }}>
      Agent setup {checking ? '· checking…' : needsSetup ? '· setup available' : ready ? '· ready' : '· action required'}
    </button>
    {open && <div className="setup-overlay" role="presentation">
      <section className="setup-dialog" role="dialog" aria-modal="true" aria-labelledby="setup-title">
        <div className="setup-top">
          <div><h2 id="setup-title">Set up AI agents</h2><p className="hint">Install and sign in to an agent CLI. Gemini, OpenCode and Copilot execution support is not yet wired into the AAM engine. No credentials are collected by this app.</p></div>
          <button type="button" onClick={() => { setDismissed(true); setOpen(false); }} aria-label="Close setup">Close</button>
        </div>
        {error && <p role="alert" className="error">{error}{error.includes('already in progress') && <button type="button" onClick={async () => { try { await invoke('cancel_provider_login'); setError('Login interruption requested. Retry when the previous session closes.'); } catch (e) { setError(String(e)); } }}>Cancel previous login</button>}</p>}
        {notice && <p role="status" className="hint">{notice}</p>}
        <div className="setup-provider">
          <h3>Node.js + npm <span className={node?.installed ? 'setup-ready' : 'setup-needed'}>{node?.installed ? 'Ready' : 'Required for CLI installation'}</span></h3>
          <p className="hint">{node?.detail || 'Checking Node.js…'} · Installer: {node?.installer || 'detecting…'}</p>
          {!node?.installed && <button type="button" disabled={!!working || checking} onClick={() => void installNode()}>{working === 'node' ? 'Installing Node.js…' : 'Install Node.js automatically'}</button>}
        </div>
        <div className="setup-providers">
          {(['claude', 'codex', 'gemini', 'opencode', 'copilot'] as const).map(id => {
            const item = statuses.find(s => s.id === id);
            const guide = setup[id];
            const installed = item?.installed;
            const authenticated = item?.authenticated;
            return <div className="setup-provider" key={id}>
              <h3>{guide.name} <span className={authenticated ? 'setup-ready' : 'setup-needed'}>{authenticated ? 'Ready' : !installed ? 'Not installed' : item?.authStatus === 'unknown' ? 'Auth unverified' : 'Sign in required'}</span></h3>
              {!installed && <div><p>1. Install the CLI (requires Node.js/npm):</p><button type="button" disabled={!!working || !node?.installed} onClick={() => void perform(id, "install")}>{working === id ? "Installing…" : "Install automatically"}</button><details><summary>Manual alternative</summary><code>{guide.install}</code><button type="button" onClick={() => void navigator.clipboard.writeText(guide.install)}>Copy</button></details></div>}
              {installed && !authenticated && <div><p>2. Authenticate using the provider browser flow:</p><button type="button" disabled={!!working} onClick={() => void perform(id, "login")}>Open login</button><details><summary>Manual alternative</summary><code>{guide.login}</code><button type="button" onClick={() => void navigator.clipboard.writeText(guide.login)}>Copy</button></details></div>}
              {authenticated && <p>CLI installed and authentication confirmed.</p>}
              {installed && item?.authStatus === 'unknown' && <p className="hint">Authentication is not independently verified for this provider. Complete the provider login flow and check its own account status. The app will not claim Ready without verification.</p>}
              <p className="hint">{item?.detail || 'Checking CLI status…'} · <a href={guide.docs} target="_blank" rel="noopener noreferrer">Official setup guide</a></p>
            </div>;
          })}
        </div>
        {(terminalOpen || terminalOutput) && <div className="setup-provider" aria-label="Embedded provider login terminal">
          <h3>Interactive login {terminalOpen ? '· running' : '· finished'}</h3>
          <pre className="setup-terminal" aria-live="off">{terminalOutput || 'Waiting for provider login…'}</pre>
          {terminalOpen && <button type="button" onClick={async () => { try { await invoke('cancel_provider_login'); setNotice('Login interruption requested. Wait for the terminal to close before retrying.'); } catch (e) { setError(String(e)); } }}>Cancel active login</button>}
          {terminalOpen && <form onSubmit={e => { e.preventDefault(); void sendTerminalInput(); }} className="setup-terminal-input">
            <input aria-label="Terminal input" type="text" value={terminalInput} onChange={e => setTerminalInput(e.target.value)} placeholder="Enter CLI response, if requested" autoComplete="off" />
            <button type="submit">Send</button>
          </form>}
          <p className="hint">OAuth may open your default browser. Never enter passwords or recovery codes in this terminal; authenticate on the provider's official page.</p>
        </div>}
        <div className="setup-actions">
          <button type="button" onClick={() => void refresh()} disabled={checking}>{checking ? 'Checking…' : 'Recheck installation and login'}</button>
          <button type="button" onClick={() => { setDismissed(true); setOpen(false); }}>{ready ? 'Continue' : 'Skip for now'}</button>
        </div>
        <p className="hint">Node.js is installed using the OS package manager after confirmation. CLI installation runs through npm after confirmation. After CLI installation, the app starts an embedded interactive terminal and checks authentication automatically. Provider OAuth may require your default browser. Credentials remain with the provider CLI. Restart the app if PATH changed, then recheck.</p>
      </section>
    </div>}
  </>;
}
