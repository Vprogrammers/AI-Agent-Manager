# AI-Agent-Manager

A lightweight, opt-in terminal bridge between **Claude Code** and **OpenAI Codex**. Work in any Git repository, switch providers without manually copying recent context, reuse native sessions, and inspect reported token usage. The bridge does **not** replace either CLI.

> **Important:** AAM is an orchestrator, not a permission sandbox. Claude and Codex can execute tools and modify your project according to their own settings. Review changes and permissions before running potentially destructive tasks.

## 1. Requirements

- macOS or Linux with a terminal; Python **3.10+** and Git.
- At least one installed and authenticated CLI: `claude` (Claude Code) or `codex` (OpenAI Codex).
- A **Git repository** as the target project. AAM stores its state under that repository's Git directory.
- No third-party Python packages are required.

Check the tools:

```bash
python3 --version
git --version
claude --version
codex --version
```

Install and authenticate Claude/Codex using their official instructions. AAM does not log in to either provider on your behalf. You may use only one CLI, but automatic switching requires both.

## 2. Install AAM

```bash
git clone https://github.com/Vprogrammers/AI-Agent-Manager.git
cd AI-Agent-Manager
bash scripts/install.sh
```

The installer writes a small `aam` launcher to `~/.local/bin/aam`. It **does not** change your shell configuration. On zsh, add the directory to `PATH` if needed:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
command -v aam
```

The launcher points to this checkout. Keep the cloned directory in place. To update:

```bash
cd /path/to/AI-Agent-Manager
git pull --ff-only
python3 -m unittest discover -s tests -v
```

No reinstall is normally needed because the launcher runs the source file from your checkout.

**Without installing**, run `python3 /path/to/AI-Agent-Manager/src/ai_agent_manager.py` with the same arguments described below.

## 3. Quick start

In the Git project you want to work on:

```bash
cd /path/to/your/project
aam claude --root .
# or
aam codex --root .
```

Start a fresh AAM session (discard saved bridge session references for that project):

```bash
aam claude --root . --new
```

You can also specify an absolute target path from anywhere:

```bash
aam codex --root /path/to/your/project
```

On an interactive terminal, AAM clears the screen **at startup**, displays the project and active agent, then shows a prompt such as `claude>` or `codex>`. It does not clear the screen on provider switches, so you can still see earlier output. To preserve the existing screen at startup, use `AAM_NO_CLEAR=1 aam claude --root .`.

Example conversation:

```text
claude> Review the authentication code. Do not modify files.
claude> Implement the fix and run relevant tests.
claude> /status
claude> /codex
codex> Review the changes made by Claude and identify regressions.
codex> /analytics
codex> /exit
```

**One-shot mode** runs one prompt and exits, suitable for scripts:

```bash
aam codex --root . --prompt "Summarize README.md. Do not edit files."
```

Interactive screen clearing is disabled in one-shot/non-TTY mode.

## 4. Interactive commands

These commands are entered **inside** the AAM prompt, not in your shell.

| Command | Behavior |
| --- | --- |
| `/claude` | Select Claude for the next prompt, if installed. |
| `/codex` | Select Codex for the next prompt, if installed. |
| `/new` | Clear saved bridge state and start a new bridge session; also clear the interactive terminal. |
| `/status` | Show current agent, stored session keys, last agent and any pending switch. |
| `/context` | Show the last observed provider input-token counts. **Not** an exact context-window meter. |
| `/analytics` | Aggregate locally recorded provider-reported tokens and durations by provider/profile. |
| `/checkpoint` | Write a local snapshot of bridge metadata and bounded recent turns. |
| `/recover` | **Display** the latest valid checkpoint; does not automatically restore it. |
| `/exit`, `/quit` | Leave AAM. |

`Ctrl+C` during a provider request signals the child CLI process group and can escalate to termination. It does **not** guarantee that no files were modified. Inspect `git status` before repeating interrupted work.

## 5. How Claude ↔ Codex switching works

AAM maintains **separate native sessions** for Claude and for each Codex profile. Session IDs are not portable between providers. When switching, AAM supplies a bounded **handoff** containing recent turns from the other agent and Git facts (branch, HEAD, status and diff statistics). If present, it also includes the project's `.ai-agent-manager/HANDOFF.md`.

Example:

```text
claude> Add tests for the parser.
claude> /codex
codex> Review the parser tests and fix failures.
codex> /claude
claude> Continue based on the changes made by Codex.
```

The returning agent may resume its older native session, but it also receives a delta handoff. **AAM does not transfer the full private context or reasoning of another provider.** Always verify actual source files, Git changes and test results.

### Automatic provider-limit fallback

If Claude explicitly rejects a request due to quota **before work is reported**, with zero reported token usage/cost and an unchanged repository fingerprint, AAM may automatically send the **same prompt once** to Codex. This is the safe-retry path.

If the previous provider may have executed tools or changed files, AAM **does not replay** the request automatically. Instead, it records a pending switch for the **next** prompt. AAM can also schedule a switch after other recognized provider-limit failures; a pending switch is not proof that the previous request did no work.

Example of the safe path:

```text
claude> Explain the project architecture. Do not edit files.
[aam] Claude rejected before execution. Switching to Codex; retrying the same request once.
AI-Agent-Manager | CODEX READY
codex> 
```

The actual answer is printed before the next prompt. **Do not assume every quota error triggers automatic replay.** Provider output formats and external side effects limit what AAM can verify.

## 6. Sessions, checkpoints and local files

AAM stores data in the **target project's Git directory**, typically:

```text
your-project/.git/ai-agent-manager/
  agent-bridge.json   # saved bridge sessions and recent history
  usage.jsonl         # append-only per-request usage records
  checkpoint.json     # bounded checkpoint metadata
