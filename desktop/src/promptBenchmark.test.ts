import { analyzePrompt, validateSuggestion, isDirectCommand } from './promptOptimizer';
import { TASK_TEMPLATES, TEMPLATE_VERSION, selectProjectContext } from './promptTemplates';

function assert(value:boolean, message:string):void {if(!value)throw Error(message);}
const cases = [
 {request:'Fix login crash on Android. Do not change auth API.',kind:'bug'},
 {request:'Implement SwiftUI login screen for iOS. Must support VoiceOver.',kind:'feature'},
 {request:'Refactor Kotlin repository without changing public API.',kind:'refactor'},
 {request:'Review security of backend authentication. Do not edit files.',kind:'review'},
 {request:'Update README documentation for the Python CLI.',kind:'docs'},
 {request:'Investigate the failing build in this repository.',kind:'bug'},
 {request:'git status -sb',kind:'command'},
 {request:'Fix src/login.ts when login fails. Keep existing tests.',kind:'bug'},
 {request:'Create a React component, only change src/ui.tsx.',kind:'feature'},
 {request:'Review the API implementation, report severity and file paths.',kind:'review'}
] as const;
assert(TEMPLATE_VERSION===1,'template version must be explicit');
assert(Object.keys(TASK_TEMPLATES).length===6,'all task kinds have templates');
for (const c of cases) {
 const result=analyzePrompt(c.request,['src/login.ts','src/login.ts'],['AGENTS.md','CLAUDE.md'],'detailed',false,'codex','',300);
 assert(result.kind===c.kind,'classification: '+c.request);
 assert(validateSuggestion(c.request,result.suggestion).safe,'requirements preserved: '+c.request);
 assert(result.suggestion.includes(c.request),'original request preserved: '+c.request);
 if(isDirectCommand(c.request))assert(result.unchanged,'direct command must remain unchanged');
}
assert(selectProjectContext(['a','a','b'],['AGENTS.md','untrusted.txt'],false).files.join(',')==='a,b','deduplicate paths');
assert(selectProjectContext([],['AGENTS.md','untrusted.txt'],false).instructions.join(',')==='AGENTS.md','allow only recognized project instructions');
assert(selectProjectContext([],['AGENTS.md'],true).instructions.length===0,'do not repeat native session instructions');
assert(analyzePrompt('Fix login',[],['AGENTS.md'],'detailed',true).suggestion.indexOf('Follow existing project instructions')<0,'existing session should not repeat project context');
console.log('Prompt benchmark passed: '+cases.length+' deterministic cases');
