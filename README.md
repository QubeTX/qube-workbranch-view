# WB-300

**Mission control for parallel coding agents.** One window, every repo, every branch, every agent — live.

You're running a swarm of coding agents — Claude here, Codex there, a couple of builds, a test
run. Git's rule is that a branch can be checked out in at most **one** worktree, so the real
shape of that work is a **branch tree**: `main`, a daily workbranch off it, and a task branch
per agent, each living in its own worktree. The work is the branches — and `git status` in
fifteen terminals is not a control tower.

**WB-300 (`wb300`) is the control tower.** It draws your actual branch hierarchy as a live
tree — which agent is on which branch, what files it's changing right now, what's uncommitted,
what's committed, what's pushed, what's merged, and where two branches are about to collide.
Run it inside a repo for that repo's tree; run it anywhere else and it zooms out to **every
active repository on the machine in one view**. And when something important happens — a
commit lands, work reaches the remote, two branches touch the same file — it can tap you on
the shoulder with a native OS notification.

```txt
▾ qube-workbranch-view              12 branches · 2 agents
├─ main                             ✓ pushed · clean
└─ emmett/wb-2026-06-10             ↑2 committed, not pushed
   ├─ feat/branch-tree-12           ● claude · ◆ editing
   │  ⌂ ~/git/qwv-feat-branch-tree
   │  ├─ src/ui/tree.rs             ~ modified
   │  └─ src/app/state.rs           ~ modified
   └─ fix/status-flash-9            ● codex · uncommitted
      ⌂ ~/git/qwv-fix-status-flash
      └─ src/live/mod.rs            ~ modified
```

It is **not** a Git GUI. It's an operations console built on what Git actually knows:

```txt
Git is the source of truth.        ← the tree is derived from commit topology, never guessed
Filesystem events are live hints.  ← a save lights up the file being written
Process scanning shows life signs. ← "● claude" right on the branch the agent is working
Remote polling shows the outside.  ← pushed? it flashes green
The archive is the black box.      ← commits, pushes, merges, conflicts — recorded
OS toasts are the tap on the shoulder. ← only for what you chose to hear about
Ratatui is the cockpit.            ← fast, flicker-free, runs anywhere a terminal does
```

## What it does

- **Shows the real hierarchy.** The tree is derived from commit topology (one batched
  `rev-list`, cached): trunk, workbranches, task branches, with naming conventions used only to
  break genuine ties. Each branch row carries its lifecycle stage — *editing → uncommitted →
  committed → pushed → merged* — its agent, its ⌂ worktree path, and expands into the exact
  files being changed (`~` modified, `+` added, `-` deleted). Repos that don't follow any
  convention degrade gracefully to a flat list. By default you see the branches that matter
  *now* (a worktree, an agent, or unmerged work); `a` reveals everything else, dimmed.
- **Sees every agent — the real ones.** It scans the OS process table, maps each process to its
  worktree, and labels it — Claude/Codex agents, builds, tests, shells, editors — with CPU,
  memory, and uptime. It tells a real coding-agent CLI apart from the Claude *desktop* app.
  Found a forgotten or stuck agent? `K` ends it, behind a type-the-PID confirm (wb300 never
  kills anything on its own).
- **Updates itself, live.** Save a file and a blue ◆ pulses on that file's row and its branch;
  a branch holds *yellow* the whole time it's uncommitted; commit and the row flashes *magenta*,
  push and it flashes *green*, then it settles. Rebases don't masquerade as commits. The header
  strip keeps the totals and data freshness always visible — and says **stale** plainly if it
  ever can't read Git.
- **Taps you on the shoulder.** Native OS notifications for exactly three things: a branch got
  new commits, a branch's work reached the remote, and two branches started changing the same
  file. Never for anything else. Bursts coalesce ("3 branches pushed"), repeats are suppressed,
  and `--no-notify` or a small config file turns any of it off.
- **Forecasts the merge hell.** The **Merge Risk** view flags every file changed on two or more
  branches, names the agent on each side, and color-codes by how scary the path is (lockfiles,
  migrations, and CI configs are the scariest). New risks are recorded — and toasted.
- **Cleans up — safely.** `x` removes the selected branch's *worktree* (the branch and its
  commits are kept) behind a type-the-name confirm. Dirty work gets a rescue patch saved
  *first*, and if that rescue can't be written, the delete is **aborted**.
- **Zooms out.** Outside a repo (or with `--home`) the same tree shows every active repository
  as a root node — your single window over a fleet of agents across many projects. `Enter`
  drills into a repo; `q` comes back.
- **Talks to other agents.** `wb300 agent` prints the whole picture as JSON (schema
  `wb300.agent.v2`): the branch hierarchy with roles, parents, lifecycle stages, agents, and
  changed files — so an orchestrating Claude/Codex/script can read the state in one shot.
- **Documents itself.** `wb300 help` prints the full manual — every view, key, glyph, and
  concept — in the terminal. And `wb300 uninstall` removes it cleanly, whatever way it was
  installed.

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
- **The hierarchy is cheap.** Branch parentage comes from one batched, fingerprint-cached
  `rev-list` over the off-trunk history — a capture costs *fewer* git subprocesses than v1 did,
  and a poll where nothing changed costs no extra ones at all.
