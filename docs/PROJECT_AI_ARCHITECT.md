# Project AI Architect

A conservative, offline scanner for Git checkouts. It detects technology markers and existing root instructions, then proposes an isolated project-review agent, workflow skill and project inventory.

Preview changes without writing:

```bash
python3 src/project_ai_architect.py --root /path/to/repository
python3 src/project_ai_architect.py --root /path/to/repository --json
```

After reviewing the printed unified diffs, explicitly approve:

```bash
python3 src/project_ai_architect.py --root /path/to/repository --approve
```

**Safety:** only creates files under `.ai-agent-manager/architect/`; never overwrites existing files, including generated ones modified by users. Existing AGENTS.md and CLAUDE.md remain authoritative. The Smart Router detects generated files as additional project context; it does not automatically execute their instructions or change provider selection. Re-run to discover newly missing files. Existing managed files are preserved rather than automatically refreshed.

**Limitations:** stack detection uses file/path heuristics, not semantic source analysis. No LLM-generated agent specialization, automatic change tracking, merge conflict resolution, or in-app approval UI is implemented yet. The `--approve` flag is the approval gate.
