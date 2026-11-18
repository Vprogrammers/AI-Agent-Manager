# AI Agent Manager Desktop (experimental)

Tauri 2 + React + TypeScript desktop foundation, reusing the existing `aam` Python CLI. **Not a v1 desktop release.**

## Run locally

Install Node.js, Rust, Tauri system prerequisites, Python 3.10+, Git, the `aam` launcher, and at least one authenticated provider CLI.

```bash
cd desktop
npm install
npm run tauri dev
```

Enter an absolute path to a Git repository, select Claude or Codex, and send a prompt. Requests use `aam <provider> --root <path> --ui classic --prompt <text>` and therefore reuse the existing bridge sessions and failover rules.

## Limitations / next milestones

- Currently one-shot request/response: **no live event streaming or cancellation** in the desktop UI.
- `aam` must be available in the desktop process PATH; bundling the Python engine and CLI discovery are not implemented.
- No native folder picker, workspace persistence, token panel, diff panel, or packaging yet.
- Commands execute with the permissions of the current OS user. Never open untrusted repositories without reviewing provider permissions.
- Windows support requires validation of the Python engine and subprocess/process-group handling before shipping.
- This is a development scaffold. Tauri bundling is intentionally disabled until distribution and sidecar strategy are implemented.
