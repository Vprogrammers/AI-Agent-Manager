import { analyzePrompt, isDirectCommand, recommendedStrategy, emptyFeedback, validateSuggestion, promptDiff, followupContext, dedupeGuidance, compareExecutionEvidence } from './promptOptimizer';
import { it } from 'vitest';

it('validates prompt optimizer behavior and safeguards', () => {

function assert(condition:boolean, message:string):void {if(!condition)throw Error(message);}
assert(isDirectCommand('git status'),'git status must bypass expansion');
assert(analyzePrompt('git status').suggestion==='git status','simple command unchanged');
const bug=analyzePrompt('Fix login',[],[],'balanced');
assert(bug.kind==='bug','bug detection');
assert(bug.suggestion.includes('root cause'),'bug guidance');
assert(bug.missing.length>0,'missing reproduction');
const concise=analyzePrompt('Implement login screen',[],[],'concise');
const detailed=analyzePrompt('Implement login screen',[],[],'detailed');
assert(detailed.suggestion.length>concise.suggestion.length,'strategy length');
assert(analyzePrompt('Fix login', ['src/login.ts'],['AGENTS.md']).suggestion.includes('src/login.ts'),'project file context');
assert(recommendedStrategy(emptyFeedback())==='balanced','default strategy');
console.log('Prompt optimizer smoke tests passed');

assert(validateSuggestion('Do not delete files','Do not delete files\nCheck tests').safe,'constraint preserved');
assert(!validateSuggestion('Do not delete files','Delete files').safe,'unsafe edit rejected');
assert(promptDiff('fix login','fix login\nrun tests').added.length===1,'diff additions');
assert(followupContext('Now add tests','Implement login').includes('Implement login'),'followup reference');
assert(analyzePrompt('Implement login',[],[],'detailed',false,'codex','',0).unchanged,'zero budget');

assert(dedupeGuidance(['Run tests','Execute tests']).length===1,'semantic alias dedup');
assert(compareExecutionEvidence({testsPassed:5,testsFailed:1},{testsPassed:6,testsFailed:0}).verdict==='better','quality evidence comparison');
assert(compareExecutionEvidence({},{testsPassed:1,testsFailed:0}).verdict==='inconclusive','missing evidence must not claim improvement');

});