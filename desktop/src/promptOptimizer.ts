/** Local, deterministic prompt suggestions. No provider calls. */
export type Strategy = 'concise'|'balanced'|'detailed';
export type Analysis = {kind:string; clarity:number; missing:string[]; relevantFiles:string[]; instructions:string[]; estimate:number; suggestion:string; unchanged:boolean};
const patterns: [string,RegExp][] = [
 ['bug',/\b(fix|bug|crash|error|broken|poprav|grešk|gresk|ne radi|puca)\b/i],
 ['feature',/\b(implement|add|create|build|dodaj|napravi|implementir)\b/i],
 ['refactor',/\b(refactor|cleanup|restructure|refakt|očisti|ocisti)\b/i],
 ['review',/\b(review|audit|security|analiz|proveri|provjeri|pregled)\b/i],
 ['docs',/\b(readme|documentation|docs|dokument)\b/i],
];
const steps: Record<string,string[]> = {
 bug:['Reproduce the issue if possible and identify the root cause.','Apply the smallest safe fix.','Run relevant regression tests.'],
 feature:['Inspect existing implementation and dependencies.','Implement the requested behavior using project conventions.','Add or update relevant tests.'],
 refactor:['Preserve public behavior and interfaces.','Make focused changes without unrelated rewrites.','Run regression checks.'],
 review:['Report concrete findings with file references and severity.','Do not modify code unless requested.'],
 docs:['Verify claims against the current code.','Update only relevant documentation.'],
 general:['Inspect relevant existing code before changes.','Verify the outcome and report limitations.']
};
export function isDirectCommand(text:string):boolean {
 return /^(?:(?:run|execute|pokreni|izvrši|izvrsi)\s+)?git\s+(?:status|branch|diff|log|rev-parse)(?:\s+[-\w./]+)*\s*$/i.test(text.trim());
}
export function analyzePrompt(text:string, files:string[]=[], instructions:string[]=[], strategy:Strategy='balanced', existingSession=false):Analysis {
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
 const relevantFiles=files.slice(0,5);
 const concise=kind==='general'?[]:steps[kind].slice(0,1);
 const balanced=steps[kind].slice(0,2);
 const detailed=steps[kind];
 const selected=strategy==='concise'?concise:strategy==='balanced'?balanced:detailed;
 const additions:string[]=[];
 if(!existingSession&&instructions.length) additions.push('Follow existing project instructions: '+instructions.slice(0,4).join(', ')+'.');
 if(relevantFiles.length&&strategy!=='concise') additions.push('Check potentially relevant files (verify before editing): '+relevantFiles.join(', ')+'.');
 additions.push(...selected);
 if(strategy==='detailed') additions.push('Report changed files, checks executed, and unresolved risks.');
 if(missing.length&&strategy!=='concise') additions.push('Clarify blocking ambiguity instead of assuming: '+missing.join(' '));
 // Do not expand an already detailed request, or duplicate guidance already present.
 const unique=additions.filter(a=>!original.toLowerCase().includes(a.toLowerCase()));
 const suggestion=original.length>700||!unique.length?original:original+'\n\n'+unique.map(a=>'- '+a).join('\n');
 return {kind,clarity,missing,relevantFiles,instructions,estimate:Math.ceil(suggestion.length/4),suggestion,unchanged:suggestion===original};
}
export type Feedback = {approved:number;edited:number;regenerated:number;rejected:number;strategy:Record<Strategy,number>};
export const emptyFeedback=():Feedback=>({approved:0,edited:0,regenerated:0,rejected:0,strategy:{concise:0,balanced:0,detailed:0}});
export function recommendedStrategy(stats:Feedback):Strategy {
 const entries=(Object.keys(stats.strategy) as Strategy[]).sort((a,b)=>stats.strategy[b]-stats.strategy[a]);
 return stats.strategy[entries[0]]>0?entries[0]:'balanced';
}
