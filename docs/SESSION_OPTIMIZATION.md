# Session optimization

Six conservative features are available in AI-Agent-Manager:

1. **Context budget:** `/context` displays last observed provider input tokens, not the exact active context size. `AAM_CONTEXT_WARNING_TOKENS` (default 100000) warns after a completed turn. No automatic session reset.
2. **Cache-aware sessions:** Existing native session IDs are reused for each provider/profile. `AAM_FORCE_NEW_SESSION=1` explicitly disables reuse. Cache hit rates depend on providers; no guaranteed savings.
3. **Automatic checkpoints:** Every `AAM_CHECKPOINT_EVERY_TURNS` successful turns (default 8), save `checkpoint.json` under the target repository's Git directory. `/checkpoint` saves immediately. Checkpoints are deterministic and never call an AI provider.
4. **Adaptive model routing:** Existing complexity-based LOW/MEDIUM/HIGH environment variables select models per turn. Defaults remain the provider's own choice; configure real model IDs explicitly.
5. **Token analytics:** `/analytics` aggregates provider-reported tokens by provider/profile from local `usage.jsonl`. Cached tokens may be included in input totals. No price estimates or invented savings.
6. **Bounded session summarization:** `AAM_HANDOFF_HISTORY_CHARS` (default 4000) bounds cross-provider history. `AAM_SUMMARY_CHARS` (default 1200) bounds checkpoint excerpts. These are deterministic excerpts, **not** semantic AI summaries, to avoid extra token cost or loss of critical decisions.

These features never truncate native provider history, interrupt an active turn, commit files, or edit Gustero. Checkpoints are stored in the target repository's Git directory, not tracked. On cross-provider switches, review repository changes and prior work before acting.