- **`sysinfo`** for the cross-platform process scan, **`notify`** for filesystem events (pruned,
  hard-capped watches), and **`notify-rust`** for OS toasts (pure-Rust zbus on Linux; on Windows
  it self-registers a per-user AppUserModelID so toasts say "WB-300").
- **One rule runs the whole thing:** `KeyEvent → Action → reducer → async task → snapshot →
  render`. The UI only ever *reads*; a single reducer is the only place state changes; the Git
  snapshot is canonical and everything on screen is derived from it. It ships with a real unit +
  integration suite that runs Git against throwaway repos (including a bare "origin").

And it's **safe by default**: it never fetches, pushes, rebases, resets, or kills anything on its
own — every mutation is an explicit, confirmed keystroke (`f` fetch, `x` remove a worktree, `K`
kill an agent), and it won't terminate its own process. The only network call it makes is one you
ask for (`f` to fetch) or `wb300 update`.

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
record how they were installed so `wb300 update` (and `wb300 uninstall`) later act through the
matching channel.

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
wb300                    # open the live TUI (the branch tree)
wb300 --repo /path/to/repo
wb300 --home             # machine-wide view of every actively-worked-on repo
wb300 --no-live          # static snapshot mode
wb300 --no-notify        # no OS notifications this run
wb300 --no-alt-screen    # fallback renderer
wb300 help               # the full manual, in the terminal
wb300 agent              # headless JSON snapshot (no TUI) — for other agents
wb300 agent --home       # headless JSON for the whole machine
wb300 update             # self-update to the latest release
wb300 uninstall          # remove wb300 (--purge also removes state/config)
```

## Notifications

WB-300 sends native OS notifications for exactly three events — a branch got new commits, a
branch's work reached its remote, and a new merge-conflict risk — and never for anything else.
Bursts coalesce into one toast and repeats are suppressed per branch. Configure (or disable)
them in `~/.config/wb300/config.toml` (Windows: `%LOCALAPPDATA%\wb300\config.toml`):

```toml
[notifications]
enabled = true
commit = true
push = true
conflict_risk = true
cooldown_secs = 30
```

`--no-notify` forces them off for a single run. On Windows, toasts identify as **WB-300** via a
per-user registry entry registered at startup (no admin needed); if that registration fails they
fall back to displaying as "Windows PowerShell".

## Headless JSON for orchestrating agents

`wb300 agent` prints the full repository state as JSON and exits — no TUI — so another agent
(Claude, Codex, a script) can get an instant, structured view of the branch hierarchy, the
agents on it, and the files in flight:

```sh
wb300 agent | jq '.repo.branches[] | {name, role, parent, lifecycle, agent: .agent.name}'
```

The output carries a stable `"schema": "wb300.agent.v2"` tag and a `"mode"` of `"repo"` (the
current repository) or `"home"` (every active repository, machine-wide — used automatically
outside a repo, or with `--home`). Each repo's `branches` array is the hierarchy in depth-first
order with `parent` pointers, plus the path-level `worktrees` and `collisions` views. stdout is
**pure JSON**: diagnostics and errors go to stderr, so the stream is always safe to parse.

## Keybindings

```txt
q / Esc   quit / back out            Tab / 1 – 6      switch tab
?         help overlay               j / k            move selection
l / h     expand / collapse          Enter            toggle node (home: open repo)
a         active-only ⇄ all branches r                refresh snapshot
/         filter branches            f                fetch from remotes (never automatic)
:         command palette            x                remove selected branch's worktree (confirm)
K         kill agent / process       p                prune stale worktree metadata (confirm)
```

`wb300 help` explains every view, stage, and glyph in detail.

## Self-update & uninstall

```sh
wb300 update            # update in place to the latest release
wb300 update --json     # machine-readable result (for orchestrating agents)
wb300 uninstall         # remove wb300 via the channel that installed it
wb300 uninstall --purge # … and remove state, config, and registry entries too
```

`wb300 update` checks the GitHub releases API and updates in place via `cargo install` or the
prebuilt installers. On Windows it is **registry-aware**: the four first-class installers each
record an install marker (`HKCU\Software\WB300\InstallSource`), and `update` downloads the
*matching* installer, verifies its SHA-256 against the published sidecar, runs it, and confirms
the new `--version` — so a perUser Corporate install upgrades without ever prompting for admin.
`wb300 uninstall` uses the same detection to run the matching uninstaller (or `cargo
uninstall`), and never touches your repositories. See `CLAUDE.md` for the maintenance contract
(the installer ⇄ `src/update.rs` lockstep).

## Development

```sh
cargo run                 # run the TUI
cargo test                # unit + integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Architecture notes live in `CLAUDE.md` and `docs/` (the v2 design is
`docs/WB-300_V2_DESIGN.md`). Part of the **QubeTX** line (TR-300, ND-300, SD-300); it shares
their license, packaging, and tag-triggered release cycle.

## License

PolyForm Noncommercial 1.0.0 — see [LICENSE](LICENSE).
Copyright (c) 2026, Emmett S.
