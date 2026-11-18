# Project AI Architect

Project-aware, offline source analysis and approval-gated configuration refresh.

## Interactive Textual workflow

Enter `/architect` to inspect the proposed changes and unified diffs. After reviewing them, enter `/architect-apply` to approve. Existing user-edited files are reported as conflicts and never overwritten. Smart Router discovers the generated files as project context.

## API

```python
from project_architect_refresh import analyze, refresh_plan, refresh_apply

facts = analyze("/path/to/git-project")
changes = refresh_plan("/path/to/git-project")  # preview only
for change in changes:
    print(change["status"], change["path"], change["diff"])
refresh_apply("/path/to/git-project", changes, approved=True)
```

The original standalone CLI remains available for initial conservative generation:

```bash
python3 src/project_ai_architect.py --root /path/to/project
python3 src/project_ai_architect.py --root /path/to/project --approve
```

**Safety:** generated files live only under `.ai-agent-manager/architect/`. A SHA-256 manifest tracks the last approved generated contents. Files edited by users are marked `conflict` and preserved. Refresh checks for changed files between preview and apply. Existing `AGENTS.md` and `CLAUDE.md` are never modified. No AI provider is executed during analysis.

**Limitations:** analysis is bounded static syntax/import inspection, not full cross-language semantic resolution or an LLM-based architecture review. Interactive approval currently applies to the Textual UI; other UIs retain the standalone CLI workflow. The initial standalone CLI does not register a refresh manifest, so its earlier generated files are treated as user-owned until regenerated through the refresh workflow. No tests have been executed in this environment.
