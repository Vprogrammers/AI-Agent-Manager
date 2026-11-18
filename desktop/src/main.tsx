import React from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import './style.css';

type Provider = 'claude' | 'codex';
type GithubRepo = { nameWithOwner: string; isPrivate: boolean; description: string | null };
type Message = { role: 'user' | 'assistant'; text: string; provider: Provider };
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
  const [error, setError] = React.useState('');
  const [pendingSwitch, setPendingSwitch] = React.useState(false);
  const [githubOpen, setGithubOpen] = React.useState(false);
  const [repositories, setRepositories] = React.useState<GithubRepo[]>([]);
  const [repoFilter, setRepoFilter] = React.useState('');
  const [loadingRepos, setLoadingRepos] = React.useState(false);
  const [cloning, setCloning] = React.useState(false);
  const selectWorkspace = (path: string) => {
    setRoot(path); setMessages([]); setError(''); setPendingSwitch(false);
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
  const send = async () => {
    const text = prompt.trim();
    if (!text || !root.trim() || busy) return;
    setPrompt(''); setError(''); setBusy(true); setPendingSwitch(false);
    setMessages(m => [...m, { role: 'user', text, provider }]);
    try {
      const answer = await invoke<string>('run_prompt', { root, provider, prompt: text });
      setMessages(m => [...m, { role: 'assistant', text: answer, provider }]);
    } catch (e) {
      const message = String(e);
      setError(message);
      if (provider === 'claude' && /Next request uses codex|automatic retry blocked/i.test(message)) setPendingSwitch(true);
    } finally { setBusy(false); }
  };
  return <div className="app">
    <aside><h2>AI Agent Manager</h2><label>Workspace</label><button className="open-folder" onClick={()=>void chooseFolder()}>Open Folder…</button><button className="open-folder" onClick={()=>void loadGithub()}>Open from GitHub…</button><input aria-label="Git repository path" placeholder="/path/to/git/repo" value={root} onChange={e=>setRoot(e.target.value)} onBlur={()=>{if(root.trim()&&!recent.includes(root.trim()))selectWorkspace(root.trim());}}/><label>Recent workspaces</label><div className="recent">{recent.map(path=><button key={path} title={path} className={path===root?'selected':''} onClick={()=>selectWorkspace(path)}>{path.split('/').filter(Boolean).pop() || path}</button>)}</div><label>Agent</label><div className="agents"><button className={provider==='claude'?'active claude':''} onClick={()=>setProvider('claude')}>Claude</button><button className={provider==='codex'?'active codex':''} onClick={()=>setProvider('codex')}>Codex</button></div><p className="hint">Provider limits may require a manual continuation when prior work cannot be ruled out.</p></aside>
    <main>{githubOpen&&<div className="github-panel"><div className="github-heading"><strong>GitHub repositories</strong><button onClick={()=>setGithubOpen(false)}>Close</button></div><p className="hint">Uses your authenticated GitHub CLI (gh). Clone a repository to a local folder before running agents.</p><input aria-label="Search GitHub repositories" placeholder="Search repositories…" value={repoFilter} onChange={e=>setRepoFilter(e.target.value)}/>{loadingRepos&&<p>Loading repositories…</p>}<div className="github-list">{repositories.filter(r=>r.nameWithOwner.toLowerCase().includes(repoFilter.toLowerCase())).map(r=><div className="github-repo" key={r.nameWithOwner}><div><strong>{r.nameWithOwner}</strong><small>{r.isPrivate?'Private':'Public'}</small><p>{r.description}</p></div><button disabled={cloning} onClick={()=>void cloneGithub(r.nameWithOwner)}>{cloning?'Cloning…':'Clone'}</button></div>)}</div></div>}<header><strong>Project session</strong><span>{root || 'Choose a Git repository'}</span></header><section className="messages" aria-live="polite">{messages.length===0&&<p className="empty">Choose a Git workspace and send your first request.</p>}{messages.map((m,i)=><article key={i} className={m.role}><small>{m.role==='user'?'YOU':m.provider.toUpperCase()}</small><pre>{m.text}</pre></article>)}{busy&&<p className="hint">Agent working…</p>}</section>{error&&<div className="error" role="alert"><p>{error}</p>{pendingSwitch&&<div className="recovery"><p>Claude reached a limit. Automatic replay was blocked because earlier work cannot be excluded. Inspect repository changes before continuing. The next request can use Codex with the saved handoff.</p><button onClick={()=>{setProvider('codex');setPendingSwitch(false);setError('');}}>Switch to Codex for next request</button></div>}</div>}<footer><textarea aria-label="Prompt" value={prompt} onChange={e=>setPrompt(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();void send();}}} placeholder="Ask Claude or Codex…"/><button disabled={busy||!root.trim()||!prompt.trim()} onClick={()=>void send()}>{busy?'Running…':'Send'}</button></footer></main>
  </div>;
}
createRoot(document.getElementById('root')!).render(<React.StrictMode><App/></React.StrictMode>);
