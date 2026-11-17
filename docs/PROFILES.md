# Codex environment profiles (opt-in)

The default profile preserves existing behavior. Set `AAM_CODEX_PROFILE=auto` for heuristic selection, or choose `minimal`, `coding`, or `full`.

- `default` / `full`: original Codex configuration, plugins and MCP integrations.
- `minimal`: temporary isolated `CODEX_HOME`, with local auth copied into it, without global config/plugins/MCP.
- `coding`: same isolated environment; built-in local coding tools remain available. **Not a filesystem sandbox.**
- `auto`: full for external integrations, coding for implementation tasks, minimal for other prompts.

**Caution:** Isolated `CODEX_HOME` may break native Codex session resume between turns. For ongoing multi-turn work and reliable handoff, use `default` or `full` until session persistence has been tested on your CLI version.

## Test

```bash
python3 -m unittest discover -s tests -v
AAM_CODEX_PROFILE=auto aam codex --root .
```

## Compare token usage

```bash
python3 src/codex_benchmark.py --model gpt-6.1-terra --runs 5 --profiles default minimal coding full --output /tmp/aam-benchmark.json
```

The benchmark makes real API requests, which consume quota. Each run uses a fresh empty working directory and the same read-only prompt. Reported token usage includes cache and is not a monetary estimate. Provider system instructions and built-in tool definitions cannot be removed by this tool.
