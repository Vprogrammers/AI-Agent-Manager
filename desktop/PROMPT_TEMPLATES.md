# Project-owned prompt optimization

Task templates live in `src/promptTemplates.ts` (version `TEMPLATE_VERSION`).
They are deterministic and local: no provider call or hidden rewriting of the original request.

## Priority of instructions
1. User's original request and explicit constraints (always preserved verbatim).
2. Native project instructions (`AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, or `.github/copilot-instructions.md`), referenced by path, never copied wholesale.
3. Task-specific template (bug, feature, refactor, review, docs, general).
4. Agent-specific guidance, only when useful and within the extra-token budget.

For an existing native agent session, do not repeat project instruction references.
Relevant files are deduplicated and capped at five; paths are hints to inspect, not verified evidence.
No prompt expansion is performed for standalone git status commands.

## Regression benchmark
Run from `desktop`:

```sh
npm run build
npx --yes tsx src/promptOptimizer.test.ts
npx --yes tsx src/promptBenchmark.test.ts
```

`promptBenchmark.test.ts` checks deterministic classification, verbatim preservation,
direct-command bypass and selective context handling. It does **not** measure real
agent execution quality, actual token consumption, latency or cost.

## Future quality evaluation
Run original and optimized prompts on the same isolated repository snapshot and
test suite. Compare equivalent acceptance criteria and test coverage; capture
provider-reported tokens and elapsed time separately. Never claim improved quality
from shorter prompts or heuristic estimates alone.
