# Operations and validation

Install (does not modify shell configuration): `bash scripts/install.sh`, then ensure `~/.local/bin` is on PATH. Use `aam codex --root /path/to/repo` or `aam claude --root /path/to/repo`.

Commands: `/claude`, `/codex`, `/new`, `/status`, `/exit`. Ctrl-C during execution sends SIGINT to the native CLI process group; escalation to SIGTERM/SIGKILL only if needed. Inspect repository changes before resuming.

Progress is emitted to stderr; it is event-based, not a guaranteed percent-complete indicator. Duration and provider-reported token counts are printed after each completed subprocess. Usage ledger is stored locally in the target Git directory: `ai-agent-manager/usage.jsonl`. Missing provider token counts are reported as unavailable, not zero. No cost is estimated.

Cross-provider handoff is bounded and requires a fresh review of repository state. Native session IDs cannot transfer across providers. Claude rate-limit failures automatically retry the same prompt once on Codex **only** when Claude explicitly reports zero turns, zero executed turns, zero cost, no reported token usage (API wall time may be nonzero), and Git repository state is unchanged. All other failures schedule the alternate provider for the next request without replay. No automatic replay after interruption or uncertain side effects.

CI checks Python syntax and unit tests on 3 Python versions. Native provider E2E tests require local CLI authentication and are intentionally not run in public CI. No changes to Gustero are required.
