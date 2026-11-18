# AI-Agent-Manager v1.0.0

Release date: 2026-11-18

## Highlights

- Claude Code and OpenAI Codex CLI orchestration with separate native sessions.
- Automatic Claude → Codex fallback on verified pre-execution provider limits.
- Conservative replay safety: requests are not automatically repeated if side effects cannot be excluded.
- Cross-provider handoff with bounded conversation excerpts and actual Git repository facts.
- Textual terminal interface with distinct provider styling, Enter-to-send, Ctrl+N newline, activity steps, answer previews and slash-command suggestions.
- Esc cancellation, recent session history, checkpoint metadata, Git diff summaries and token usage inspection.
- Explicit upstream update check via `/updates`.
- Classic CLI and optional Rich presentation remain available.

## Installation

```bash
git clone https://github.com/Vprogrammers/AI-Agent-Manager.git
cd AI-Agent-Manager
bash scripts/install.sh
python3 -m unittest discover -s tests -v
```

Install the optional Textual dependency if needed, then start from a target Git repository:

```bash
aam claude --root .
# or
aam codex --root .
```

## Validation and limitations

The user reported successful local regression and manual tests, including provider switching, context handoff, cancellation, history, token reporting and Git inspection. No independent CI run is asserted by this document.

- Provider output and native CLI versions determine which model, token and streaming metadata is available.
- Context handoff does not transfer private reasoning or the complete native session of the other provider.
- Cancellation cannot undo filesystem or external side effects already performed.
- `/updates` checks the configured Git upstream but does not install updates.
- AAM is not a sandbox. Review provider permissions and repository changes.

## Release procedure

Create the annotated Git tag from the verified release commit and publish these notes as a GitHub Release. Do not move an existing tag or publish until the tag and target commit are verified.
