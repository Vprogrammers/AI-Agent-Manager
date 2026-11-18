import React from 'react';
import { invoke } from '@tauri-apps/api/core';

type NodeStatus = { installed: boolean; detail: string; installer: string };

type ProviderStatus = {
  id: 'claude' | 'codex';
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
  const ready = statuses.some(item => item.authenticated);
  const installNode = async () => {
    if (!window.confirm('Install Node.js and npm using ' + (node?.installer || 'the OS package manager') + '? This downloads software and may request administrator authorization.')) return;
    setWorking('node'); setError(''); setNotice('');
    try {
      setNotice(await invoke<string>('install_node_runtime'));
      await refresh();
    } catch (e) { setError(String(e)); }
    finally { setWorking(null); }
  };
  const perform = async (id: 'claude' | 'codex', action: 'install' | 'login') => {
    if (action === 'install' && !window.confirm('Install ' + setup[id].name + ' globally using npm? This downloads software from the npm registry and changes your system environment.')) return;
    setWorking(id); setError(''); setNotice('');
    try {
      const message = await invoke<string>(action === 'install' ? 'install_provider_cli' : 'open_provider_login', { provider: id });
      setNotice(message);
      await refresh();
    } catch (e) { setError(String(e)); }
    finally { setWorking(null); }
  };

  return <>
    <button type="button" className="setup-launch" onClick={() => { setOpen(true); void refresh(); }}>
      Agent setup {checking ? '· checking…' : ready ? '· ready' : '· action required'}
    </button>
    {open && <div className="setup-overlay" role="presentation">
      <section className="setup-dialog" role="dialog" aria-modal="true" aria-labelledby="setup-title">
        <div className="setup-top">
          <div><h2 id="setup-title">Set up AI agents</h2><p className="hint">Install and sign in to at least one CLI to run AI requests. No credentials are collected by this app.</p></div>
          <button type="button" onClick={() => { setDismissed(true); setOpen(false); }} aria-label="Close setup">Close</button>
        </div>
        {error && <p role="alert" className="error">{error}</p>}
        {notice && <p role="status" className="hint">{notice}</p>}
        <div className="setup-provider">
          <h3>Node.js + npm <span className={node?.installed ? 'setup-ready' : 'setup-needed'}>{node?.installed ? 'Ready' : 'Required for CLI installation'}</span></h3>
          <p className="hint">{node?.detail || 'Checking Node.js…'} · Installer: {node?.installer || 'detecting…'}</p>
          {!node?.installed && <button type="button" disabled={!!working || checking} onClick={() => void installNode()}>{working === 'node' ? 'Installing Node.js…' : 'Install Node.js automatically'}</button>}
        </div>
        <div className="setup-providers">
          {(['claude', 'codex'] as const).map(id => {
            const item = statuses.find(s => s.id === id);
            const guide = setup[id];
            const installed = item?.installed;
            const authenticated = item?.authenticated;
            return <div className="setup-provider" key={id}>
              <h3>{guide.name} <span className={authenticated ? 'setup-ready' : 'setup-needed'}>{authenticated ? 'Ready' : !installed ? 'Not installed' : item?.authStatus === 'unknown' ? 'Auth unverified' : 'Sign in required'}</span></h3>
              {!installed && <div><p>1. Install the CLI (requires Node.js/npm):</p><button type="button" disabled={!!working || !node?.installed} onClick={() => void perform(id, "install")}>{working === id ? "Installing…" : "Install automatically"}</button><details><summary>Manual alternative</summary><code>{guide.install}</code><button type="button" onClick={() => void navigator.clipboard.writeText(guide.install)}>Copy</button></details></div>}
              {installed && !authenticated && <div><p>2. Authenticate in your system terminal:</p><button type="button" disabled={!!working} onClick={() => void perform(id, "login")}>Open login</button><details><summary>Manual alternative</summary><code>{guide.login}</code><button type="button" onClick={() => void navigator.clipboard.writeText(guide.login)}>Copy</button></details></div>}
              {authenticated && <p>CLI installed and authentication confirmed.</p>}
              <p className="hint">{item?.detail || 'Checking CLI status…'} · <a href={guide.docs} target="_blank" rel="noopener noreferrer">Official setup guide</a></p>
            </div>;
          })}
        </div>
        <div className="setup-actions">
          <button type="button" onClick={() => void refresh()} disabled={checking}>{checking ? 'Checking…' : 'Recheck installation and login'}</button>
          <button type="button" onClick={() => { setDismissed(true); setOpen(false); }}>{ready ? 'Continue' : 'Skip for now'}</button>
        </div>
        <p className="hint">Node.js is installed using the OS package manager after confirmation. CLI installation runs through npm after confirmation. Login opens your system terminal; credentials remain with the provider CLI. Restart the app if PATH changed, then recheck.</p>
      </section>
    </div>}
  </>;
}
