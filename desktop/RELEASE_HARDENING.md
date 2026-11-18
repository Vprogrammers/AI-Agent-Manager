# Desktop release and autonomy hardening

Implementation tracking for the AI Agent Manager desktop application. This file is not a claim that all work is finished.

## Release gates

- [ ] macOS x86_64 and arm64 DMG builds succeed on GitHub Actions and pass install/launch smoke tests on clean machines
- [ ] Sign and notarize macOS app and DMGs with configured Apple credentials
- [ ] Verify packaged engine discovers and authenticates Claude Code and Codex CLI from Finder/Dock
- [ ] Introduce reproducible npm lockfile, `npm ci`, pinned build dependencies and verified checksums
- [ ] Add Windows and Linux packaging, platform CI and install smoke tests
- [ ] Add signed updater manifests and an opt-in auto-update flow after signing

## Runtime and safety

- [ ] Provider process-group cancellation on macOS/Linux/Windows, tested with actual agent child processes
- [ ] Durable request IDs, workspace identity and recovery after app restart
- [ ] Failover continuation after partial changes with explicit handoff checkpoint and Git-diff verification; never replay unknown side effects
- [ ] Provider-specific permissions verified against installed CLI versions; avoid presenting Claude tool allowlists as OS sandboxing
- [ ] Preflight provider authentication, version and capability diagnostics
- [ ] Git diff preview, safe rollback controls and explicit approval for destructive operations

## Token intelligence

- [ ] Verify provider usage counters are per-turn rather than cumulative before aggregating
- [ ] Persist separate per-request, per-session, per-project and per-provider totals
- [ ] Report cached-token ratios only with compatible denominators; don't claim cost savings without validated prices
- [ ] Benchmark session reuse and bounded handoff against a documented baseline

All items require executable tests and documented results before marking complete.
