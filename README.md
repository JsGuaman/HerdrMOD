# herdr


<p align="center">
  <img src="assets/logo.png" alt="herdr" width="100" />
</p>

<p align="center">
  <a href="https://herdr.dev">herdr.dev</a> · <a href="#install">install</a> · <a href="https://herdr.dev/docs/quick-start/">quick start</a> · <a href="https://herdr.dev/docs/">docs</a>
</p>

<p align="center">
  English · <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-666666?labelColor=333333" alt="Apache 2.0 license" /></a>
  <a href="https://github.com/herdrdev/herdr/releases"><img src="https://img.shields.io/github/downloads/herdrdev/herdr/total?labelColor=333333&color=666666" alt="total GitHub release downloads" /></a>
  <a href="https://github.com/herdrdev/herdr/stargazers"><img src="https://img.shields.io/github/stars/herdrdev/herdr?labelColor=333333&color=666666&logo=github" alt="GitHub stars" /></a>
  <a href="https://github.com/herdrdev/herdr/releases/latest"><img src="https://img.shields.io/github/v/release/herdrdev/herdr?label=release&labelColor=333333&color=666666" alt="latest stable release" /></a>
  <a href="https://formulae.brew.sh/formula/herdr"><img src="https://img.shields.io/homebrew/v/herdr?label=homebrew&labelColor=333333&color=666666" alt="Homebrew version" /></a>
  <a href="https://x.com/herdrdev"><img src="https://img.shields.io/badge/follow-%40herdrdev-000000?logo=x&logoColor=white" alt="follow @herdrdev on X" /></a>
</p>

---

## HerdrMOD (`JsGuaman/HerdrMOD`)

