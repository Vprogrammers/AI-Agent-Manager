import React from 'react';
import { invoke } from '@tauri-apps/api/core';

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
  const [checking, setChecking] = React.useState(true);
  const [error, setError] = React.useState('');
  const [open, setOpen] = React.useState(false);
  const [dismissed, setDismissed] = React.useState(false);

  const refresh = React.useCallback(async () => {
    setChecking(true);
    setError('');
    try {
      const items = await invoke<ProviderStatus[]>('provider_setup_status');
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
        <div className="setup-providers">
          {(['claude', 'codex'] as const).map(id => {
            const item = statuses.find(s => s.id === id);
            const guide = setup[id];
            const installed = item?.installed;
            const authenticated = item?.authenticated;
            return <div className="setup-provider" key={id}>
              <h3>{guide.name} <span className={authenticated ? 'setup-ready' : 'setup-needed'}>{authenticated ? 'Ready' : !installed ? 'Not installed' : item?.authStatus === 'unknown' ? 'Auth unverified' : 'Sign in required'}</span></h3>
              {!installed && <div><p>1. Install the CLI in your system terminal:</p><code>{guide.install}</code><button type="button" onClick={() => void navigator.clipboard.writeText(guide.install)}>Copy</button></div>}
              {installed && !authenticated && <div><p>2. Sign in using your system terminal:</p><code>{guide.login}</code><button type="button" onClick={() => void navigator.clipboard.writeText(guide.login)}>Copy</button></div>}
              {authenticated && <p>CLI installed and authentication confirmed.</p>}
              <p className="hint">{item?.detail || 'Checking CLI status…'} · <a href={guide.docs} target="_blank" rel="noopener noreferrer">Official setup guide</a></p>
            </div>;
          })}
        </div>
        <div className="setup-actions">
          <button type="button" onClick={() => void refresh()} disabled={checking}>{checking ? 'Checking…' : 'Recheck installation and login'}</button>
          <button type="button" onClick={() => { setDismissed(true); setOpen(false); }}>{ready ? 'Continue' : 'Skip for now'}</button>
        </div>
        <p className="hint">Commands run in your terminal, never inside the app. After installation, restart the app if your PATH changed, then recheck. You can reopen setup at any time.</p>
      </section>
    </div>}
  </>;
}
