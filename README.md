# OpenUsage — Windows tray tracker for Claude Code

A Windows port of [robinebers/openusage](https://github.com/robinebers/openusage) (the macOS
SwiftUI menu-bar app), rebuilt in Rust + Tauri.

**Why not a straight port:** OpenUsage's UI is 13.5k lines of SwiftUI, and SwiftUI has no
Windows target. Its data layer (22k lines — providers, models, pricing) is portable, so that
is what is ported here; the tray popover is rebuilt.

## What it does

Reads Claude Code's own history — `projects/**/*.jsonl` under every Claude config root it can
find — sums tokens per model, and prices them locally.

**WSL-aware:** Claude Code installed inside Ubuntu keeps its history in
`\\wsl$\<distro>\home\<user>\.claude`, invisible to a native `%USERPROFILE%\.claude`. The
scanner runs `wsl.exe -l -q` and reads every distro's home dirs, so a Windows tray app picks
up WSL sessions with no configuration.

## Install

Download **`OpenUsage_0.1.0_x64-setup.exe`** from the [releases page](../../releases) and run it.
It installs to `%LOCALAPPDATA%\OpenUsage`, adds a Start-menu shortcut, and can run at login.

Portable users can take `openusage.exe` directly and run it from anywhere.

## Build

```sh
npm install
npx tauri build --target x86_64-pc-windows-gnu
```

On Linux this needs `llvm` (for `llvm-rc`), `mingw-w64`, and `nsis` — the installer builds fine
cross-compiled, just unsigned.

## Tests

```sh
cd src-tauri && cargo test
```

- `claude.rs` unit tests pin the pricing maths against a real line from `~/.claude/projects`
  (Claude Opus 5: in 2 / out 477 / cache_read 36 697 / cache_write 20 792 → $0.4807005).
- `tests/real_data.rs` scans **this host's actual** Claude history and asserts the numbers are
  non-zero, so a change to the jsonl schema fails the suite instead of silently showing an
  empty tray.

## Deviations from upstream, and why

- **Pricing is local, not downloaded.** Upstream fetches a pricing catalog; the rates here
  (opus 15/75, sonnet 3/15, haiku 1/5 per million; cache read 0.1x input, cache write 1.25x)
  are compiled in, so the app works offline. Unknown models price as sonnet rather than opus.
- **`<synthetic>` entries are dropped** — a placeholder for locally injected context, not a
  real model behind a real request.
- **Only the Claude Code provider.** The other ten need their own credential and API paths;
  the scanner is a module, so each is an added file.
- **No updater.** Upstream uses Sparkle; Windows builds would use WinGet or an in-app check.

## Providers

Claude Code is the only one that reports **token-level usage** (it reads
`projects/**/*.jsonl`). The other detected tools report **account state and balances**, which is
what they expose locally without a new credential flow:

| Tool | Read from | Reports |
|---|---|---|
| Claude Code | `~/.claude.json`, `projects/**/*.jsonl` | tokens + spend per model, org cards |
| OpenRouter | `~/.config/openrouter/key.json` | credits remaining (API) |
| Z.ai (GLM) | `~/.config/zai/key.json` | account balance (API) |
| GitHub Copilot | `~/.config/gh/hosts.yml` | authenticated + user |
| OpenAI Codex | `~/.codex/sessions` | session groups |
| Grok CLI | `~/.grok/auth.json` | account + plan |
| Devin | `~/.local/share/devin/credentials.toml` | authenticated |
| Pi agent | `~/.pi/agent/sessions` | session count |
| Cursor | `%APPDATA%\Cursor` | installed + state db |
| Ollama | `ollama list` | local model count (free) |

Each root also gets checked under `%APPDATA%` / `%LOCALAPPDATA%` where Windows puts it, and
inside every WSL distro's home — so a tool installed only in WSL is still found.

## Licence

Upstream is MIT (`robinebers/openusage`). This port keeps that licence; see `LICENSE` in the
upstream repository.
