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
type TokenUsage = Record<string, number>;
type UsageRecord = { provider: Provider; model: string; duration: number; usage: TokenUsage; time: string };
const USAGE_PREFIX = 'aam.desktop.usage.v1:';
function loadUsage(path: string): UsageRecord[] {
  if (!path) return [];
  try {
    const value: unknown = JSON.parse(localStorage.getItem(USAGE_PREFIX + path) || '[]');
    return Array.isArray(value) ? value.filter((v): v is UsageRecord =>
      !!v && (v.provider === 'claude' || v.provider === 'codex') &&
      typeof v.duration === 'number' && !!v.usage && typeof v.usage === 'object'
    ).slice(-500) : [];
  } catch { return []; }
}
function numericUsage(value: unknown): TokenUsage {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
  return Object.fromEntries(Object.entries(value).filter(([,v]) =>
    typeof v === 'number' && Number.isFinite(v) && v >= 0
  )) as TokenUsage;
}
const numberFormat = (value: number) => value.toLocaleString('en-US');
function totals(records: UsageRecord[]): TokenUsage {
  const result: TokenUsage = {};
  for (const record of records) for (const [key,value] of Object.entries(record.usage)) result[key] = (result[key] || 0) + value;
  return result;
}
function usageRows(usage: TokenUsage): Array<[string, number]> {
  const order = ['input_tokens','cached_input_tokens','cache_read_input_tokens','cache_creation_input_tokens','output_tokens','reasoning_output_tokens'];
  return Object.entries(usage).sort(([a],[b]) => {
    const ia = order.indexOf(a), ib = order.indexOf(b);
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib);
  }).map(([key,value]) => [TOKEN_LABELS[key] || key.replace(/_/g,' ').replace(/^./,c=>c.toUpperCase()), value]);
}
function cacheRatio(usage: TokenUsage): string | null {
  const input = usage.input_tokens;
  const cached = usage.cached_input_tokens ?? usage.cache_read_input_tokens;
  return input > 0 && cached !== undefined && cached <= input ? (100*cached/input).toFixed(1)+'%' : null;
}
function TokenTable({usage}: {usage:TokenUsage}) {
  const rows = usageRows(usage);
  return rows.length ? <div className="token-stats">{rows.map(([label,value],i)=><div className="token-row" key={i}><span>{label}</span><span className="token-value">{numberFormat(value)}</span></div>)}{cacheRatio(usage)&&<div className="token-row"><span>Cached share</span><span className="token-value">{cacheRatio(usage)}</span></div>}</div> : <p className="token-empty">Not reported</p>;
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
  const [permission, setPermission] = React.useState<'read-only'|'workspace-write'|'full-access'>('workspace-write');
  const [messages, setMessages] = React.useState<Message[]>([]);
  const [busy, setBusy] = React.useState(false);
  const [startedAt, setStartedAt] = React.useState<number | null>(null);
  const [elapsed, setElapsed] = React.useState(0);
  React.useEffect(() => { if (startedAt === null) return; const id = window.setInterval(() => setElapsed(Math.floor((Date.now()-startedAt)/1000)), 1000); return () => window.clearInterval(id); }, [startedAt]);
  const [error, setError] = React.useState('');
  const [pendingSwitch, setPendingSwitch] = React.useState(false);
  const [failedPrompt, setFailedPrompt] = React.useState('');
  const [usage, setUsage] = React.useState<string>('Not reported');
  const [lastUsage, setLastUsage] = React.useState<TokenUsage>({});
  const [usageHistory, setUsageHistory] = React.useState<UsageRecord[]>([]);
  const [usageExpanded, setUsageExpanded] = React.useState(true);
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
    setRoot(path); setUsageHistory(loadUsage(path)); setLastUsage({}); setUsage('Not reported'); setMessages([]); setError(''); setPendingSwitch(false); setFailedPrompt('');
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
    if (permission === 'full-access' && !window.confirm('Full access allows the agent to execute commands and modify files outside the workspace without normal sandbox protection. Continue for this request?')) return;
    setPrompt(''); setError(''); setLimitNotice(''); setUsage('Waiting for provider'); setLastUsage({});
    setLastProvider(requestedProvider); setActivity('Starting agent…'); setLiveText(''); setLiveProvider(requestedProvider);
    setBusy(true); setStartedAt(Date.now()); setElapsed(0); setPendingSwitch(false);
    setMessages(m => [...m, { role: 'user', text, provider: requestedProvider }]);
    const requestStarted = Date.now();
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
            if (usage) { const parsed = numericUsage(usage); setLastUsage(parsed); setUsage(Object.keys(parsed).length ? 'Live usage' : 'Not reported'); }
          }
        } else if (kind === 'result') {
          completed = true;
          const actual = data.provider === 'codex' ? 'codex' : 'claude';
          setLastProvider(actual); setProvider(actual);
          const counts = data.usage as Record<string,unknown> | undefined;
          const parsed = numericUsage(counts);
          setLastUsage(parsed); setUsage(Object.keys(parsed).length ? 'Reported' : 'Not reported');
          const record: UsageRecord = {provider:actual,model:String(data.model || 'Unknown'),duration:Math.round((Date.now()-requestStarted)/1000),usage:parsed,time:new Date().toISOString()};
          setUsageHistory(previous => { const next=[...previous,record].slice(-500); try { localStorage.setItem(USAGE_PREFIX+root,JSON.stringify(next)); } catch {} return next; });
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
      await invoke<void>('run_prompt', {requestId:id,root,provider:requestedProvider,prompt:text,permission});
      if (!completed && !failed) setError('Agent finished without a structured result. Check CLI version.');
    } catch (e) { setError(String(e)); }
    finally { unlisten?.(); setBusy(false); setStartedAt(null); }
  };
  return <div className="app">
    <aside className={`workspace-panel ${busy ? liveProvider : provider}`}><h2>AI Agent Manager</h2><label>Workspace</label><button className="open-folder" onClick={()=>void chooseFolder()}>Open Folder…</button><button className="open-folder" onClick={()=>void loadGithub()}>Open from GitHub…</button><input aria-label="Git repository path" placeholder="/path/to/git/repo" value={root} onChange={e=>setRoot(e.target.value)} onBlur={()=>{if(root.trim()&&!recent.includes(root.trim()))selectWorkspace(root.trim());}}/><label>Recent workspaces</label><div className="recent">{recent.map(path=><button key={path} title={path} className={path===root?'selected':''} onClick={()=>selectWorkspace(path)}>{path.split('/').filter(Boolean).pop() || path}</button>)}</div><label>Permissions</label><select aria-label="Agent permissions" value={permission} disabled={busy} onChange={e=>setPermission(e.target.value as typeof permission)}><option value="read-only">Read only</option><option value="workspace-write">Workspace write</option><option value="full-access">Full access (unsafe)</option></select><p className="hint">Workspace write allows project edits. Full access requires confirmation before each request.</p><label>Agent</label><div className="agents"><button disabled={busy} className={provider==='claude'?'active claude':''} onClick={()=>setProvider('claude')}>Claude</button><button disabled={busy} className={provider==='codex'?'active codex':''} onClick={()=>setProvider('codex')}>Codex</button></div><p className="hint">Provider limits may require a manual continuation when prior work cannot be ruled out.</p><div className="usage-panel">
      <div className="usage-heading"><strong>Usage statistics</strong><button onClick={()=>setUsageExpanded(v=>!v)} aria-expanded={usageExpanded}>{usageExpanded?'Hide':'Show'}</button></div>
      {usageExpanded&&<>
        <strong>Last request</strong>
        <span>Provider: {lastProvider}</span>
        <span>Status: {busy ? 'Running' : usage}</span>
        {usageHistory.length>0&&<><span>Model: {usageHistory[usageHistory.length-1].model}</span><span>Duration: {usageHistory[usageHistory.length-1].duration}s</span></>}
        <TokenTable usage={lastUsage}/>
        <strong>Desktop history · this workspace</strong>
        <span>Completed requests: {usageHistory.length}</span>
        <span>Recorded duration: {numberFormat(usageHistory.reduce((sum,r)=>sum+r.duration,0))}s</span>
        <TokenTable usage={totals(usageHistory)}/>
        <strong>By provider</strong>
        {(['claude','codex'] as const).map(agent => {
          const records = usageHistory.filter(r=>r.provider===agent);
          return <div className="provider-usage" key={agent}><span>{agent.toUpperCase()} · {records.length} requests</span><TokenTable usage={totals(records)}/></div>;
        })}
        <small>Totals are from up to 500 completed requests recorded by this desktop app for this workspace. Older CLI usage and other devices are excluded. Provider counters may be cumulative or incomplete; these totals are not billing figures or context-window usage.</small>
        <button className="clear-usage" disabled={busy||usageHistory.length===0} onClick={()=>{if(window.confirm('Clear desktop usage history for this workspace?')){localStorage.removeItem(USAGE_PREFIX+root);setUsageHistory([]);}}}>Clear desktop history</button>
      </>}
    </div></aside>
    <main>{limitNotice&&<div className="limit-banner" role="status">{limitNotice}</div>}{githubOpen&&<div className="github-panel"><div className="github-heading"><strong>GitHub repositories</strong><button onClick={()=>setGithubOpen(false)}>Close</button></div><p className="hint">Uses your authenticated GitHub CLI (gh). Clone a repository to a local folder before running agents.</p><input aria-label="Search GitHub repositories" placeholder="Search repositories…" value={repoFilter} onChange={e=>setRepoFilter(e.target.value)}/>{loadingRepos&&<p>Loading repositories…</p>}<div className="github-list">{repositories.filter(r=>r.nameWithOwner.toLowerCase().includes(repoFilter.toLowerCase())).map(r=><div className="github-repo" key={r.nameWithOwner}><div><strong>{r.nameWithOwner}</strong><small>{r.isPrivate?'Private':'Public'}</small><p>{r.description}</p></div><button disabled={cloning} onClick={()=>void cloneGithub(r.nameWithOwner)}>{cloning?'Cloning…':'Clone'}</button></div>)}</div></div>}<header><strong>Project session</strong><span>{root || 'Choose a Git repository'}</span></header><section className="messages" aria-live="polite">{messages.length===0&&<p className="empty">Choose a Git workspace and send your first request.</p>}{messages.map((m,i)=><article key={i} className={m.role}><small>{m.role==='user'?'YOU':m.provider.toUpperCase()}</small><pre>{m.text}</pre></article>)}{busy&&liveText&&<article className="streaming"><small>{liveProvider.toUpperCase()} · LIVE</small><pre>{liveText}<span className="stream-cursor">▍</span></pre></article>}{busy&&<p className="hint" role="status">{lastProvider.toUpperCase()} working… {elapsed}s · {activity}</p>}<div ref={messagesEnd} aria-hidden="true" /></section>{error&&<div className="error" role="alert"><p>{error}</p>{pendingSwitch&&<div className="recovery"><p>Claude reached a limit. Codex is now selected for the next request. The failed request was NOT replayed because previous file changes could not be verified. Review Git changes first.</p><button onClick={()=>{setPrompt('Continue the previous task using the current repository state. First inspect existing changes and do not repeat completed operations. Previous request: '+failedPrompt);setPendingSwitch(false);setError('');}}>Prepare safe continuation prompt</button></div>}</div>}<footer className={`composer ${busy ? liveProvider : provider}`}><textarea aria-label="Prompt" value={prompt} onChange={e=>setPrompt(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();void send();}}} placeholder="Ask Claude or Codex…"/><button className={`send-button ${busy ? liveProvider : provider}`} disabled={busy||!root.trim()||!prompt.trim()} onClick={()=>void send()}>{busy?'Running…':'Send →'}</button></footer></main>
  </div>;
}
createRoot(document.getElementById('root')!).render(<React.StrictMode><App/></React.StrictMode>);
