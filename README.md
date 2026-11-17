# AI Agent Manager

Reusable, **opt-in** Claude Code ↔ Codex CLI bridge. No Gustero files or settings are modified.

## Requirements

Python 3.10+, Git, and installed/authenticated `claude` and/or `codex` CLI. Start inside a Git repository. No Python dependencies.

## Usage

```bash
git clone https://github.com/Vprogrammers/AI-Agent-Manager.git
python3 /path/to/AI-Agent-Manager/src/ai_agent_manager.py codex --root /path/to/your/project
# or:
python3 /path/to/AI-Agent-Manager/src/ai_agent_manager.py claude --root /path/to/your/project
```

Inside the bridge: `/claude`, `/codex`, `/exit`. To start without prior bridge sessions use `--new`; for a single prompt use `--prompt "Summarize README; do not edit files"`.

Model routing is optional: `AAM_CODEX_LOW_MODEL`, `AAM_CODEX_MEDIUM_MODEL`, `AAM_CODEX_HIGH_MODEL`, and corresponding `AAM_CLAUDE_...` variables. If unset, the native CLI selects its default model. Selection is heuristic and occurs **between turns**.

State is stored under the **target repository's Git directory** in `ai-agent-manager/agent-bridge.json`, not in tracked files. Optional project handoff: `.ai-agent-manager/HANDOFF.md`. Cross-provider handoff includes bounded recent explicit turns and current Git facts, not native provider context. Review uncommitted changes before continuing. On a suspected provider limit, the next request may switch provider; failed requests are **not automatically replayed**.

## Portable features\n\nStreaming progress, per-turn duration and reported token usage, local JSONL ledger, /new, /status, and controlled interruption. See [operations](docs/OPERATIONS.md).\n\n## Safety and compatibility

- **Does not** install shell aliases, replace native `claude` / `codex`, modify `AGENTS.md`, or commit/push target repositories.
- Do not run simultaneously against the same target repo: concurrent state writes are not coordinated.
- A CLI failure may still have modified files; inspect `git status` before retrying.
- CLI output formats and session resume syntax may change. Verify with your installed CLI versions.
- The native Gustero terminal UI and shell alias patches are intentionally not copied. This project has its own streaming and opt-in installer.
- Python unit tests: `python3 -m unittest discover -s tests -v`.

## License

MIT.


## Session optimization

See [Session optimization](docs/SESSION_OPTIMIZATION.md). Commands: `/context`, `/checkpoint`, `/analytics`.
