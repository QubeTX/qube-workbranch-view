# WB-300

**Mission control for parallel Git worktrees.** Watch every agent workspace live.

You're running a swarm of coding agents — Claude here, Codex there, a couple of builds, a
test run — each on its own branch in its own Git worktree. They're all editing at once, and
you have no Git-native, process-aware way to see what's actually alive, what's drifting, and
where two of them are about to clobber the same file. `git status` in fifteen terminals is not
a control tower.

**WB-300 (`wb300`) is the control tower.** Point it at a repo and it opens a live terminal
dashboard that correlates your branches, your worktrees, and the **OS processes running inside
each one** — so you can see, at a glance, which agent is working where, what's dirty, what just
got pushed, what's about to collide, and what's safe to clean up. Run it *outside* a repo and
it zooms out to a machine-wide view of every project being worked on across your whole machine.

It is **not** a Git GUI. It's an operations console that happens to use Git and worktrees as
the safest thing to observe:

```txt
Git is the source of truth.        ← never guessed, always asked
Filesystem events are live hints.  ← a save lights up instantly
Process scanning shows life signs. ← "● claude pid 109220" right on the worktree
Remote polling shows the outside.  ← pushed? it flashes green
The archive is the black box.      ← created/removed worktrees, recorded
Ratatui is the cockpit.            ← fast, flicker-free, runs anywhere a terminal does
```

## What it does

- **Sees every agent.** It scans the OS process table, figures out which worktree each process
  is living in, and labels it — Claude/Codex agents, builds, test runs, shells, editors — with
  CPU, memory, and uptime. A green `● claude pid 1234` sits right on the worktree it's in.
- **Updates itself, live.** Save a file and a blue mark pulses on that worktree while it's being
  written; its name stays *yellow* the whole time it has uncommitted work; commit and the whole
  row flashes *magenta*, push and it flashes *green* — then it settles back. Another terminal
  creates or deletes a worktree and it notices and records it. No refresh key required (though
  `r` is there if you want it). If it ever can't read Git, the header says **stale** rather than
  leaving old data looking live.
- **Warns you before the merge hell.** It compares what every worktree has touched — including
  commits made since the shared base — and flags any file two or more worktrees have changed,
  color-coded by how scary the path is (lockfiles, migrations, and CI configs are the scariest).
- **Cleans up — safely.** It scores each worktree (safe / caution / dirty / active / protected)
  and lets you remove one behind a **type-the-name-to-confirm** dialog. Dirty work gets a rescue
  patch saved *first*, and if that rescue can't be written, the delete is **aborted**. It will
  not eat your uncommitted work.
- **Zooms out.** Run it outside any repo (or `wb300 --home`) for a machine-wide tower of every
  repo with a live agent or parallel worktrees, grouped by workbranch, with the same live
  flashes — your single window over a fleet of agents across many projects.
- **Talks to other agents.** `wb300 agent` prints the whole picture as JSON (no TUI) so an
  orchestrating Claude/Codex/script can read the worktree/branch/agent/collision state in one
  shot.

## Under the hood

WB-300 is a single, dependency-light Rust binary (edition 2024). A few deliberate choices make
it fast and safe to leave running on a busy machine:

- **Ratatui + Crossterm** for an immediate-mode TUI that's flicker-free and works in Windows
  Terminal, iTerm, tmux, and plain SSH alike — with **unconditional terminal restoration** (an
  RAII guard plus a panic hook) so a crash or `Ctrl-C` never leaves your shell wedged in raw mode.
- **Tokio** drives everything off the UI thread. The render loop is a single `select!` over
  keyboard input, a filesystem watcher, a periodic poll, and completed Git captures — so **the
  UI never blocks on Git**, even when a repo has dozens of worktrees.
- **It drives the real `git` CLI** (via async subprocesses) instead of linking a Git library —
  your config, credential helpers, SSH setup, and Git-for-Windows quirks all apply unchanged.
  Parsing is NUL-safe (`-z` / porcelain v2) everywhere, so paths with spaces and newlines are fine.
- **`sysinfo`** for the cross-platform process scan and **`notify`** for filesystem events —
  the watcher only watches your source files (never `node_modules`/`target`/`.git`) and is
  hard-capped, so it stays a good citizen of your OS's file-watch budget.
- **One rule runs the whole thing:** `KeyEvent → Action → reducer → async task → snapshot →
  render`. The UI only ever *reads*; a single reducer is the only place state changes; the Git
  snapshot is canonical and everything on screen is derived from it. Easy to reason about, easy
  to test — it ships with a real unit + integration suite that runs Git against throwaway repos.

And it's **safe by default**: it never fetches, pushes, rebases, resets, or kills a process on
its own. The only network call it makes is one you ask for (`f` to fetch) or `wb300 update`.

## Installation

### macOS / Linux

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/QubeTX/qube-workbranch-view/releases/latest/download/wb300-installer.sh | sh
```

### Windows — PowerShell (quickest)

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/QubeTX/qube-workbranch-view/releases/latest/download/wb300-installer.ps1 | iex"
```

### Windows — first-class installers

Four installers, none of which need Rust on the install machine. **Pick one format per
edition** — installing both the MSI and the EXE of the same edition leaves two Add/Remove
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
wb300 --home             # machine-wide view of every actively-worked-on repo
wb300 --no-live          # static snapshot mode
wb300 --no-alt-screen    # fallback renderer
wb300 agent              # headless JSON snapshot (no TUI) — for other agents
wb300 agent --home       # headless JSON for the whole machine
wb300 update             # self-update to the latest release
```

## Headless JSON for orchestrating agents

`wb300 agent` prints the full repository state as JSON and exits — no TUI — so another agent
(Claude, Codex, a script) can get an instant, structured view of the worktree / branch /
workbranch / running-agent / collision picture:

```sh
wb300 agent | jq '.repo.worktrees[] | {branch, workbranch, agent: .agent.name}'
```

The output carries a stable `"schema": "wb300.agent.v1"` tag and a `"mode"` of `"repo"` (the
current repository) or `"home"` (every active repository, machine-wide — used automatically
outside a repo, or with `--home`). stdout is **pure JSON**: diagnostics and errors go to
stderr, so the stream is always safe to parse.

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

Architecture notes live in `CLAUDE.md` and `docs/`. Part of the **QubeTX** line (TR-300,
ND-300, SD-300); it shares their license, packaging, and tag-triggered release cycle.

## License

PolyForm Noncommercial 1.0.0 — see [LICENSE](LICENSE).
Copyright (c) 2026, Emmett S.
