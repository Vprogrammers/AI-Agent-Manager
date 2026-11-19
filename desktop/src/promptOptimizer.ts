import { TASK_TEMPLATES, selectProjectContext, type TaskKind } from './promptTemplates';
/** Local, deterministic prompt suggestions. No provider calls. */
/** Conservative equivalence: only well-known instruction aliases, never remove user requirements. */
export function semanticInstructionKey(line:string):string {
 const normalized=line.toLowerCase().trim().replace(/[.!?]+$/,'').replace(/\s+/g,' ');
 const aliases:[RegExp,string][]=[
  [/^(?:run|execute) (?:the )?tests?$/,'run tests'],
  [/^(?:test|verify) (?:the )?(?:changes|fix)$/,'verify changes'],
  [/^(?:do not|don't|never) (?:change|modify) unrelated (?:files|code)$/,'preserve unrelated code'],
  [/^(?:keep|preserve) unrelated (?:code|behavior|behaviour)$/,'preserve unrelated code']
 ];
 return aliases.find(([pattern])=>pattern.test(normalized))?.[1]||normalized;
}
export function dedupeGuidance(lines:string[]):string[] {
 const seen=new Set<string>();
 return lines.filter(line=>{const key=semanticInstructionKey(line);if(seen.has(key))return false;seen.add(key);return true;});
}
export type Agent = 'claude'|'codex'|'other';
export type PromptMetrics = {originalEstimate:number; proposedEstimate:number; deltaEstimate:number};
export function estimateTokens(text:string):number {return Math.ceil(text.length/4);}
export function promptDiff(original:string, proposed:string):{added:string[];removed:string[]} {
 const before=new Set(original.split('\n').map(x=>x.trim()).filter(Boolean));
 const after=new Set(proposed.split('\n').map(x=>x.trim()).filter(Boolean));
 return {added:[...after].filter(x=>!before.has(x)),removed:[...before].filter(x=>!after.has(x))};
}
export function validateSuggestion(original:string, proposed:string):{safe:boolean;issues:string[]} {
 const issues:string[]=[];
 if(!proposed.includes(original.trim())) issues.push('Original request is not preserved verbatim.');
 const critical=/\b(?:do not|don't|never|without|only|must|nemoj|nikad|samo|bez|ne sme|ne smije)\b/i;
 const constraints=original.split(/[\n.!?]+/).map(x=>x.trim()).filter(x=>critical.test(x));
 for(const constraint of constraints) if(!proposed.includes(constraint)) issues.push('A constraint may be missing: '+constraint.slice(0,90));
 return {safe:issues.length===0,issues};
}
export function followupContext(text:string, previous?:string):string {
 if(!previous||!/(?:^|\s)(?:now|continue|again|same|that|this|sada|nastavi|opet|isto|to|ovo)(?:\s|$)/i.test(text))return '';
 return 'Related prior user request (verify current repository state): '+previous.slice(0,350);
}
export type Strategy = 'concise'|'balanced'|'detailed';
export type Analysis = {kind:string; clarity:number; missing:string[]; relevantFiles:string[]; instructions:string[]; estimate:number; suggestion:string; unchanged:boolean};
const patterns: [string,RegExp][] = [
 ['bug',/\b(fix|bug|crash|error|broken|poprav|grešk|gresk|ne radi|puca)\b/i],
 ['feature',/\b(implement|add|create|build|dodaj|napravi|implementir)\b/i],
 ['refactor',/\b(refactor|cleanup|restructure|refakt|očisti|ocisti)\b/i],
 ['review',/\b(review|audit|security|analiz|proveri|provjeri|pregled)\b/i],
 ['docs',/\b(readme|documentation|docs|dokument)\b/i],
];
export function isDirectCommand(text:string):boolean {
 return /^(?:(?:run|execute|pokreni|izvrši|izvrsi)\s+)?git\s+(?:status|branch|diff|log|rev-parse)(?:\s+[-\w./]+)*\s*$/i.test(text.trim());
}
export function analyzePrompt(text:string, files:string[]=[], instructions:string[]=[], strategy:Strategy='balanced', existingSession=false, agent:Agent='other', previousRequest='', maxExtraTokens=120):Analysis {
 const original=text.trim();
 if (!original) return {kind:'general',clarity:0,missing:[],relevantFiles:[],instructions:[],estimate:0,suggestion:'',unchanged:true};
 if (isDirectCommand(original)) return {kind:'command',clarity:100,missing:[],relevantFiles:[],instructions:[],estimate:Math.ceil(original.length/4),suggestion:original,unchanged:true};
 const kind=patterns.find(([,pattern])=>pattern.test(original))?.[0]||'general';
 const missing:string[]=[];
 const hasTarget=/\b(android|ios|macos|web|backend|frontend|kmp|kotlin|swift|rust|python|react|typescript|login|auth|api|database|ui|screen|module|file|component)\b/i.test(original)||files.length>0;
 if(!hasTarget) missing.push('Specify the affected module or behavior if the repository does not make it clear.');
 if(!/\b(when|after|before|expected|should|fails|crash|error|kada|nakon|oček|treba|ne radi)\b/i.test(original)&&kind==='bug') missing.push('Describe how to reproduce the problem and the expected result.');
 if(original.length<25&&kind==='general') missing.push('Clarify the intended outcome.');
 const clarity=Math.max(10,Math.min(95,95-missing.length*25-(original.length<35?15:0)));
 const context=selectProjectContext(files,instructions,existingSession);
 const relevantFiles=context.files;
 const concise=kind==='general'?[]:TASK_TEMPLATES[kind as TaskKind].slice(0,1);
 const balanced=TASK_TEMPLATES[kind as TaskKind].slice(0,2);
 const detailed=TASK_TEMPLATES[kind as TaskKind];
 const selected=strategy==='concise'?concise:strategy==='balanced'?balanced:detailed;
 const additions:string[]=[];
 const prior=followupContext(original,previousRequest);
 if(prior) additions.push(prior);
 if(context.instructions.length) additions.push('Follow existing project instructions: '+context.instructions.join(', ')+'.');
 if(relevantFiles.length&&strategy!=='concise') additions.push('Check potentially relevant files (verify before editing): '+relevantFiles.join(', ')+'.');
 additions.push(...selected);
 if(agent==='claude'&&strategy==='detailed')additions.push('Explain the reasoning behind nontrivial design decisions concisely.');
 if(agent==='codex'&&strategy==='detailed')additions.push('Prefer focused patches and report concrete test commands.');
 if(strategy==='detailed') additions.push('Report changed files, checks executed, and unresolved risks.');
 if(missing.length&&strategy!=='concise') additions.push('Clarify blocking ambiguity instead of assuming: '+missing.join(' '));
 // Do not expand an already detailed request, or duplicate guidance already present.
 const originalKeys=new Set(original.split(/[\n.!?]+/).map(semanticInstructionKey));
 const unique=dedupeGuidance(additions).filter(a=>!originalKeys.has(semanticInstructionKey(a))&&!original.toLowerCase().includes(a.toLowerCase()));
 const budget=Math.max(0,maxExtraTokens)*4;
 const chosen:string[]=[];let remaining=budget;
 for(const item of unique){const line='- '+item+'\n';if(line.length<=remaining){chosen.push(line.trimEnd());remaining-=line.length;}}
 const suggestion=original.length>700||!chosen.length?original:original+'\n\n'+chosen.join('\n');
 return {kind,clarity,missing,relevantFiles,instructions,estimate:Math.ceil(suggestion.length/4),suggestion,unchanged:suggestion===original};
}
export type Feedback = {approved:number;edited:number;regenerated:number;rejected:number;strategy:Record<Strategy,number>};
export const emptyFeedback=():Feedback=>({approved:0,edited:0,regenerated:0,rejected:0,strategy:{concise:0,balanced:0,detailed:0}});
export function recommendedStrategy(stats:Feedback):Strategy {
 const entries=(Object.keys(stats.strategy) as Strategy[]).sort((a,b)=>stats.strategy[b]-stats.strategy[a]);
 return stats.strategy[entries[0]]>0?entries[0]:'balanced';
}

export type ExecutionEvidence = {testsPassed?:number;testsFailed?:number;completed?:boolean;durationSeconds?:number;reportedInputTokens?:number};
export function compareExecutionEvidence(original:ExecutionEvidence,optimized:ExecutionEvidence):{verdict:'better'|'worse'|'inconclusive';reason:string} {
 if(original.testsFailed===undefined||optimized.testsFailed===undefined||original.testsPassed===undefined||optimized.testsPassed===undefined)
  return {verdict:'inconclusive',reason:'Both runs need comparable test results.'};
 if(original.testsPassed+original.testsFailed!==optimized.testsPassed+optimized.testsFailed)
  return {verdict:'inconclusive',reason:'Test coverage differs between runs.'};
 if(optimized.testsFailed<original.testsFailed)return {verdict:'better',reason:'Fewer failing tests with equal test counts.'};
 if(optimized.testsFailed>original.testsFailed)return {verdict:'worse',reason:'More failing tests with equal test counts.'};
 return {verdict:'inconclusive',reason:'Equal test results; token and duration metrics alone do not prove quality.'};
}
