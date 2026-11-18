import React from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import './style.css';

type Provider = 'claude' | 'codex';
type Message = { role: 'user' | 'assistant'; text: string; provider: Provider };
function App() {
  const [root, setRoot] = React.useState('');
  const [provider, setProvider] = React.useState<Provider>('claude');
  const [prompt, setPrompt] = React.useState('');
  const [messages, setMessages] = React.useState<Message[]>([]);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState('');
  const send = async () => {
    const text = prompt.trim();
    if (!text || !root.trim() || busy) return;
    setPrompt(''); setError(''); setBusy(true);
    setMessages(m => [...m, { role: 'user', text, provider }]);
    try {
      const answer = await invoke<string>('run_prompt', { root, provider, prompt: text });
      setMessages(m => [...m, { role: 'assistant', text: answer, provider }]);
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  return <div className="app">
    <aside><h2>AI Agent Manager</h2><label>Workspace</label><input aria-label="Git repository path" placeholder="/path/to/git/repo" value={root} onChange={e=>setRoot(e.target.value)}/><label>Agent</label><div className="agents"><button className={provider==='claude'?'active claude':''} onClick={()=>setProvider('claude')}>Claude</button><button className={provider==='codex'?'active codex':''} onClick={()=>setProvider('codex')}>Codex</button></div><p className="hint">Uses the existing AAM CLI and its saved project sessions. Install and authenticate the provider CLIs first.</p></aside>
    <main><header><strong>Project session</strong><span>{root || 'Choose a Git repository'}</span></header><section className="messages" aria-live="polite">{messages.length===0&&<p className="empty">Select a workspace and send your first request.</p>}{messages.map((m,i)=><article key={i} className={m.role}><small>{m.role==='user'?'YOU':m.provider.toUpperCase()}</small><pre>{m.text}</pre></article>)}{busy&&<p className="hint">Agent working…</p>}</section>{error&&<p className="error" role="alert">{error}</p>}<footer><textarea aria-label="Prompt" value={prompt} onChange={e=>setPrompt(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();void send();}}} placeholder="Ask Claude or Codex…"/><button disabled={busy||!root.trim()||!prompt.trim()} onClick={()=>void send()}>{busy?'Running…':'Send'}</button></footer></main>
  </div>;
}
createRoot(document.getElementById('root')!).render(<React.StrictMode><App/></React.StrictMode>);
