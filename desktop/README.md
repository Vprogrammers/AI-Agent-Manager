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

## Workspace picker and safe provider continuation

Use **Open Folder…** to select a Git repository via the native OS dialog. Recently selected paths are saved in the webview's local storage on this machine (not synchronized).

When Claude reports a limit but the CLI cannot verify that no work was performed, AAM intentionally blocks automatic replay. The desktop UI displays a **Switch to Codex for next request** action instead. Review the working tree first, then send a new continuation prompt; the failed request is not automatically resent.

## Open from GitHub (development)

Install and authenticate [GitHub CLI](https://cli.github.com/): `gh auth login` and verify `gh auth status`. Click **Open from GitHub…** to list up to 100 repositories available to the authenticated account, filter them, then choose **Clone** and a local parent directory. The new local clone opens as the current workspace. Private repositories require corresponding GitHub permissions. Existing destination folders are never overwritten; use **Open Folder…** to select an existing clone. GitHub CLI is invoked locally; no tokens are stored by AAM Desktop.

## macOS desktop packaging (development distribution)

From the repository root, install the Python launcher once:

```bash
bash scripts/install.sh
cd desktop
npm ci
npm run icons
npm run tauri build
```

The macOS bundles are generated under `desktop/src-tauri/target/release/bundle/`.
Install the `.dmg` or copy the `.app` to Applications and launch from Finder/Dock.
The GUI now searches `~/.local/bin/aam` before PATH because Finder does not reliably inherit shell PATH.
**This is not a self-contained installer**: the installed app still requires Python 3, Git, a working AAM launcher pointing at an accessible repository checkout, and an authenticated Claude or Codex CLI.
GitHub clone also requires `gh`. Distribution to another Mac needs a bundled engine and dependency discovery; public distribution also needs Apple code signing and notarization.
Windows and Linux packaging are not yet enabled or verified. Do not ship this as a cross-platform release.

## Self-contained AAM engine (packaged desktop)

Release builds package the Python AAM orchestration engine as a PyInstaller one-file sidecar. They **do not require** a separately installed AAM CLI or Python runtime. Build on each target OS and architecture (cross-compiling a PyInstaller executable is not supported).

```bash
python3 -m pip install pyinstaller
cd desktop
npm ci
npm run icons
npm run tauri build
```

The Tauri build hook runs `npm run bundle:engine` to generate `src-tauri/binaries/aam-engine-<rust-target-triple>` and embeds it in the installer. Installed apps look for the bundled engine beside their executable; development mode retains the legacy `~/.local/bin/aam` fallback.

**External requirements remain:** Git, Claude Code and/or Codex CLI, and authentication for the chosen agent. GitHub cloning additionally requires `gh`. GUI-launched macOS apps may not inherit shell PATH for these external tools; dependency discovery and setup UI remain to be implemented. The application is not yet a verified one-click experience on a fresh machine.

macOS public distribution requires signing and notarization. Windows/Linux builds require platform CI, signing where applicable, and verification. Do not publish unsigned installers as trusted releases.
