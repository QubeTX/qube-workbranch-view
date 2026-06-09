# WB-300

**Mission control for parallel Git worktrees.**

WB-300 (`wb300`) is a cross-platform Rust **TUI operator console** for supervising many
parallel coding-agent workspaces through Git worktrees. Run it inside a repository and it
opens a live dashboard correlating branches, worktrees, dirty/clean status, the OS processes
running inside each worktree (Claude, Codex, builds, tests…), changed-file collisions across
worktrees, remote drift, and safe-cleanup candidates.

It is **not** a general Git GUI. It is a *parallel-agent operations console* that happens to
use Git and worktrees as the safest observable layer:

```txt
Git is the source of truth.
Filesystem events are live hints.
Process scanning shows life signs.
Remote polling shows outside-world drift.
The archive is the black box recorder.
Ratatui is the cockpit.
```

> **Status: pre-1.0.** The live worktree intelligence — discovery, status, process mapping,
> the live engine, collisions, remote/pushed state, and safe cleanup — is in place, along with
> full cross-platform packaging and a registry-aware self-updater. The machine-wide home view
> and the headless `wb300 agent` JSON snapshot land next (see `docs/WB-300_HANDOFF_PLAN.md`).
> The installers below go live with the first tagged release.

## Features (in progress)

- **Branch + worktree explorer** — local branches, remote-tracking branches, and worktrees
  with detached / locked / prunable / stale state.
- **Process → worktree mapping** — which agent (Claude / Codex / …) or task is running inside
  which worktree, with CPU / memory / runtime.
- **Live updates** — created / modified / pushed / deleted / merged shown as they happen, with
  temporary color flashes.
- **Collision detection** — when multiple worktrees touch the same file or a high-risk path.
- **Readiness & cleanup** — what's ready to review, stale, risky, or safe to remove (with
  rescue snapshots before anything destructive).
- **Machine-wide home view** — run `wb300` outside a repo to see every repo being actively
  worked on across the machine, grouped by workbranch, with the same live flashes.

## Installation

WB-300 ships the same way as the rest of the QubeTX line. (Available from the first release.)

### macOS / Linux

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/QubeTX/qube-workbranch-view/releases/latest/download/wb300-installer.sh | sh
```

### Windows — PowerShell (quickest)

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/QubeTX/qube-workbranch-view/releases/latest/download/wb300-installer.ps1 | iex"
```

### Windows — first-class installers

Four signed-style installers, none of which need Rust on the install machine. **Pick one
format per edition** — installing both MSI and EXE of the same edition leaves two Add/Remove
Programs entries pointing at the same binary.

| Edition | Scope | Admin? | Installs to | Asset |
|---|---|---|---|---|
| **Global MSI** | perMachine | yes (UAC) | `C:\Program Files\wb300\bin` | `wb300-x86_64-pc-windows-msvc.msi` |
| **Corporate MSI** | perUser | no | `%LocalAppData%\Programs\wb300\bin` | `wb300-x86_64-pc-windows-msvc-corporate.msi` |
| **Global EXE** | perMachine | yes (UAC) | `C:\Program Files\wb300\bin` | `wb300-x86_64-pc-windows-msvc-setup.exe` |
| **Corporate EXE** | perUser | no | `%LocalAppData%\Programs\wb300\bin` | `wb300-x86_64-pc-windows-msvc-corporate-setup.exe` |

Each ships a `.sha256` sidecar. The **Corporate** editions need no admin rights — the right
pick on locked-down corporate workstations. All four add `wb300` to the appropriate PATH and
record how they were installed so `wb300 update` later fetches the matching installer.

### Cargo

```sh
cargo install wb300
```

### From source

```sh
git clone https://github.com/QubeTX/qube-workbranch-view.git
cd qube-workbranch-view
cargo build --release
```

## Usage

```sh
cd /path/to/git/repo
wb300                    # open the live TUI
wb300 --repo /path/to/repo
wb300 --no-live          # static snapshot mode
wb300 --no-alt-screen    # fallback renderer
wb300 update             # self-update to the latest release
```

## Keybindings

```txt
q / Esc   quit / close overlay      Tab / Shift+Tab   next / previous tab
?         help                      1 – 7             jump to a tab
j / k     move selection            r                 refresh snapshot
/         filter worktrees          f                 fetch from remotes (never automatic)
:         command palette           x                 remove selected worktree (type-to-confirm)
                                     p                 prune stale worktree metadata (confirm)
```

(The full, configurable keybinding set lands with the config subsystem.)

## Self-update

```sh
wb300 update          # update in place to the latest release
wb300 update --json   # machine-readable result (for orchestrating agents)
```

`wb300 update` checks the GitHub releases API and updates in place via `cargo install` or the
prebuilt installers. On Windows it is **registry-aware**: the four first-class installers each
record an install marker (`HKCU\Software\WB300\InstallSource`), and `update` downloads the
*matching* installer, verifies its SHA-256 against the published sidecar, runs it, and confirms
the new `--version` — so a perUser Corporate install upgrades without ever prompting for admin.
See `CLAUDE.md` for the maintenance contract (the installer ⇄ `src/update.rs` lockstep).

## Development

```sh
cargo run                 # run the TUI
cargo test                # unit + integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

## License

PolyForm Noncommercial 1.0.0 — see [LICENSE](LICENSE).
Copyright (c) 2026, Emmett S.