> **Notice of Modification (Apache License 2.0, Section 4(b)):**  
> This repository is a modified derivative work ("**HerdrMOD**") maintained by [`JsGuaman`](https://github.com/JsGuaman), based on [`herdrdev/herdr`](https://github.com/herdrdev/herdr) (licensed under the [Apache License 2.0](LICENSE)). It is an independent fork and is not an official distribution of `herdrdev/herdr`.

### What's New in HerdrMOD

1. **Right-Hand Directory Tree Panel (`23a0de78`)**
   - Displays a live file & folder tree synced to the active terminal pane's current working directory.
   - **Folder navigation:** single-click expands/collapses tree nodes (`▸`/`▾`), double-click enters subfolders, plus `..`, `[↑]` parent, `[↻]` refresh, `[◎]` snap back to active pane CWD, draggable width divider, scrollbar, collapse/expand (`»`/`«`), and keyboard controls (`↑`/`↓`/`←`/`→`/`Enter`/`Backspace`/`r`/`e`/`Esc`).
   - **Default app launch:** double-clicking any file opens it with the host OS default application (`ShellExecuteW` on Windows).
   - **File Explorer shortcut:** clicking `[Explorer]` opens Windows File Explorer directly in the currently viewed directory.
2. **Workspace Hibernation for Lower RAM Usage (`6a92dd7b`)**
   - Adds workspace hibernation/restoration across state, persistence snapshots, IPC/socket API, headless server, context menus, and sidebar UI so inactive workspaces release live PTY/terminal memory and can be restored on demand.
3. **Auto-Resume `agy` Sessions on Restore & Unhibernate (`eb40ebed`)**
   - Persists and restores `agy` (`antigravity_cli`) conversation/session state when restoring persisted sessions or waking hibernated workspaces, improving integration hooks and snapshot recovery.
4. **Windows Antigravity Hook Execution Fix (`3ac4379c`)**
   - Fixes PowerShell hook invocation (`herdr-agent-state.ps1`) and target registration for Antigravity on Windows.

<details>
<summary>Modified &amp; added files relative to upstream/master (Apache 2.0 Section 4(b) change log)</summary>

- **Added:** `src/app/file_tree.rs`, `src/ui/file_tree.rs`
- **Modified:** `.gitignore`, `README.md`, `build.rs`, `docs/versions/0.8.2/website/src/content/docs/socket-api.mdx`, `src/agent_resume.rs`, `src/api/schema/panes.rs`, `src/api/schema/tests.rs`, `src/api/server.rs`, `src/app/api/agents.rs`, `src/app/api/panes.rs`, `src/app/api_helpers.rs`, `src/app/input/mod.rs`, `src/app/input/modal.rs`, `src/app/input/mouse.rs`, `src/app/input/sidebar.rs`, `src/app/input/terminal.rs`, `src/app/mod.rs`, `src/app/session.rs`, `src/app/state.rs`, `src/integration/assets/antigravity_cli/herdr-agent-state.ps1`, `src/integration/assets/antigravity_cli/herdr-agent-state.sh`, `src/integration/mod.rs`, `src/integration/targets.rs`, `src/integration/tests.rs`, `src/ipc.rs`, `src/pane.rs`, `src/persist.rs`, `src/persist/io.rs`, `src/persist/restore.rs`, `src/persist/snapshot.rs`, `src/platform/fallback.rs`, `src/platform/linux.rs`, `src/platform/macos.rs`, `src/platform/windows.rs`, `src/server/alt_screen_read.rs`, `src/server/headless.rs`, `src/terminal/runtime.rs`, `src/ui.rs`, `src/ui/sidebar.rs`

</details>

---

https://github.com/user-attachments/assets/043ec09f-4bdd-41d5-aee0-8fda6b83e267

**the runtime your coding agents live on.**

- **always running** — herdr is a background server; the terminals live inside it. close the lid, drop the network, or restart the machine; agents keep working and sessions come back. reattach from any terminal, or over ssh.
- **never hunt for the stuck one** — every pane is marked working, blocked, or idle. when an agent stops and needs an answer, herdr says so.
- **agent-native** — agents drive herdr through the cli and socket api: they can spawn panes, prompt each other, and wait until another agent is genuinely blocked. [agent skill →](https://herdr.dev/docs/agent-skill/)
- **runs what you already run** — claude code, codex, cursor, opencode, grok and the rest. herdr doesn't wrap or replace them; it owns their terminals.
- **keyboard and mouse, both first-class** — tmux-style prefix keys *and* click, drag, split. pick per moment, not per tool.
- **plugins** — extend panes and workflows. [browse the marketplace →](https://herdr.dev/plugins/)
- **one rust binary, no electron** — runs in whatever terminal you already use.

---

## install

```bash
curl -fsSL https://herdr.dev/install.sh | sh
```

or `brew install herdr` · `mise use -g herdr` · windows: `powershell -ExecutionPolicy Bypass -c "irm https://herdr.dev/install.ps1 | iex"` · [endpoint-protected Windows](https://herdr.dev/docs/windows-beta/) · [binaries](https://github.com/herdrdev/herdr/releases)

then start it where the work lives:

```bash
herdr
```

run your agents, split panes, walk away. `ctrl+b q` detaches, `herdr` reattaches. [quick start →](https://herdr.dev/docs/quick-start/)

## docs

everything lives at [herdr.dev/docs](https://herdr.dev/docs/): [quick start](https://herdr.dev/docs/quick-start/) · [concepts](https://herdr.dev/docs/concepts/) · [supported agents](https://herdr.dev/docs/agents/) · [keyboard](https://herdr.dev/docs/keyboard/) · [configuration](https://herdr.dev/docs/configuration/) · [session state](https://herdr.dev/docs/session-state/) · [remote](https://herdr.dev/docs/persistence-remote/) · [integrations](https://herdr.dev/docs/integrations/) · [plugins](https://herdr.dev/docs/plugins/) · [socket api](https://herdr.dev/docs/socket-api/)

## thanks

every past sponsor and backer is listed in [SPONSORS.md](./SPONSORS.md) — thank you 🐑

enterprise / partnership: hey@herdr.dev

## agent instructions

if you are an ai agent helping with this repository, read [`AGENTS.md`](./AGENTS.md) before making changes and read [`CONTRIBUTING.md`](./CONTRIBUTING.md) before opening issues or PRs.

## development

```bash
git clone https://github.com/herdrdev/herdr
cd herdr
cargo build --release

just test        # unit tests
just check       # formatting, tests, and maintenance checks
```

## license

Herdr is licensed under the [Apache License 2.0](LICENSE).
