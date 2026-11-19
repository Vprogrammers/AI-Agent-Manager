/** Versioned, deterministic task templates. Project instructions remain authoritative. */
export type TaskKind = 'bug'|'feature'|'refactor'|'review'|'docs'|'general';
export const TEMPLATE_VERSION = 1;
export const TASK_TEMPLATES: Record<TaskKind, readonly string[]> = {
 bug: ['Reproduce the issue if possible and identify the root cause.', 'Apply the smallest safe fix.', 'Run relevant regression tests.'],
 feature: ['Inspect existing implementation and dependencies.', 'Implement the requested behavior using project conventions.', 'Add or update relevant tests.'],
 refactor: ['Preserve public behavior and interfaces.', 'Make focused changes without unrelated rewrites.', 'Run regression checks.'],
 review: ['Report concrete findings with file references and severity.', 'Do not modify code unless requested.'],
 docs: ['Verify claims against the current code.', 'Update only relevant documentation.'],
 general: ['Inspect relevant existing code before changes.', 'Verify the outcome and report limitations.']
};
const projectInstruction = /(?:^|\/)(?:AGENTS\.md|CLAUDE\.md|GEMINI\.md|\.github\/copilot-instructions\.md)$/i;
/** References only; never injects entire instruction files or their unverified contents. */
export function selectProjectContext(files: string[], instructions: string[], existingSession: boolean, maxFiles = 5): {files:string[]; instructions:string[]} {
 const unique = (items:string[]) => [...new Set(items.map(x=>x.trim()).filter(x=>x && !x.includes('\n') && !x.includes('\r')))];
 return {files:unique(files).slice(0,Math.max(0,maxFiles)),
  instructions:existingSession ? [] : unique(instructions).filter(x=>projectInstruction.test(x)).slice(0,4)};
}
