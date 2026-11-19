import { analyzePrompt, isDirectCommand, recommendedStrategy, emptyFeedback } from './promptOptimizer';

function assert(condition:boolean, message:string):void {if(!condition)throw Error(message);}
assert(isDirectCommand('git status'),'git status must bypass expansion');
assert(analyzePrompt('git status').suggestion==='git status','simple command unchanged');
const bug=analyzePrompt('Fix login crash',[],[],'balanced');
assert(bug.kind==='bug','bug detection');
assert(bug.suggestion.includes('root cause'),'bug guidance');
assert(bug.missing.length>0,'missing reproduction');
const concise=analyzePrompt('Implement login screen',[],[],'concise');
const detailed=analyzePrompt('Implement login screen',[],[],'detailed');
assert(detailed.suggestion.length>concise.suggestion.length,'strategy length');
assert(analyzePrompt('Fix login', ['src/login.ts'],['AGENTS.md']).suggestion.includes('src/login.ts'),'project file context');
assert(recommendedStrategy(emptyFeedback())==='balanced','default strategy');
console.log('Prompt optimizer smoke tests passed');
