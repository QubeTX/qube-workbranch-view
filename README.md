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

> **Status: early.** The terminal shell and packaging are in place; the live worktree
> intelligence lands phase by phase (see `docs/WB-300_HANDOFF_PLAN.md`). The first tagged
> release wires up the installers below.

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

### Windows

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/QubeTX/qube-workbranch-view/releases/latest/download/wb300-installer.ps1 | iex"
```

Windows also gets four first-class installers — Global / Corporate editions, each as MSI and
EXE — none of which require Rust on the install machine.

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
?         help                      1 – 6             jump to a tab
```

(The full, configurable keybinding set lands with the config subsystem.)

## Self-update

`wb300 update` updates in place via cargo or the prebuilt installers, including the
registry-aware Windows installer path. See `CLAUDE.md` for the maintenance contract.

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
