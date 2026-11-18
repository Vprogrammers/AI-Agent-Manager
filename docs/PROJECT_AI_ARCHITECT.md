# Project AI Architect

Project-aware, offline source analysis and approval-gated configuration refresh.

## Interactive terminal and Textual workflow

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

The standalone CLI uses the same managed generation and refresh engine:

```bash
python3 src/project_ai_architect.py --root /path/to/project
python3 src/project_ai_architect.py --root /path/to/project --approve
```

**Safety:** generated files live only under `.ai-agent-manager/architect/`. A SHA-256 manifest tracks the last approved generated contents. Files edited by users are marked `conflict` and preserved. Refresh checks for changed files between preview and apply. Existing `AGENTS.md` and `CLAUDE.md` are never modified. No AI provider is executed during analysis.

**Limitations:** analysis is bounded static syntax/import inspection, not full cross-language semantic resolution or an LLM-based architecture review. Interactive preview and approval are available in the standard terminal and Textual UI. Both CLI and Textual now register a refresh manifest on approval. Unmodified legacy standalone files can be adopted; edited files remain conflicts. Tests run in GitHub Actions CI; provider-authenticated end-to-end tests still require local CLI credentials.
