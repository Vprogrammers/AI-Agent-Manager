import React from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import './style.css';

type Provider = 'claude' | 'codex';
type GithubRepo = { nameWithOwner: string; isPrivate: boolean; description: string | null };
type Message = { role: 'user' | 'assistant'; text: string; provider: Provider };
const TOKEN_LABELS: Record<string,string> = {
  input_tokens: 'Input',
  cached_input_tokens: 'Cached input',
  cache_read_input_tokens: 'Cached input',
  cache_creation_input_tokens: 'Cache creation',
  output_tokens: 'Output',
  reasoning_output_tokens: 'Reasoning output',
};
function tokenRows(value: string): Array<[string,string]> {
  if (!value || /^(Waiting|Not reported|0 reported)/.test(value)) return [];
  return value.split(' · ').map(part => {
    const index = part.lastIndexOf(': ');
    if (index < 0) return null;
    const key = part.slice(0,index).trim();
    const number = Number(part.slice(index+2).trim());
    if (!Number.isFinite(number)) return null;
    return [TOKEN_LABELS[key] || key.replace(/_/g,' ').replace(/^./,c=>c.toUpperCase()), number.toLocaleString('en-US')] as [string,string];
  }).filter((row): row is [string,string] => row !== null);
}
const WORKSPACES_KEY = 'aam.desktop.workspaces.v1';
function savedWorkspaces(): string[] {
  try { const value: unknown = JSON.parse(localStorage.getItem(WORKSPACES_KEY) || '[]'); return Array.isArray(value) ? value.filter((v): v is string => typeof v === 'string').slice(0, 8) : []; }
  catch { return []; }
}
function App() {
  const [root, setRoot] = React.useState('');
  const [recent, setRecent] = React.useState<string[]>(savedWorkspaces);
  const [provider, setProvider] = React.useState<Provider>('claude');
  const [prompt, setPrompt] = React.useState('');
  const [messages, setMessages] = React.useState<Message[]>([]);
  const [busy, setBusy] = React.useState(false);
  const [startedAt, setStartedAt] = React.useState<number | null>(null);
  const [elapsed, setElapsed] = React.useState(0);
  React.useEffect(() => { if (startedAt === null) return; const id = window.setInterval(() => setElapsed(Math.floor((Date.now()-startedAt)/1000)), 1000); return () => window.clearInterval(id); }, [startedAt]);
  const [error, setError] = React.useState('');
  const [pendingSwitch, setPendingSwitch] = React.useState(false);
  const [failedPrompt, setFailedPrompt] = React.useState('');
  const [usage, setUsage] = React.useState<string>('Not reported');
  const [lastProvider, setLastProvider] = React.useState<string>('None');
  const [limitNotice, setLimitNotice] = React.useState('');
  const [activity, setActivity] = React.useState('');
  const [liveText, setLiveText] = React.useState('');
  const [liveProvider, setLiveProvider] = React.useState<'claude'|'codex'>('claude');
  const requestSeq = React.useRef(0);
  const messagesEnd = React.useRef<HTMLDivElement>(null);
  React.useEffect(() => { messagesEnd.current?.scrollIntoView({ block: 'end', behavior: 'smooth' }); }, [messages, liveText]);
  const [githubOpen, setGithubOpen] = React.useState(false);
  const [repositories, setRepositories] = React.useState<GithubRepo[]>([]);
  const [repoFilter, setRepoFilter] = React.useState('');
  const [loadingRepos, setLoadingRepos] = React.useState(false);
  const [cloning, setCloning] = React.useState(false);
  const selectWorkspace = (path: string) => {
    setRoot(path); setMessages([]); setError(''); setPendingSwitch(false); setFailedPrompt('');
    const next = [path, ...recent.filter(p => p !== path)].slice(0, 8);
    setRecent(next); localStorage.setItem(WORKSPACES_KEY, JSON.stringify(next));
  };
  const chooseFolder = async () => {
    try {
      const selected = await open({ directory: true, multiple: false, title: 'Choose Git repository' });
      if (typeof selected === 'string') selectWorkspace(selected);
    } catch (e) { setError('Cannot open folder picker: ' + String(e)); }
  };
  const loadGithub = async () => {
    setGithubOpen(true); setLoadingRepos(true); setError('');
    try { setRepositories(await invoke<GithubRepo[]>('github_repositories')); }
    catch (e) { setError(String(e)); }
    finally { setLoadingRepos(false); }
  };
  const cloneGithub = async (repository: string) => {
    try {
      const destination = await open({ directory: true, multiple: false, title: 'Choose parent folder for clone' });
      if (typeof destination !== 'string') return;
      setCloning(true); setError('');
      const path = await invoke<string>('github_clone', { repository, destination });
      selectWorkspace(path); setGithubOpen(false);
    } catch (e) { setError(String(e)); }
    finally { setCloning(false); }
  };
  const readDiagnostics = (log: string) => {
    const tokenLines = [...log.matchAll(/tokens:\s*(\{[^\n]*\}|unavailable)/g)];
    if (tokenLines.length) {
      const raw = tokenLines[tokenLines.length - 1][1];
      if (raw === 'unavailable') setUsage('Not reported by provider');
      else {
        try {
          const parsed = JSON.parse(raw.replace(/'/g, '"')) as Record<string, unknown>;
          const fields = Object.entries(parsed).filter(([,v]) => typeof v === 'number');
          const total = fields.reduce((sum,[,v])=>sum+(v as number),0);
          setUsage(fields.length ? (total === 0 ? '0 reported (may be incomplete)' : fields.map(([k,v])=>k.replace(/_tokens$/,'')+': '+v).join(' · ')) : 'Not reported');
        } catch { setUsage('Not reported'); }
      }
    }
    const agents = [...log.matchAll(/\[aam\] (?:Agent ready:|Automatically switching to)\s*(claude|codex)/gi)];
    if (agents.length) setLastProvider(agents[agents.length - 1][1]);
  };
  const send = async () => {
    const text = prompt.trim();
    if (!text || !root.trim() || busy) return;
    const id = String(++requestSeq.current);
    const requestedProvider = provider;
    setPrompt(''); setError(''); setLimitNotice(''); setUsage('Waiting for provider');
    setLastProvider(requestedProvider); setActivity('Starting agent…'); setLiveText(''); setLiveProvider(requestedProvider);
    setBusy(true); setStartedAt(Date.now()); setElapsed(0); setPendingSwitch(false);
    setMessages(m => [...m, { role: 'user', text, provider: requestedProvider }]);
    let completed = false;
    let streamText = '';
    let codexSnapshot = '';
    let claudeSnapshot = '';
    let failed = false;
    let unlisten: (() => void) | undefined;
    try {
      unlisten = await listen<{requestId:string;data:Record<string,unknown>}>('aam-event', ({payload}) => {
        if (payload.requestId !== id) return;
        const data = payload.data;
        const kind = data.kind;
        if (kind === 'provider' && (data.provider === 'claude' || data.provider === 'codex')) {
          setLastProvider(data.provider); setLiveProvider(data.provider); setProvider(data.provider); setActivity('Running '+data.provider);
        } else if (kind === 'activity') {
          const ev = data.event as Record<string,unknown> | undefined;
          if (ev) {
            const eventType = String(ev.type || 'activity');
            const item = ev.item as Record<string,unknown> | undefined;
            setActivity(eventType + (item?.type ? ' · '+item.type : ''));
            // Claude emits content_block_delta; Codex emits agent_message deltas.
            // Some CLI versions only emit completed messages: render those as snapshots.
            const delta = ev.delta as Record<string,unknown> | undefined;
            const message = ev.message as Record<string,unknown> | undefined;
            const itemText = typeof item?.text === 'string' ? item.text : '';
            let chunk = '';
            if (eventType === 'content_block_delta' && delta?.type === 'text_delta' && typeof delta.text === 'string') {
              chunk = delta.text;
            } else if ((eventType === 'item.agent_message.delta' || eventType === 'response.output_text.delta') && typeof ev.delta === 'string') {
              chunk = ev.delta;
            } else if (eventType === 'item.completed' && item?.type === 'agent_message' && itemText) {
              if (itemText.startsWith(codexSnapshot)) chunk = itemText.slice(codexSnapshot.length);
              else if (!streamText) chunk = itemText;
              codexSnapshot = itemText;
            } else if (eventType === 'assistant' && message?.content && Array.isArray(message.content)) {
              const full = (message.content as Array<Record<string,unknown>>).filter(c=>c.type==='text').map(c=>String(c.text || '')).join('');
              if (full.startsWith(claudeSnapshot)) chunk = full.slice(claudeSnapshot.length);
              else if (!streamText) chunk = full;
              claudeSnapshot = full;
            }
            if (chunk) {
              streamText += chunk;
              setLiveText(streamText);
            }
            const usage = (ev.usage || (ev.message as Record<string,unknown> | undefined)?.usage) as Record<string,unknown> | undefined;
            if (usage) setUsage(Object.entries(usage).filter(([,v])=>typeof v==='number').map(([k,v])=>k+': '+v).join(' · ') || 'Not reported');
          }
        } else if (kind === 'result') {
          completed = true;
          const actual = data.provider === 'codex' ? 'codex' : 'claude';
          setLastProvider(actual); setProvider(actual);
          const counts = data.usage as Record<string,unknown> | undefined;
          setUsage(counts && Object.keys(counts).length ? Object.entries(counts).map(([k,v])=>k+': '+v).join(' · ') : 'Not reported');
          setMessages(m => [...m, {role:'assistant', text:String(data.answer || streamText), provider:actual}]);
          setLiveText('');
          setActivity('Completed · '+String(data.model || actual));
        } else if (kind === 'error') {
          failed = true;
          setError(String(data.message || 'Agent failed'));
          const pending = data.pending_provider;
          if (pending === 'codex' || pending === 'claude') {
            setProvider(pending); setLastProvider(String(data.provider || requestedProvider));
            setFailedPrompt(text); setPendingSwitch(true);
            setLimitNotice('Provider limit reached. '+pending+' selected for next request; unsafe replay was blocked.');
          }
          setActivity('Request failed');
          if (streamText) { setMessages(m=>[...m,{role:'assistant',text:streamText+'\n\n[Partial response — request failed]',provider:liveProvider}]); setLiveText(''); }
        } else if (kind === 'process_error') {
          failed = true; setError(String(data.message || 'CLI process failed'));
        }
      });
      await invoke<void>('run_prompt', {requestId:id,root,provider:requestedProvider,prompt:text});
      if (!completed && !failed) setError('Agent finished without a structured result. Check CLI version.');
    } catch (e) { setError(String(e)); }
    finally { unlisten?.(); setBusy(false); setStartedAt(null); }
  };
  return <div className="app">
    <aside><h2>AI Agent Manager</h2><label>Workspace</label><button className="open-folder" onClick={()=>void chooseFolder()}>Open Folder…</button><button className="open-folder" onClick={()=>void loadGithub()}>Open from GitHub…</button><input aria-label="Git repository path" placeholder="/path/to/git/repo" value={root} onChange={e=>setRoot(e.target.value)} onBlur={()=>{if(root.trim()&&!recent.includes(root.trim()))selectWorkspace(root.trim());}}/><label>Recent workspaces</label><div className="recent">{recent.map(path=><button key={path} title={path} className={path===root?'selected':''} onClick={()=>selectWorkspace(path)}>{path.split('/').filter(Boolean).pop() || path}</button>)}</div><label>Agent</label><div className="agents"><button disabled={busy} className={provider==='claude'?'active claude':''} onClick={()=>setProvider('claude')}>Claude</button><button disabled={busy} className={provider==='codex'?'active codex':''} onClick={()=>setProvider('codex')}>Codex</button></div><p className="hint">Provider limits may require a manual continuation when prior work cannot be ruled out.</p><div className="usage-panel"><strong>Last request</strong><span>Provider: {lastProvider}</span><div className="token-stats"><strong>Tokens</strong>{tokenRows(usage).length ? tokenRows(usage).map(([label,value],index)=><div className="token-row" key={index}><span>{label}</span><span className="token-value">{value}</span></div>) : <span className="token-empty">{usage}</span>}</div><small>Only provider-reported usage is shown. Zero can mean missing counters.</small></div></aside>
    <main>{limitNotice&&<div className="limit-banner" role="status">{limitNotice}</div>}{githubOpen&&<div className="github-panel"><div className="github-heading"><strong>GitHub repositories</strong><button onClick={()=>setGithubOpen(false)}>Close</button></div><p className="hint">Uses your authenticated GitHub CLI (gh). Clone a repository to a local folder before running agents.</p><input aria-label="Search GitHub repositories" placeholder="Search repositories…" value={repoFilter} onChange={e=>setRepoFilter(e.target.value)}/>{loadingRepos&&<p>Loading repositories…</p>}<div className="github-list">{repositories.filter(r=>r.nameWithOwner.toLowerCase().includes(repoFilter.toLowerCase())).map(r=><div className="github-repo" key={r.nameWithOwner}><div><strong>{r.nameWithOwner}</strong><small>{r.isPrivate?'Private':'Public'}</small><p>{r.description}</p></div><button disabled={cloning} onClick={()=>void cloneGithub(r.nameWithOwner)}>{cloning?'Cloning…':'Clone'}</button></div>)}</div></div>}<header><strong>Project session</strong><span>{root || 'Choose a Git repository'}</span></header><section className="messages" aria-live="polite">{messages.length===0&&<p className="empty">Choose a Git workspace and send your first request.</p>}{messages.map((m,i)=><article key={i} className={m.role}><small>{m.role==='user'?'YOU':m.provider.toUpperCase()}</small><pre>{m.text}</pre></article>)}{busy&&liveText&&<article className="streaming"><small>{liveProvider.toUpperCase()} · LIVE</small><pre>{liveText}<span className="stream-cursor">▍</span></pre></article>}{busy&&<p className="hint" role="status">{lastProvider.toUpperCase()} working… {elapsed}s · {activity}</p>}<div ref={messagesEnd} aria-hidden="true" /></section>{error&&<div className="error" role="alert"><p>{error}</p>{pendingSwitch&&<div className="recovery"><p>Claude reached a limit. Codex is now selected for the next request. The failed request was NOT replayed because previous file changes could not be verified. Review Git changes first.</p><button onClick={()=>{setPrompt('Continue the previous task using the current repository state. First inspect existing changes and do not repeat completed operations. Previous request: '+failedPrompt);setPendingSwitch(false);setError('');}}>Prepare safe continuation prompt</button></div>}</div>}<footer className={`composer ${busy ? liveProvider : provider}`}><textarea aria-label="Prompt" value={prompt} onChange={e=>setPrompt(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();void send();}}} placeholder="Ask Claude or Codex…"/><button className={`send-button ${busy ? liveProvider : provider}`} disabled={busy||!root.trim()||!prompt.trim()} onClick={()=>void send()}>{busy?'Running…':'Send →'}</button></footer></main>
  </div>;
}
createRoot(document.getElementById('root')!).render(<React.StrictMode><App/></React.StrictMode>);