```

In a Git worktree, the actual Git directory may be elsewhere; AAM resolves it using `git rev-parse --absolute-git-dir`.

- Running without `--new` attempts to reuse saved session references.
- `--new` or `/new` clears the bridge's saved session references; it does **not** delete native Claude/Codex history or your source files.
- `/checkpoint` captures metadata, recent excerpts, session IDs and the last completed request. It is **not** a complete backup of code or a semantic AI summary.
- `/recover` only displays the checkpoint. It does **not** restore files, Git state or native sessions.
- A checkpoint is automatically written every **8 successful turns** by default.
- The optional `.ai-agent-manager/HANDOFF.md` is a normal project file: add it yourself if you want persistent instructions or decisions included in handoffs. Review it before committing, because it may contain sensitive details.

Avoid simultaneous AAM instances targeting the same Git repository: state and usage writes are not coordinated between processes.

## 7. Token usage, context and cost

Run `/context` for last observed input-token values. This does **not** measure remaining context capacity or guarantee when a provider will compact its history.

Run `/analytics` for grouped counters:

```json
{
  "codex/coding": {
    "turns": 2,
    "input": 5000,
    "cached": 3500,
    "output": 300,
    "seconds": 18.5
  }
}
```

These are **illustrative numbers**, not live measurements. The `input` counter may **include** cached input; do not add `cached` to `input` as if they were disjoint. Missing provider counters are not reliable zeros. Durations are wall-clock subprocess times, not API latency. AAM does not calculate billing cost or claim a verified savings percentage.

The JSONL ledger also records provider, model, profile, status, duration and an optional task ID. To label new records for your own analysis:

```bash
AAM_TASK_ID=parser-refactor aam codex --root .
```

## 8. Model selection

By default, the **native CLI** selects its own model. You may configure optional model IDs using environment variables:

| Provider | Low complexity | Medium complexity | High complexity |
| --- | --- | --- | --- |
| Codex | `AAM_CODEX_LOW_MODEL` | `AAM_CODEX_MEDIUM_MODEL` | `AAM_CODEX_HIGH_MODEL` |
| Claude | `AAM_CLAUDE_LOW_MODEL` | `AAM_CLAUDE_MEDIUM_MODEL` | `AAM_CLAUDE_HIGH_MODEL` |

Example (replace placeholders with model IDs supported by your installed CLI):

```bash
AAM_CODEX_LOW_MODEL="YOUR_SMALL_MODEL" \
AAM_CODEX_HIGH_MODEL="YOUR_LARGE_MODEL" \
aam codex --root .
```

Routing is a **keyword heuristic**, not an AI classifier: security, architecture and migrations tend to be high; documentation/formatting tends to be low; otherwise medium. The model is pinned to a stored native session when that session is reused. Changing an environment variable does not necessarily change the model of an existing session; use `/new` or `--new` when a new session is appropriate.

## 9. Codex runtime profiles

Set `AAM_CODEX_PROFILE` **before** launching AAM:

| Value | Behavior |
| --- | --- |
| `coding` | **Current default.** Isolated persistent `CODEX_HOME` for coding tasks. |
| `minimal` | Isolated persistent `CODEX_HOME` without your normal global Codex configuration. |
| `full` | Uses your normal Codex environment and integrations. |
| `default` | Same environment-preserving behavior as `full`. |
| `auto` | Keyword-based choice: `full` for external integrations, `coding` for coding, otherwise `minimal`. |

Examples:

```bash
AAM_CODEX_PROFILE=full aam codex --root .
AAM_CODEX_PROFILE=auto aam codex --root .
```

The isolated profiles keep their state under `~/.local/state/ai-agent-manager/codex/` by default and may copy your local Codex authentication file into a protected profile directory. They are **not security sandboxes**. Switching profiles can affect native session availability, configuration and MCP access; use `full` if you need your normal integrations.

To change the profile-home location:

```bash
AAM_PROFILE_HOME=/path/to/private/aam-profiles AAM_CODEX_PROFILE=coding aam codex --root .
```

## 10. Configuration reference

| Variable | Default | Purpose |
| --- | --- | --- |
| `AAM_NO_CLEAR` | unset | Set to `1` to preserve terminal content on startup or `/new`. |
| `AAM_CODEX_PROFILE` | `coding` | Codex runtime profile. |
| `AAM_PROFILE_HOME` | `~/.local/state/ai-agent-manager/codex` | Isolated Codex profile directory. |
| `AAM_FORCE_NEW_SESSION` | unset | Set to `1` to bypass saved native session IDs for requests. |
| `AAM_CONTEXT_WARNING_TOKENS` | `100000` | Warn when last observed input reaches threshold; `0` disables warning. |
| `AAM_CHECKPOINT_EVERY_TURNS` | `8` | Auto-checkpoint interval; `0` disables automatic checkpoints. |
| `AAM_HANDOFF_HISTORY_CHARS` | `4000` | Maximum length of cross-agent delta excerpt. |
| `AAM_SUMMARY_CHARS` | `1200` | Maximum prompt/answer excerpt length in checkpoints. |
| `AAM_TASK_ID` | unset | Optional label written to new usage records. |
| `AAM_CODEX_*_MODEL`, `AAM_CLAUDE_*_MODEL` | unset | Optional low/medium/high model IDs. |

Example:

```bash
AAM_CHECKPOINT_EVERY_TURNS=4 \
AAM_CONTEXT_WARNING_TOKENS=80000 \
AAM_CODEX_PROFILE=full \
aam codex --root .
```

## 11. Testing and troubleshooting

Run all unit tests from the AAM checkout:

```bash
cd /path/to/AI-Agent-Manager
python3 -m unittest discover -s tests -v
```

Public CI checks Python syntax and unit tests. It does **not** run authenticated end-to-end requests against the actual provider CLIs. Verify native CLI compatibility on your machine.

**`aam: command not found`** — Ensure `~/.local/bin` is on your `PATH`; run `bash scripts/install.sh` again if needed.

**`Not a Git repository`** — Pass `--root` pointing to a Git checkout, or `cd` into one.

**Provider CLI not found** — Install and authenticate the requested `claude` or `codex` executable; check `command -v claude` and `command -v codex`.

**Missing integrations / unexpected Codex config** — Try `AAM_CODEX_PROFILE=full aam codex --root .`. The default `coding` profile uses isolated configuration.

**Unexpected session resume failure** — Native CLI versions may change session commands or storage. Inspect `/status`, then use `/new` only if discarding the bridge's session references is acceptable.

**Limit reached, but prompt not replayed** — This is intentional when AAM cannot prove the previous request was rejected before work. Inspect files and Git status before retrying.

**Large usage totals** — `/analytics` aggregates the existing ledger across previous AAM runs for that target repository, not only the current interactive session.

**Terminal not clearing** — Verify interactive TTY support and that `AAM_NO_CLEAR` is not set to `1`. Terminal escape sequences are not sent in one-shot/non-interactive mode.

**Interrupted request** — Check `git status`, inspect diffs and confirm any external actions before continuing.

## 12. Safety and limitations

- AAM does **not** automatically commit or push your target repository. A provider may still perform those operations if authorized through its own tools; review prompts and CLI permissions.
- Provider authentication, quotas, pricing, tools and permissions are managed by the respective CLIs.
- Handoffs and checkpoints are **bounded excerpts**, not complete conversation recovery.
- Repository fingerprints improve retry safety but cannot prove the absence of side effects outside Git (network requests, external services, databases, etc.).
- AAM is not a filesystem sandbox, credential vault, or multi-process transaction coordinator.
- Keep session metadata and usage logs private if your prompts may contain sensitive information.
- Claude/Codex CLI interfaces can change; run unit and real-world smoke tests after updates.

## Further documentation

- [Operations and validation](docs/OPERATIONS.md)
- [Codex profiles and benchmark](docs/PROFILES.md)
- [Session optimization](docs/SESSION_OPTIMIZATION.md)

## License

MIT.


## Optional modern terminal UI (MVP)

Install the optional renderer:

```bash
python3 -m pip install rich
aam codex --root . --ui rich
# or
aam claude --root . --ui rich
```

The existing `aam` commands, profiles, state and safe failover remain unchanged.
While Codex runs, the Rich UI highlights the active provider and model, shows recent
tool-event types, elapsed time and provider-reported token usage. On completion, the
temporary activity panel disappears and the final answer is rendered as Markdown,
with a token summary. Claude's JSON mode reports limited intermediate activity;
its usage appears when the turn finishes. One-shot and redirected sessions retain
plain output. The default `--ui classic` remains dependency-free.

This is the first presentation-layer MVP, not yet a full-screen Textual application.
It never exposes hidden reasoning or tool arguments, and cannot display token counts
that a provider has not supplied.


## Textual full-screen TUI (experimental)

```bash
python3 -m pip install textual
aam codex --root .
# or: aam claude --root .
```

Textual is now the default for interactive terminals. Explicit `--ui textual` also works. For the legacy terminal, use `--ui classic` or `--ui rich`. One-shot `--prompt` and redirected sessions default to classic. Install `textual` before launching the interactive default; if it is missing, AAM reports the install command instead of silently changing UI.\n\nThe full-screen UI includes a multiline composer (Ctrl+Enter to send), separate
Claude (terracotta) and Codex (teal) answer borders, live provider event labels,
reported token counters, collapsible steps (Ctrl+O), agent switching (Ctrl+T),
Git diff summary (Ctrl+D), and session commands. It retains the original Python
orchestration and safe retry policy. Ctrl+Q exits.

**Limitations:** This is an experimental interface. Native CLI approval prompts
are not reproduced; the CLI's configured permission mode still applies. The
provider's reported model may be unavailable when using its default selection.
Claude JSON output does not guarantee intermediate events; the app only displays
those actually emitted. Tool arguments and hidden reasoning are intentionally
not displayed. Test the app in a real terminal before relying on it for edits.
