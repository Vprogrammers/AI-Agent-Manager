import React from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import './style.css';
import AgentSetupWizard from './AgentSetupWizard';
import { analyzePrompt, recommendedStrategy, emptyFeedback, promptDiff, validateSuggestion, estimateTokens, type Agent, type Strategy, type Feedback } from './promptOptimizer';

type Provider = 'claude' | 'codex' | 'antigravity' | 'opencode' | 'copilot';
type RoutingMode = Provider | 'auto' | 'balanced';

type AiConfigItem = { name:string; path:string; kind:string; claude:'supported'|'unsupported'|'unknown'; codex:'supported'|'unsupported'|'unknown'; claudeReason:string; codexReason:string };
function AiStatus({status,reason}: {status:AiConfigItem['claude'];reason:string}) {
  return <span className={'ai-status '+status} title={reason} aria-label={status+': '+reason}>{status==='supported'?'Compatible':status==='unsupported'?'Not native':'Unverified'}</span>;
}

type ArchitectProposal = { path:string; action:string; reason:string; before:string|null; after:string };
type ArchitectPlan = { stack:string[]; proposals:ArchitectProposal[] };
type GithubRepo = { nameWithOwner: string; isPrivate: boolean; description: string | null };
type Message = { role: 'user' | 'assistant'; text: string; provider: Provider; usage?: TokenUsage; model?: string; duration?: number };
const TOKEN_LABELS: Record<string,string> = {
  input_tokens: 'Input',
  cached_input_tokens: 'Cached input',
  cache_read_input_tokens: 'Cached input',
  cache_creation_input_tokens: 'Cache creation',
  output_tokens: 'Output',
  reasoning_output_tokens: 'Reasoning output',
  agent_turns: 'Agent turns (Claude)',
};
type TokenUsage = Record<string, number>;
type UsageRecord = { provider: Provider; model: string; duration: number; usage: TokenUsage; time: string };
const USAGE_PREFIX = 'aam.desktop.usage.v1:';
function loadUsage(path: string): UsageRecord[] {
  if (!path) return [];
  try {
    const value: unknown = JSON.parse(localStorage.getItem(USAGE_PREFIX + path) || '[]');
    return Array.isArray(value) ? value.filter((v): v is UsageRecord =>
      !!v && (['claude', 'codex', 'antigravity', 'opencode', 'copilot'].includes(v.provider)) &&
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
type AgentStep = { time:string; label:string; status:'running'|'done'|'info' };
function describeAgentStep(ev: Record<string,unknown>): AgentStep | null {
  const type = String(ev.type || '');
  const item = ev.item && typeof ev.item === 'object' ? ev.item as Record<string,unknown> : {};
  const itemType = String(item.type || '');
  const command = itemType === 'command_execution' ? String(item.command || '') : '';
  // Commands may contain secrets in arguments. Show the operation type, not raw CLI arguments.
  const labels: Record<string,string> = {
    command_execution:'Executing shell command', file_change:'Editing project files',
    mcp_tool_call:'Calling external tool', web_search:'Searching documentation',
    reasoning:'Analyzing', agent_message:'Preparing response'
  };
  if (type === 'turn.started') return {time:new Date().toLocaleTimeString(),label:'Starting agent turn',status:'running'};
  if (type === 'turn.completed') return {time:new Date().toLocaleTimeString(),label:'Agent turn completed',status:'done'};
  if (type === 'item.started' || type === 'item.completed' || type === 'item.updated') {
    if (!labels[itemType] && !command) return null;
    if (type === 'item.updated') return null;
    return {time:new Date().toLocaleTimeString(),label:labels[itemType] || 'Processing tool',status:type === 'item.started'?'running':'done'};
  }
  if (type === 'tool_use') return {time:new Date().toLocaleTimeString(),label:'Using '+String(ev.name || 'tool').slice(0,60),status:'running'};
  if (type === 'tool_result') return {time:new Date().toLocaleTimeString(),label:'Tool finished',status:'done'};
  if (type === 'content_block_start') {
    const block = ev.content_block as Record<string,unknown> | undefined;
    if (block?.type === 'tool_use') return {time:new Date().toLocaleTimeString(),label:'Using '+String(block.name || 'tool').slice(0,60),status:'running'};
  }
  return null;
}

function TokenTable({usage}: {usage:TokenUsage}) {
  const rows = usageRows(usage);
  if (!rows.length) return <p className="token-empty">Not reported by provider</p>;
  const input = usage.input_tokens;
  const cached = usage.cached_input_tokens ?? usage.cache_read_input_tokens;
  // Codex input includes cached tokens; Claude reports cache reads/creation separately.
  const isInclusive = typeof input === 'number' && typeof cached === 'number' && input >= cached;
  const newInput = typeof input === 'number' ? (isInclusive ? input - cached : input) : null;
  const processedInput = typeof input === 'number'
    ? input + (isInclusive ? 0 : (cached ?? 0)) + (usage.cache_creation_input_tokens ?? 0)
    : null;
  const processed = processedInput === null ? null : processedInput + (usage.output_tokens ?? 0);
  const row = (label:string, value:number|string) => <div className="token-row" key={label}><span>{label}</span><strong className="token-value">{typeof value === 'number' ? numberFormat(value) : value}</strong></div>;
  return <div className="token-stats">
    {processed !== null && row('Total processed (reported)', processed)}
    {newInput !== null && row('New input', newInput)}
    {typeof cached === 'number' && row('Cached input', cached)}
    {typeof usage.cache_creation_input_tokens === 'number' && row('Cache creation', usage.cache_creation_input_tokens)}
    {typeof usage.output_tokens === 'number' && row('Output', usage.output_tokens)}
    {row('Internal model calls', 'Not exposed by CLI')}
    {typeof usage.agent_turns === 'number' && row('Claude agent turns', usage.agent_turns)}
    <details><summary>Raw provider counters</summary>{rows.map(([label,value],i)=><div className="token-row" key={i}><span>{label}</span><span className="token-value">{numberFormat(value)}</span></div>)}</details>
    <small>Provider counters are not directly comparable across Claude and Codex. Total processed is an estimate from reported counters, not billing or quota usage. Claude agent turns are measured separately from model calls; Codex JSON does not expose per-model-call events.</small>
  </div>;
}

const WORKSPACES_KEY = 'aam.desktop.workspaces.v1';
function savedWorkspaces(): string[] {
  try { const value: unknown = JSON.parse(localStorage.getItem(WORKSPACES_KEY) || '[]'); return Array.isArray(value) ? value.filter((v): v is string => typeof v === 'string').slice(0, 8) : []; }
  catch { return []; }
}
function previewPrompt(text: string): {optimized:string; removed:number} {
  const seen = new Set<string>();
  let fenced = false;
  let removed = 0;
  const lines: string[] = [];
  for (const line of text.split('\n')) {
    const key = line.trim();
    if (key.startsWith('```') || key.startsWith('~~~')) { fenced = !fenced; lines.push(line); continue; }
    if (!fenced && key.length >= 24 && seen.has(key)) { removed++; continue; }
    lines.push(line);
    if (!fenced && key) seen.add(key);
  }
  return {optimized:lines.join('\n'),removed};
}
function expandPrompt(text: string): {draft:string; questions:string[]} {
  const request = text.trim();
  if (!request) return {draft:'',questions:[]};
  const questions: string[] = [];
  if (!/\b(android|ios|macos|web|backend|frontend|kmp|kotlin|swift|rust|python|react|typescript|platform|repo|project)\b/i.test(request))
    questions.push('Which project, platform, or component is in scope?');
  if (!/\b(test|verify|acceptance|expected|result|pass|prover|testir|oček|prihvat)\b/i.test(request))
    questions.push('What observable result or test will confirm success?');
  if (!/\b(only|except|without|must not|do not|nemoj|samo|bez|ne menjaj|ne mijenjaj)\b/i.test(request))
    questions.push('Are there files or behaviors that must remain unchanged?');
  const draft = [
    'Task (original request; preserve its meaning):',
    request,
    '',
    'Execution instructions:',
    '- Inspect the existing project structure and relevant code before editing.',
    '- Reuse existing architecture, conventions, and dependencies.',
    '- Do not invent requirements or silently choose among ambiguous alternatives.',
    '- If a missing detail blocks safe implementation, ask for clarification.',
    '- Make the smallest change that meets the confirmed request.',
    '- Run relevant available checks and report exactly what was executed.',
    '- Report changed files, results, and unresolved issues.',
    ...(questions.length ? ['', 'Open questions (do not assume answers):', ...questions.map(q=>'- '+q)] : [])
  ].join('\n');
  return {draft,questions};
}
const SESSION_PREFIX = 'aam.desktop.session.v1:';
function loadSession(path: string): Message[] {
  if (!path) return [];
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(SESSION_PREFIX + path) || '[]');
    if (!Array.isArray(saved)) return [];
    return saved.filter((m): m is Message =>
      m !== null && typeof m === 'object' && (m.role === 'user' || m.role === 'assistant') &&
      typeof m.text === 'string' && typeof m.provider === 'string'
    ).slice(-100);
  } catch { return []; }
}

function App() {
  const [root, setRoot] = React.useState('');
  const [aiConfig, setAiConfig] = React.useState<AiConfigItem[]>([]);
  const [aiConfigError, setAiConfigError] = React.useState('');
  const [aiConfigLoading, setAiConfigLoading] = React.useState(false);
  const [aiConfigOpen, setAiConfigOpen] = React.useState(false);
  const scanAiConfig = React.useCallback(async (path:string) => {
    if (!path.trim()) { setAiConfig([]); return; }
    setAiConfigLoading(true); setAiConfigError('');
    try { setAiConfig(await invoke<AiConfigItem[]>('project_ai_config',{root:path})); }
    catch(e) { setAiConfig([]); setAiConfigError(String(e)); }
    finally { setAiConfigLoading(false); }
  },[]);
  const [architect, setArchitect] = React.useState<ArchitectPlan|null>(null);
  const [architectOpen, setArchitectOpen] = React.useState(false);
  const [architectLoading, setArchitectLoading] = React.useState(false);
  const [architectSelection, setArchitectSelection] = React.useState<string[]>([]);
  const [architectError, setArchitectError] = React.useState('');
  const [architectSuccess, setArchitectSuccess] = React.useState('');
  const scanArchitect = React.useCallback(async (path:string) => {
    setArchitect(null); setArchitectSelection([]); setArchitectError(''); setArchitectSuccess('');
    if (!path.trim()) return;
    setArchitectLoading(true);
    try {
      const plan = await invoke<ArchitectPlan>('architect_preview',{root:path});
      setArchitect(plan); setArchitectSelection(plan.proposals.map(p=>p.path));
    } catch(e) { setArchitectError(String(e)); }
    finally { setArchitectLoading(false); }
  },[]);
  const [recent, setRecent] = React.useState<string[]>(savedWorkspaces);
  const [provider, setProvider] = React.useState<RoutingMode>('auto');
  const [prompt, setPrompt] = React.useState('');
  const [messages, setMessages] = React.useState<Message[]>([]);
  React.useEffect(() => {
    if (!root) return;
    try { localStorage.setItem(SESSION_PREFIX + root, JSON.stringify(messages.slice(-100))); } catch { /* storage may be disabled or full */ }
  }, [root, messages]);
  const [pendingPrompt, setPendingPrompt] = React.useState<string | null>(null);
  const [suggestedPrompt, setSuggestedPrompt] = React.useState('');
  const [strategy, setStrategy] = React.useState<Strategy>('balanced');
  const [maxExtraTokens,setMaxExtraTokens] = React.useState(120);
  const [optimizerMetrics,setOptimizerMetrics] = React.useState<{runs:number;addedEstimate:number;reportedInput:number;reportedOutput:number}>({runs:0,addedEstimate:0,reportedInput:0,reportedOutput:0});
  const [feedback, setFeedback] = React.useState<Feedback>(()=>{try { const saved=JSON.parse(localStorage.getItem('aam.optimizer.feedback.v1')||'null'); return saved&&saved.strategy? saved as Feedback:emptyFeedback(); } catch{return emptyFeedback();}});
  const [relevantPaths, setRelevantPaths] = React.useState<string[]>([]);
  const [optimizerInstructions, setOptimizerInstructions] = React.useState<string[]>([]);

  const previousRequest = React.useMemo(()=>[...messages].reverse().find(m=>m.role==='user')?.text||'', [messages]);
  const agentType:Agent = provider==='claude'?'claude':provider==='codex'?'codex':'other';
  const optimizerAnalysis = React.useMemo(()=>analyzePrompt(pendingPrompt||'',relevantPaths,optimizerInstructions,strategy,messages.length>0,agentType,previousRequest,maxExtraTokens),[pendingPrompt,relevantPaths,optimizerInstructions,strategy,messages.length,agentType,previousRequest,maxExtraTokens]);
  const optimizerDiff = React.useMemo(()=>promptDiff(pendingPrompt||'',suggestedPrompt),[pendingPrompt,suggestedPrompt]);
  const optimizerSafety = React.useMemo(()=>validateSuggestion(pendingPrompt||'',suggestedPrompt),[pendingPrompt,suggestedPrompt]);
  const trackOptimizer = (action:'approved'|'edited'|'regenerated'|'rejected') => {
    setFeedback(prev=>{const next={...prev,[action]:prev[action]+1,strategy:{...prev.strategy,[strategy]:prev.strategy[strategy]+1}};localStorage.setItem('aam.optimizer.feedback.v1',JSON.stringify(next));return next;});
  };
  React.useEffect(()=>{if(!pendingPrompt||!root){setRelevantPaths([]);setOptimizerInstructions([]);return;}let active=true;
    void invoke<string[]>('relevant_prompt_files',{root,prompt:pendingPrompt}).then(paths=>{if(active)setRelevantPaths(paths);}).catch(()=>{if(active)setRelevantPaths([]);});
    void invoke<AiConfigItem[]>('project_ai_config',{root}).then(config=>{if(active)setOptimizerInstructions(config.map(x=>x.path).filter(Boolean).slice(0,4));}).catch(()=>{if(active)setOptimizerInstructions([]);});
    return ()=>{active=false;};
  },[pendingPrompt,root]);
  React.useEffect(()=>{if(pendingPrompt)setSuggestedPrompt(optimizerAnalysis.suggestion);},[optimizerAnalysis,pendingPrompt]);
  const [permission, setPermission] = React.useState<'read-only'|'workspace-write'|'full-access'>('workspace-write');
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
  const [agentSteps, setAgentSteps] = React.useState<AgentStep[]>([]);
  const [showSteps, setShowSteps] = React.useState(true);
  const [timing, setTiming] = React.useState<{provider:string;preflightMs:number;executionMs:number;totalMs:number}|null>(null);
  const [liveText, setLiveText] = React.useState('');
  const [liveProvider, setLiveProvider] = React.useState<Provider>('claude');
  const requestSeq = React.useRef(0);
  const messagesEnd = React.useRef<HTMLDivElement>(null);
  React.useEffect(() => { messagesEnd.current?.scrollIntoView({ block: 'end', behavior: 'smooth' }); }, [messages, liveText]);
  const [githubOpen, setGithubOpen] = React.useState(false);
  const [repositories, setRepositories] = React.useState<GithubRepo[]>([]);
  const [repoFilter, setRepoFilter] = React.useState('');
  const [loadingRepos, setLoadingRepos] = React.useState(false);
  const [cloning, setCloning] = React.useState(false);
  const [diagnostics, setDiagnostics] = React.useState<Array<{name:string;status:string;detail:string}>>([]);
  const [diagnosticsOpen, setDiagnosticsOpen] = React.useState(false);
  const [stopping, setStopping] = React.useState(false);
  const refreshDiagnostics = async () => { try { setDiagnostics(await invoke('desktop_diagnostics',{root})); } catch(e) { setError(String(e)); } };
  React.useEffect(()=>{void refreshDiagnostics();},[root]);
  const selectWorkspace = (path: string) => {
    if (busy) { setError('Wait for the current request to finish or stop it before changing workspaces.'); return; }
    setRoot(path); void scanAiConfig(path); void scanArchitect(path); setUsageHistory(loadUsage(path)); setLastUsage({}); setUsage('Not reported'); setMessages(loadSession(path)); setError(''); setPendingSwitch(false); setFailedPrompt('');
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
  const clearSession = async () => {
    if (!root.trim() || busy) return;
    if (!window.confirm('Start a fresh Claude/Codex session for this workspace? Saved conversation references and handoff context will be cleared. Usage history and files will remain.')) return;
    try {
      await invoke('clear_agent_session', {root});
      localStorage.removeItem(SESSION_PREFIX + root);
      setMessages([]); setPrompt(''); setLastUsage({}); setUsage('Not reported');
      setError(''); setActivity(''); setLiveText(''); setLimitNotice('');
      setPendingSwitch(false); setFailedPrompt('');
    } catch (e) { setError('Cannot clear session: ' + String(e)); }
  };
  const submitPrompt = () => {
    const original=prompt.trim();
    if(!original||busy||!root.trim())return;
    if(original==='/clear'||original==='/new'){void clearSession();return;}
    const chosen=recommendedStrategy(feedback);
    setStrategy(chosen);setPendingPrompt(original);
    setSuggestedPrompt(analyzePrompt(original,[],[],chosen,messages.length>0,agentType,previousRequest,maxExtraTokens).suggestion);
  };
  const regenerateSuggestion = () => {
    if(!pendingPrompt)return;
    const next:Strategy=strategy==='balanced'?'concise':strategy==='concise'?'detailed':'balanced';
    trackOptimizer('regenerated');setStrategy(next);
  };
  const send = async (approvedText?: string) => {
    const text = (approvedText ?? prompt).trim();
    if (text === '/clear' || text === '/new') { await clearSession(); return; }
    if (!text || !root.trim() || busy) return;
    const id = String(++requestSeq.current);
    const requestedProvider = provider;
    const requestRoot = root;
    if (permission === 'full-access' && !window.confirm('Full access allows the agent to execute commands and modify files outside the workspace without normal sandbox protection. Continue for this request?')) return;
    if(pendingPrompt) setOptimizerMetrics(prev=>({...prev,runs:prev.runs+1,addedEstimate:prev.addedEstimate+estimateTokens(text)-estimateTokens(pendingPrompt)}));
    setPrompt(''); setPendingPrompt(null); setSuggestedPrompt(''); setError(''); setLimitNotice(''); setUsage('Waiting for provider'); setLastUsage({});
    setLastProvider(requestedProvider); setActivity('Starting agent…'); setLiveText(''); setLiveProvider(requestedProvider === 'codex' ? 'codex' : 'claude');
    setBusy(true); setStartedAt(Date.now()); setElapsed(0); setPendingSwitch(false);
    setMessages(m => [...m, { role: 'user', text, provider: requestedProvider === 'codex' ? 'codex' : 'claude' }]);
    const requestStarted = Date.now();
    let completed = false;
    setTiming(null);
    setAgentSteps([]);
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
        if (kind === 'timing') {
          setTiming({provider:String(data.provider || ''),preflightMs:Number(data.preflightMs || 0),executionMs:Number(data.executionMs || 0),totalMs:Number(data.totalMs || 0)});
        } else if (kind === 'routing') {
          setActivity('Auto: '+String(data.reason || 'Routing')+' · '+String(data.model || 'CLI default'));
        } else if (kind === 'provider' && (['claude', 'codex', 'antigravity', 'opencode', 'copilot'].includes(String(data.provider)))) {
          const selectedProvider = String(data.provider) as Provider;
          setLastProvider(selectedProvider); setLiveProvider(selectedProvider); if (requestedProvider !== 'auto' && requestedProvider !== 'balanced') setProvider(selectedProvider); setActivity('Running '+selectedProvider);
        } else if (kind === 'activity') {
          const ev = data.event as Record<string,unknown> | undefined;
          if (ev) {
            const step = describeAgentStep(ev);
            if (step) setAgentSteps(previous=>[...previous,step].slice(-120));
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
          const actual = (['claude', 'codex', 'antigravity', 'opencode', 'copilot'].includes(String(data.provider)) ? data.provider : 'claude') as Provider;
          setLastProvider(actual); if (requestedProvider !== 'auto' && requestedProvider !== 'balanced') setProvider(actual);
          const counts = data.usage as Record<string,unknown> | undefined;
          const parsed = numericUsage(counts);
          setLastUsage(parsed); setUsage(Object.keys(parsed).length ? 'Reported' : 'Not reported');
          const record: UsageRecord = {provider:actual,model:String(data.model || 'Unknown'),duration:Math.round((Date.now()-requestStarted)/1000),usage:parsed,time:new Date().toISOString()};
          setUsageHistory(previous => { const next=[...previous,record].slice(-500); try { localStorage.setItem(USAGE_PREFIX+requestRoot,JSON.stringify(next)); } catch {} return next; });
          setMessages(m => [...m, {role:'assistant', text:String(data.answer || streamText), provider:actual, usage:parsed, model:record.model, duration:record.duration}]);
          setLiveText('');
          setActivity('Completed · '+String(data.model || actual));
        } else if (kind === 'error') {
          failed = true;
          setError(String(data.message || 'Agent failed')); setPrompt(current=>current||text);
          const pending = data.pending_provider;
          if (pending === 'codex' || pending === 'claude') {
            if (requestedProvider !== 'auto' && requestedProvider !== 'balanced') setProvider(pending); setLastProvider(String(data.provider || requestedProvider));
            setFailedPrompt(text); setPendingSwitch(true);
            setLimitNotice('Provider limit reached. '+pending+' selected for next request; unsafe replay was blocked.');
          }
          setActivity('Request failed');
          if (streamText) { setMessages(m=>[...m,{role:'assistant',text:streamText+'\n\n[Partial response — request failed]',provider:liveProvider}]); setLiveText(''); }
        } else if (kind === 'process_error') {
          failed = true; setError(String(data.message || 'CLI process failed')); setPrompt(current=>current||text);
        }
      });
      await invoke<void>('run_prompt', {requestId:id,root:requestRoot,provider:requestedProvider,prompt:text,permission});
      if (!completed && !failed) setError('Agent finished without a structured result. Check CLI version.');
    } catch (e) { setError(String(e)); setPrompt(current => current || text); if (streamText) { setMessages(m=>[...m,{role:'assistant',text:streamText+'\n\n[Partial response — interrupted]',provider:requestedProvider === 'codex' ? 'codex' : 'claude'}]);setLiveText(''); } }
    finally { unlisten?.(); setBusy(false); setStopping(false); setStartedAt(null); }
  };
  return <div className="app">
    <aside className={`workspace-panel ${busy ? liveProvider : provider}`}><h2>AI Agent Manager</h2><AgentSetupWizard/><label>Workspace</label><button className="open-folder" disabled={busy} onClick={()=>void chooseFolder()}>Open Folder…</button><button className="open-folder" disabled={busy} onClick={()=>void loadGithub()}>Open from GitHub…</button><input aria-label="Git repository path" disabled={busy} placeholder="/path/to/git/repo" value={root} onChange={e=>setRoot(e.target.value)} onBlur={()=>{if(root.trim()&&!recent.includes(root.trim()))selectWorkspace(root.trim());}}/><label>Recent workspaces</label><div className="recent">{recent.map(path=><button key={path} title={path} className={path===root?'selected':''} disabled={busy} onClick={()=>selectWorkspace(path)}>{path.split('/').filter(Boolean).pop() || path}</button>)}</div><label>Permissions</label><select aria-label="Agent permissions" value={permission} disabled={busy} onChange={e=>setPermission(e.target.value as typeof permission)}><option value="read-only">Read only</option><option value="workspace-write">Workspace write</option><option value="full-access">Full access (unsafe)</option></select><p className="hint">{provider==='codex'?'Codex read-only and workspace-write use CLI filesystem sandboxing. Full access bypasses sandbox and approvals.':'Claude read-only restricts allowed tool names, not an OS sandbox. Workspace write uses acceptEdits (other operations may still be allowed). Full access skips permissions.'}</p><button disabled={busy || !root.trim()} onClick={()=>void clearSession()}>Clear session</button><p className="hint">Or type /clear. Resets Claude/Codex conversation context, not provider login or usage history.</p><label>Agent routing</label><div className="agents"><button disabled={busy} className={provider==='auto'?'active codex':''} onClick={()=>setProvider('auto')}>Auto</button><button disabled={busy} className={provider==='balanced'?'active codex':''} onClick={()=>setProvider('balanced')}>Balanced</button><button disabled={busy} className={provider==='claude'?'active claude':''} onClick={()=>setProvider('claude')}>Claude</button><button disabled={busy} className={provider==='codex'?'active codex':''} onClick={()=>setProvider('codex')}>Codex</button><button disabled={busy} className={provider==='antigravity'?'active codex':''} onClick={()=>setProvider('antigravity')}>Gemini</button><button disabled={busy} className={provider==='opencode'?'active codex':''} onClick={()=>setProvider('opencode')}>OpenCode</button><button disabled={busy} className={provider==='copilot'?'active codex':''} onClick={()=>setProvider('copilot')}>Copilot</button></div><p className="hint">Auto uses local task rules and existing sessions to reduce handoffs; Balanced prioritizes session reuse. No extra AI routing call. Manual Claude/Codex/Gemini/OpenCode/Copilot selection is available. Auto/Balanced currently route only Claude/Codex. Provider limits never justify replaying uncertain edits.</p><div className="ai-config-panel"><div className="usage-heading"><strong>Setup & Diagnostics</strong><button onClick={()=>setDiagnosticsOpen(v=>!v)}>{diagnosticsOpen?'Hide':'Show'}</button></div>{diagnosticsOpen&&<><button onClick={()=>void refreshDiagnostics()}>Refresh checks</button>{diagnostics.map(d=><div className="diagnostic-item" key={d.name}><strong>{d.name}</strong><span className={'ai-status '+(d.status==='ready'?'supported':d.status==='missing'?'unsupported':'unknown')}>{d.status}</span><p>{d.detail}</p></div>)}<small>CLI detection does not verify authentication. Sign in using the provider CLI before your first request.</small></>}</div><div className="ai-config-panel">
      <div className="usage-heading"><strong>Project AI configuration</strong><button onClick={()=>setAiConfigOpen(v=>!v)} aria-expanded={aiConfigOpen}>{aiConfigOpen?'Hide':'Show'}</button></div>
      {aiConfigOpen&&<>
        <button className="ai-config-refresh" disabled={!root.trim()||aiConfigLoading} onClick={()=>void scanAiConfig(root)}>{aiConfigLoading?'Scanning…':'Refresh scan'}</button>
        {aiConfigError&&<p className="ai-config-error">{aiConfigError}</p>}
        {!aiConfigLoading&&!aiConfigError&&aiConfig.length===0&&<p className="hint">No recognized project-level AI configuration found. Global settings and nested instructions are not scanned.</p>}
        {aiConfig.length>0&&<div className="ai-config-items">{aiConfig.map(item=><div className="ai-config-item" key={item.path}>
          <strong title={item.path}>{item.path}</strong><small>{item.kind}</small>
          <div><span>Claude</span><AiStatus status={item.claude} reason={item.claudeReason}/></div>
          <p>{item.claudeReason}</p>
          <div><span>Codex</span><AiStatus status={item.codex} reason={item.codexReason}/></div>
          <p>{item.codexReason}</p>
        </div>)}</div>}
        <small>Compatibility is inferred from conventional file locations, not verified runtime loading. Skills are activated conditionally. Global settings, nested AGENTS.md/CLAUDE.md and custom configuration may change behavior.</small>
      </>}
    </div><div className="ai-config-panel">
      <div className="usage-heading"><strong>Project AI Architect</strong><button onClick={()=>setArchitectOpen(v=>!v)} aria-expanded={architectOpen}>{architectOpen?'Hide':'Show'}</button></div>
      {architectOpen&&<>
        <p className="hint">Detect project technologies and propose agent instructions, skills and a review subagent. No files change until you approve.</p>
        <button disabled={!root||busy||architectLoading} onClick={()=>void scanArchitect(root)}>{architectLoading?'Scanning…':'Rescan project'}</button>
        {architect?.stack.length ? <p className="hint">Detected: {architect.stack.join(' · ')}</p> : null}
        {architectError&&<p className="ai-config-error" role="alert">{architectError}</p>}
        {architectSuccess&&<p className="hint" role="status">{architectSuccess}</p>}
        {architect&&!architect.proposals.length&&<p className="hint">All generated configuration is up to date.</p>}
        {architect?.proposals.map(item=><div className="architect-proposal" key={item.path}>
          <label className="architect-select"><input type="checkbox" checked={architectSelection.includes(item.path)} onChange={e=>setArchitectSelection(old=>e.target.checked?[...old,item.path]:old.filter(p=>p!==item.path))}/><strong>{item.action.toUpperCase()} {item.path}</strong></label>
          <p className="hint">{item.reason}</p>
          <details><summary>Review proposed change</summary><div className="architect-diff">
            {item.before!==null&&<><strong>Existing file (preserved outside managed section)</strong><pre>{item.before}</pre></>}
            <strong>Proposed complete file</strong><pre>{item.after}</pre>
          </div></details>
        </div>)}
        {architect&&architect.proposals.length>0&&<button disabled={busy||architectLoading||architectSelection.length===0} onClick={async()=>{
          if (!window.confirm('Apply '+architectSelection.length+' reviewed AI configuration changes to this workspace?')) return;
          setArchitectLoading(true); setArchitectError(''); setArchitectSuccess('');
          try {
            const selected=architect.proposals.filter(p=>architectSelection.includes(p.path));
            const written=await invoke<string[]>('architect_apply',{root,proposals:selected});
            await scanAiConfig(root);
            await scanArchitect(root);
            setArchitectSuccess('Applied: '+written.join(', '));
          } catch(e){setArchitectError(String(e));}
          finally{setArchitectLoading(false);}
        }}>Approve and apply selected ({architectSelection.length})</button>}
        <small>Proposals are deterministic and do not invoke an AI model. Review all changes. Existing configuration is not deleted; generated sections may be updated after approval.</small>
      </>}
    </div><div className="usage-panel">
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
    <main>{timing&&<div className="hint" role="status" style={{padding:'8px 24px',borderBottom:'1px solid #303741'}}>Timing · {timing.provider} · preflight {timing.preflightMs} ms · execution {timing.executionMs} ms · total {timing.totalMs} ms</div>}{limitNotice&&<div className="limit-banner" role="status">{limitNotice}</div>}{githubOpen&&<div className="github-panel"><div className="github-heading"><strong>GitHub repositories</strong><button onClick={()=>setGithubOpen(false)}>Close</button></div><p className="hint">Uses your authenticated GitHub CLI (gh). Clone a repository to a local folder before running agents.</p><input aria-label="Search GitHub repositories" placeholder="Search repositories…" value={repoFilter} onChange={e=>setRepoFilter(e.target.value)}/>{loadingRepos&&<p>Loading repositories…</p>}<div className="github-list">{repositories.filter(r=>r.nameWithOwner.toLowerCase().includes(repoFilter.toLowerCase())).map(r=><div className="github-repo" key={r.nameWithOwner}><div><strong>{r.nameWithOwner}</strong><small>{r.isPrivate?'Private':'Public'}</small><p>{r.description}</p></div><button disabled={cloning} onClick={()=>void cloneGithub(r.nameWithOwner)}>{cloning?'Cloning…':'Clone'}</button></div>)}</div></div>}<header><strong>Project session</strong><span>{root || 'Choose a Git repository'}</span></header><section className="messages" aria-live="polite">{messages.length===0&&<p className="empty">Choose a Git workspace and send your first request.</p>}{messages.map((m,i)=><article key={i} className={`${m.role} ${m.role === "assistant" ? `provider-${m.provider}` : ""}`}><small>{m.role==='user'?'YOU':m.provider.toUpperCase()}</small><pre>{m.text}</pre>{m.role==='assistant'&&m.usage&&<details className="prompt-usage"><summary>Tokens for this prompt · {m.model} · {m.duration}s</summary><TokenTable usage={m.usage}/><small>Provider-reported turn usage, including repeated cached context. Uncached input is input minus cached input, not just your typed text. Raw numeric diagnostics: .git/ai-agent-manager/usage-diagnostics.jsonl.</small></details>}</article>)}{busy&&liveText&&<article className="streaming"><small>{liveProvider.toUpperCase()} · LIVE</small><pre>{liveText}<span className="stream-cursor">▍</span></pre></article>}{agentSteps.length>0&&<div className="agent-steps"><button className="agent-steps-toggle" onClick={()=>setShowSteps(v=>!v)} aria-expanded={showSteps}>Agent activity · {agentSteps.length} events · {showSteps?'Hide':'Show'}</button>{showSteps&&<ol className="agent-steps-list">{agentSteps.map((step,i)=><li key={i} className={'agent-step-'+step.status}><span className="agent-step-time">{step.time}</span><span>{step.label}</span></li>)}</ol>}</div>}{busy&&<p className="hint" role="status">{lastProvider.toUpperCase()} working… {elapsed}s · {activity}</p>}<div ref={messagesEnd} aria-hidden="true" /></section>{error&&<div className="error" role="alert"><p>{error}</p>{pendingSwitch&&<div className="recovery"><p>Claude reached a limit. Codex is now selected for the next request. The failed request was NOT replayed because previous file changes could not be verified. Review Git changes first.</p><button onClick={()=>{setPrompt('Continue the previous task using the current repository state. First inspect existing changes and do not repeat completed operations. Previous request: '+failedPrompt);setPendingSwitch(false);setError('');}}>Prepare safe continuation prompt</button></div>}</div>}<footer className={`composer ${busy ? liveProvider : provider}`}>{pendingPrompt!==null&&<div className="prompt-suggestion" role="region" aria-label="Suggested prompt" style={{padding:12,border:'1px solid var(--border, #777)',borderRadius:8,margin:8}}>
<strong>Suggested improved prompt</strong>
<p className="hint">Local scripts only · no AI calls · {optimizerAnalysis.kind} · clarity heuristic {optimizerAnalysis.clarity}% · {strategy} · ~{optimizerAnalysis.estimate} tokens (rough estimate)</p>
{optimizerAnalysis.missing.length>0&&<p className="hint">Missing details: {optimizerAnalysis.missing.join(' ')}</p>}
{relevantPaths.length>0&&<p className="hint">Candidate files: {relevantPaths.join(', ')}</p>}
<div style={{display:'flex',gap:8,alignItems:'center'}}><label htmlFor="prompt-strategy">Style</label><select id="prompt-strategy" value={strategy} onChange={e=>setStrategy(e.target.value as Strategy)}><option value="concise">Concise</option><option value="balanced">Balanced</option><option value="detailed">Detailed</option></select></div>
<div style={{display:'flex',gap:8,alignItems:'center',flexWrap:'wrap'}}><label htmlFor="optimizer-budget">Max added tokens (estimate)</label><input id="optimizer-budget" type="number" min="0" max="500" value={maxExtraTokens} onChange={e=>setMaxExtraTokens(Math.max(0,Math.min(500,Number(e.target.value)||0)))} style={{width:70}}/></div>
<details><summary>Review changes</summary><p className="hint">Added lines: {optimizerDiff.added.length} · Removed lines: {optimizerDiff.removed.length}</p><pre style={{whiteSpace:'pre-wrap',maxHeight:130,overflow:'auto'}}>{optimizerDiff.added.map(x=>'+ '+x).join('\n')||'No additions'}</pre>{optimizerDiff.removed.length>0&&<pre style={{whiteSpace:'pre-wrap'}}>{optimizerDiff.removed.map(x=>'- '+x).join('\n')}</pre>}</details>
{!optimizerSafety.safe&&<p role="alert">Safety check: {optimizerSafety.issues.join(' ')}</p>}
<textarea aria-label="Edit suggested prompt" value={suggestedPrompt} onChange={e=>setSuggestedPrompt(e.target.value)} rows={9} style={{width:'100%'}}/>
<p className="hint">Original ~{estimateTokens(pendingPrompt)} tokens · proposed ~{estimateTokens(suggestedPrompt)} tokens (character-based estimates, not billing). No quality improvement is claimed.</p>
<div style={{display:'flex',gap:8,flexWrap:'wrap'}}>
<button disabled={busy||!suggestedPrompt.trim()||!optimizerSafety.safe} onClick={()=>{trackOptimizer(suggestedPrompt===optimizerAnalysis.suggestion?'approved':'edited');void send(suggestedPrompt);}}>Approve & send</button>
<button disabled={busy} onClick={regenerateSuggestion}>Try another suggestion</button>
<button disabled={busy} onClick={()=>{trackOptimizer('edited');setPrompt(suggestedPrompt);setPendingPrompt(null);}}>Edit in chat</button>
<button disabled={busy} onClick={()=>{trackOptimizer('rejected');void send(pendingPrompt);}}>Reject & send original</button>
<button disabled={busy} onClick={()=>{setPendingPrompt(null);setSuggestedPrompt('');}}>Cancel</button>
</div><p className="hint">Local usage: {optimizerMetrics.runs} submitted · ~{optimizerMetrics.addedEstimate} net added tokens (estimate, not savings). Feedback: {feedback.approved} accepted · {feedback.edited} edited · {feedback.rejected} rejected. No prompt text stored in feedback.</p>
</div>}<textarea aria-label="Prompt" value={prompt} onChange={e=>setPrompt(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();submitPrompt();}}} placeholder={busy ? `Working with ${liveProvider === "antigravity" ? "Gemini" : liveProvider.charAt(0).toUpperCase() + liveProvider.slice(1)}…` : provider === "auto" ? "Ask an agent (Auto)…" : provider === "balanced" ? "Ask an agent (Balanced)…" : `Ask ${provider === "antigravity" ? "Gemini" : provider === "opencode" ? "OpenCode" : provider === "copilot" ? "Copilot" : provider.charAt(0).toUpperCase() + provider.slice(1)}…`}/>{busy&&<button className="stop-button" disabled={stopping} onClick={()=>{setStopping(true);void invoke('cancel_prompt',{requestId:String(requestSeq.current)}).catch(e=>setError(String(e)));}}>{stopping?'Stopping…':'Stop'}</button>}<button className={`send-button ${busy ? liveProvider : provider}`} disabled={busy||!root.trim()||!prompt.trim()||diagnostics.some(d=>d.name==='Git'&&d.status==='missing')} onClick={submitPrompt}>{busy?'Running…':'Send →'}</button></footer></main>
  </div>;
}
createRoot(document.getElementById('root')!).render(<React.StrictMode><App/></React.StrictMode>);
