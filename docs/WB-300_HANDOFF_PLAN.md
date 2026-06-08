# WB-300 Handoff Plan

**Executable:** `wb300`  
**Working name:** WB-300  
**Document purpose:** handoff plan for Claude Code or another coding agent to continue product planning and implementation.  
**Status:** planning/design handoff.  
**Last updated:** 2026-06-08.

---

## 1. What WB-300 is

WB-300 is a **cross-platform terminal user interface (TUI) operator console for managing parallel coding-agent work through Git worktrees, branches, local filesystem activity, process state, and remote-tracking state**.

The intended flow is:

```bash
cd /path/to/git/repo
wb300
```

WB-300 opens a live TUI dashboard that shows:

- local branches
- remote-tracking branches
- Git worktrees
- which worktrees are dirty, clean, staged, untracked, ahead, behind, detached, locked, stale, prunable, or deleted
- which local OS processes are running inside which worktrees
- which worktrees were recently modified
- which worktrees were created or deleted while the dashboard was open
- which branches appear to have been pushed
- which files are being touched by multiple worktrees at once
- which worktrees are ready to review, stale, risky, active, or safe to clean up

The core concept:

```txt
Git is the source of truth.
Filesystem events are live hints.
Process scanning shows life signs.
Remote polling shows outside-world drift.
The archive is the black box recorder.
Ratatui is the cockpit.
```

WB-300 is **not** primarily a Git GUI. It is not trying to become SourceTree, GitKraken, lazygit, or GitHub Desktop in the terminal.

It is a **parallel-agent operations console** that happens to use Git and worktrees as the safest observable layer.

---

## 2. Why we are building it

The user/operator often runs many coding agents in parallel on the same project. Those agents use separate branches and Git worktrees. From the operator’s point of view, the problem is not merely:

```txt
What branches exist?
```

The real questions are:

```txt
What work is currently alive?
What changed recently?
Which agents are still running?
Which worktrees are dirty?
Which branches are ahead, behind, pushed, deleted, stale, or abandoned?
Which agents are touching the same files?
Which worktrees are ready for review?
Which worktrees are safe to remove?
Where is merge hell being manufactured right now?
```

Current Git tools are good at showing repository state, but they usually do not answer the operator questions around **many concurrent agent workspaces**.

WB-300 should make parallel coding-agent work observable without needing to introspect the agents themselves. It should stay Git-centered and process-centered.

The product promise:

> Show me every parallel Git workspace, what is happening inside it, and what is risky.

The strongest features are:

1. **Process-to-worktree mapping** — show which agents/tasks are actually running inside each worktree.
2. **Changed-file collision detection** — show when multiple worktrees are touching the same files or high-risk paths.
3. **Live updates** — show creations, saves, pushes, deletions, and process changes as they happen.
4. **Readiness/risk scoring** — show what needs review, cleanup, or operator attention.
5. **Archive/timeline** — preserve a local event history of created, modified, pushed, deleted, and cleaned-up worktrees.

---

## 3. Product identity

### Name

WB-300

### CLI command

```bash
wb300
```

### Short description

```txt
WB-300 is a live TUI control tower for Git worktrees used by parallel coding agents.
```

### Longer description

```txt
WB-300 helps an operator supervise many parallel coding-agent workspaces by correlating Git branches, worktrees, file changes, process activity, remote state, collisions, and cleanup status in one live terminal dashboard.
```

### Suggested tagline

```txt
Mission control for parallel Git worktrees.
```

Alternative taglines:

```txt
A live cockpit for agent-driven Git worktrees.
A terminal operations console for parallel coding agents.
Git-native visibility for agent swarms.
```

---

## 4. Conversation recap and decisions

### 4.1 Initial research request

The conversation began with research into how Claude Code achieves a smooth, polished terminal UI, including keyboard shortcuts, fluid output, fullscreen rendering, and animations.

Important takeaways:

- Claude Code should not be treated as confirmed Rust source architecture.
- Current Claude Code distribution uses native per-platform binaries.
- Historically/publicly discussed Claude Code UI architecture appears closer to TypeScript/React/Ink-style terminal rendering or a custom fullscreen terminal renderer.
- Claude Code’s smoothness seems to come less from one magic library and more from terminal-rendering discipline:
  - raw keyboard input
  - alternate-screen fullscreen rendering
  - fixed bottom input box
  - diffed redraws
  - virtualized visible content
  - capped render cadence
  - mouse support where available
  - terminal capability fallbacks
  - context-aware keybinding system

For a new app, TypeScript/OpenTUI/Ink was considered viable for chat/agent-style tools, but not chosen for WB-300.

### 4.2 Stack decision

For this specific app, the recommendation is:

```txt
Rust + Ratatui + Crossterm + Tokio + notify + sysinfo + Git CLI subprocesses
```

Reasoning:

- WB-300 is Git/process/filesystem-heavy.
- It needs fast startup, native binaries, robust path/process handling, and cross-platform terminal support.
- Rust is a better match for filesystem/process/Git orchestration than TypeScript.
- Ratatui is the modern Rust TUI default.
- Crossterm is the practical cross-platform terminal backend for raw mode, keyboard, mouse, resize, alternate screen, etc.
- Tokio allows Git subprocesses, filesystem events, process scans, remote polls, and UI input to run without blocking the render loop.
- The installed `git` CLI should be used in v1 instead of `git2` or `gix`, because it respects the user’s actual Git configuration, credential helpers, SSH setup, corporate config, Git for Windows behavior, and remote auth.

### 4.3 Main product evolution

The idea evolved through these stages:

1. A branch/worktree explorer in a tree-style terminal UI.
2. A Git-native visibility tool for multiple local and remote branches/worktrees.
3. An operator console for supervising many parallel coding agents.
4. A live dashboard that reacts when worktrees are created, modified, pushed, deleted, or archived.

The final product direction:

```txt
WB-300 should be a live, Git-native operator console for parallel agent work.
```

---

## 5. Non-goals

WB-300 should avoid becoming too broad too early.

### Not a general Git GUI

It should not try to clone all features of SourceTree, GitKraken, GitHub Desktop, or lazygit.

### Not an agent introspection platform

WB-300 should not require reading Claude/Codex/internal agent state. It should observe:

- Git state
- filesystem changes
- process table
- optional stdout/stderr logs only for processes launched through WB-300
- optional GitHub/GitLab/PR state later

### Not a hidden automation daemon by default

The first version should run while the TUI is open. A background daemon can be considered later, but should not be assumed.

### Not a destructive automation tool by default

Destructive actions must be explicit, confirmed, and safe by default:

- remove worktree
- delete branch
- kill process
- force kill process tree
- prune
- reset
- stash
- clean untracked files

### Not a remote hammer

WB-300 should not run `git fetch --all --prune` every few seconds by default. Remote operations should be explicit or conservatively polled.

---

## 6. Core principles

1. **Git is the source of truth.**
   Filesystem events should trigger refreshes, but Git snapshots decide state.

2. **Live but not reckless.**
   Local events can update quickly. Remote state should be clearly labeled as last checked/fetched.

3. **Never block input on Git.**
   Git commands must run asynchronously. The TUI should remain responsive even during fetch/status scans.

4. **Use NUL-safe Git parsing whenever possible.**
   Paths can contain weird characters. Use `-z` and NUL-delimited formats.

5. **Keep UI rendering state separate from repository state.**
   `RepoSnapshot` is truth. `TreeNode`/UI state is derived.

6. **Prefer observation over magic.**
   Show the operator what is happening. Avoid taking automatic destructive actions.

7. **Explain uncertainty.**
   Some process info may be unavailable due to OS permissions. Some filesystem events may be missed. Remote state may be stale.

8. **Cross-platform first.**
   macOS, Linux, Windows, ARM, and x64 should be planned from the start.

9. **Terminal compatibility matters.**
   Support alternate screen and mouse, but include fallbacks and safe terminal restoration.

10. **The operator’s attention is the scarce resource.**
    Sort by risk, recency, activity, readiness, and cleanup safety, not just alphabetically.

---

## 7. Recommended technology stack

### 7.1 Language/runtime

```txt
Rust
```

Reasons:

- native binaries
- fast startup
- strong path/process/filesystem support
- solid async ecosystem
- strong CLI packaging story
- good fit for Git subprocess orchestration
- better long-term fit for an operations tool than a JS runtime wrapper

### 7.2 TUI

```txt
Ratatui
```

Ratatui is the modern Rust immediate-mode TUI framework. It works well for dashboards, trees, panels, status bars, modals, and responsive terminal layouts.

### 7.3 Terminal backend

```txt
Crossterm
```

Use Crossterm for:

- raw mode
- alternate screen
- keyboard events
- mouse events
- terminal resize events
- cursor control
- terminal restoration
- Windows/Unix support

Important Windows/cross-platform input rule:

```rust
if key.kind != KeyEventKind::Press {
    return;
}
```

Windows terminals may emit press/release/repeat distinctions. Filter to `Press` unless a feature explicitly needs repeat/release.

### 7.4 Async runtime

```txt
Tokio
```

Use Tokio for:

- async subprocesses
- event bus
- Git task scheduling
- process scan ticks
- filesystem debounce timers
- remote polling
- render tick/animation scheduling

### 7.5 Filesystem watching

```txt
notify
```

Use `notify::recommended_watcher()` for the platform-specific watcher.

Important caveat: filesystem watchers are not perfect. Network filesystems, editors that save by rename, Linux watch limits, Docker/WSL/macOS edge cases, and very large directory trees can all behave differently. Treat watcher events as hints and use Git snapshots as truth.

### 7.6 Process scanning

```txt
sysinfo
```

Use it to scan local processes and map process current working directories to Git worktrees.

Need to design this as best-effort:

- process CWD may be unavailable
- process command/env may be restricted
- permissions vary by OS
- Windows path normalization matters
- processes may exit between scan and display

### 7.7 Git access

Use installed `git` binary via subprocesses for v1.

Do not use `git2` or `gix` in v1 unless a specific subprocess bottleneck becomes painful.

Why Git CLI first:

- respects user config
- respects credential helpers
- respects SSH setup
- handles Git for Windows quirks
- supports corporate Git setups
- avoids early complexity from libgit2/gix APIs
- makes behavior match the command line users already trust

Potential later additions:

```txt
gix       pure Rust Git internals, if needed later
git2      libgit2 binding, possible but not first choice
```

### 7.8 Packaging/release

Recommended:

```txt
cargo-dist
```

Use for GitHub releases and prebuilt binaries/installers.

Target platforms to plan:

```txt
x86_64-apple-darwin
aarch64-apple-darwin
x86_64-unknown-linux-gnu
aarch64-unknown-linux-gnu
x86_64-unknown-linux-musl, optional
x86_64-pc-windows-msvc
aarch64-pc-windows-msvc, optional/when stable enough for release workflow
```

### 7.9 Supporting crates

Initial dependency direction:

```toml
[dependencies]
ratatui = "0.30"
crossterm = { version = "0.29", features = ["event-stream"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "process", "sync", "time"] }
futures-util = "0.3"

clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
directories = "5"

notify = "8"
sysinfo = "0.36"

color-eyre = "0.6"
tracing = "0.1"
tracing-subscriber = "0.3"

indexmap = "2"
unicode-width = "0.2"
time = { version = "0.3", features = ["serde", "formatting"] }
```

Optional later:

```toml
tui-tree-widget = "0.24"
ratatui-textarea = "0.4"
portable-pty = "0.9"
rusqlite = { version = "0.32", features = ["bundled"] }
redb = "2"
ignore = "0.4"
```

Version numbers should be checked/updated when implementation starts.

---

## 8. Runtime architecture

### 8.1 High-level architecture

```txt
main.rs
  ├─ parse CLI args
  ├─ discover repo
  ├─ load config
  ├─ initialize terminal
  ├─ initialize app state
  ├─ start LiveEngine
  └─ run TUI event loop

LiveEngine
  ├─ FsWatcher
  ├─ GitSnapshotter
  ├─ ProcessScanner
  ├─ RemotePoller
  ├─ SnapshotDiffer
  ├─ EventArchive
  └─ UiTransitionManager

TUI
  ├─ EventLoop
  ├─ KeybindingResolver
  ├─ Action reducer
  ├─ Render scheduler
  └─ Ratatui views/widgets
```

### 8.2 Event flow

```txt
Terminal key/mouse/resize event
Filesystem watcher event
Process scan tick
Local Git snapshot tick
Remote poll/fetch tick
Async Git task result
Animation tick
        ↓
Internal event bus
        ↓
Action/event normalization
        ↓
Snapshot diffing / reducer
        ↓
AppState update
        ↓
UI transition update
        ↓
Render if dirty or animation active
```

### 8.3 Key rule

Do not directly run Git commands from UI widgets.

Use:

```txt
KeyEvent
  → KeybindingResolver
  → Action
  → AppState reducer
  → async Git/Process/FS task
  → Snapshot/result event
  → AppState update
  → render
```

Avoid:

```txt
Widget sees keypress and shells out to git directly.
```

That becomes spaghetti quickly.

---

## 9. Suggested source tree

```txt
src/
  main.rs

  app/
    mod.rs
    state.rs
    action.rs
    reducer.rs
    event.rs
    command.rs
    selection.rs
    transitions.rs

  terminal/
    mod.rs
    setup.rs
    restore.rs
    events.rs
    capabilities.rs
    panic_hook.rs

  git/
    mod.rs
    repo.rs
    commands.rs
    parser.rs
    worktree.rs
    refs.rs
    status.rs
    diff.rs
    remote.rs
    snapshot.rs
    snapshot_diff.rs

  live/
    mod.rs
    engine.rs
    fs_watcher.rs
    git_snapshotter.rs
    process_scanner.rs
    remote_poller.rs
    debouncer.rs
    scheduler.rs
    archive.rs

  process/
    mod.rs
    scanner.rs
    classifier.rs
    matcher.rs
    tree.rs

  ui/
    mod.rs
    layout.rs
    overview.rs
    worktrees.rs
    processes.rs
    collisions.rs
    cleanup.rs
    timeline.rs
    details.rs
    status_bar.rs
    command_palette.rs
    help.rs
    theme.rs
    widgets/
      mod.rs
      tree.rs
      badge.rs
      sparkline.rs
      event_log.rs
      confirm_dialog.rs

  config/
    mod.rs
    settings.rs
    keybindings.rs
    schema.rs

  storage/
    mod.rs
    event_store.rs
    archive_store.rs

  util/
    mod.rs
    paths.rs
    time.rs
    nul.rs
    shell.rs
    platform.rs
```

---

## 10. Repository discovery

When `wb300` starts, it should run from the current working directory and find the Git repository.

Commands:

```bash
git rev-parse --show-toplevel
git rev-parse --git-common-dir
git rev-parse --absolute-git-dir
git rev-parse --is-inside-work-tree
```

Required state:

```rust
struct RepoIdentity {
    start_dir: PathBuf,
    root: PathBuf,
    git_dir: PathBuf,
    common_git_dir: PathBuf,
    is_worktree: bool,
}
```

If not inside a Git repo:

- show a friendly error
- exit non-zero
- optionally support future multi-repo mode from a parent directory

---

## 11. Git data sources

### 11.1 Worktrees

Primary command:

```bash
git worktree list --porcelain -z
```

Parse fields:

```txt
worktree <path>
HEAD <oid>
branch <ref>
detached
bare
locked <reason>
prunable <reason>
```

Use this as the canonical list of worktrees.

### 11.2 Branches and refs

Primary command:

```bash
git for-each-ref \
  --sort=-committerdate \
  --format='%(refname)%00%(refname:short)%00%(objectname)%00%(upstream:short)%00%(committerdate:iso8601-strict)%00%(subject)%00%(worktreepath)%00' \
  refs/heads refs/remotes
```

Useful fields:

```txt
refname
refname:short
objectname
upstream:short
committerdate
subject
worktreepath
```

Use NUL-separated fields. Be careful with records and empty fields.

### 11.3 Worktree status

For each worktree:

```bash
git -C <worktree> status --porcelain=v2 --branch -z
```

Use to derive:

```txt
current branch
upstream
ahead/behind
staged changes
unstaged changes
untracked files
renames/copies if needed
conflict state
```

### 11.4 Dirty file lists

For each worktree:

```bash
git -C <worktree> diff --name-only -z
git -C <worktree> diff --staged --name-only -z
git -C <worktree> ls-files --others --exclude-standard -z
```

Use to compute:

- dirty/clean
- staged/unstaged/untracked counts
- changed-file overlap across worktrees
- hot file/path map

### 11.5 Branch changed files relative to base

For committed work already on the branch:

```bash
git -C <worktree> merge-base <base> HEAD
git -C <worktree> diff --name-only -z <base>...HEAD
```

This is important because two agents can collide even if both have already committed their changes and their worktrees are clean.

### 11.6 Ahead/behind relative to base

```bash
git -C <worktree> rev-list --left-right --count <base>...HEAD
```

Use this separately from upstream tracking.

A branch can track `origin/feature/foo` while still being far behind `origin/main`.

### 11.7 Remote state

Two modes:

#### Conservative default

```bash
git ls-remote --heads origin
```

Use for remote branch existence/head checks without updating local refs.

#### Explicit fetch/manual refresh

```bash
git fetch --all --prune
```

Use when the user presses `f`, or if the user opts into periodic fetch.

UI must distinguish:

```txt
local: live
remote: checked 42s ago
```

Remote-tracking refs are only as fresh as the last fetch.

---

## 12. Core data models

### 12.1 RepoSnapshot

```rust
struct RepoSnapshot {
    repo: RepoIdentity,
    captured_at: SystemTime,
    current_worktree: Option<PathBuf>,
    base_branch: Option<String>,
    worktrees: Vec<WorktreeSnapshot>,
    local_branches: Vec<BranchSnapshot>,
    remote_branches: Vec<RemoteBranchSnapshot>,
    collisions: Vec<CollisionSnapshot>,
    hot_paths: Vec<HotPathSnapshot>,
    process_summary: ProcessSnapshot,
    remote_summary: Option<RemoteSnapshot>,
}
```

### 12.2 WorktreeSnapshot

```rust
struct WorktreeSnapshot {
    id: WorktreeId,
    path: PathBuf,
    display_path: String,
    head: Option<String>,
    branch_ref: Option<String>,
    branch_short: Option<String>,
    detached: bool,
    bare: bool,
    locked: Option<String>,
    prunable: Option<String>,
    exists_on_disk: bool,
    status: Option<WorktreeStatus>,
    base: Option<BaseComparison>,
    processes: Vec<ProcessId>,
    last_file_activity_at: Option<SystemTime>,
    last_git_activity_at: Option<SystemTime>,
    lifecycle: WorktreeLifecycle,
}
```

### 12.3 WorktreeStatus

```rust
struct WorktreeStatus {
    clean: bool,
    staged_count: usize,
    unstaged_count: usize,
    untracked_count: usize,
    conflicted_count: usize,
    changed_files: Vec<ChangedFile>,
    ahead_upstream: Option<u32>,
    behind_upstream: Option<u32>,
    upstream: Option<String>,
    upstream_gone: bool,
}
```

### 12.4 BranchSnapshot

```rust
struct BranchSnapshot {
    name: String,
    full_ref: String,
    oid: String,
    upstream: Option<String>,
    subject: Option<String>,
    committer_date: Option<SystemTime>,
    worktree_path: Option<PathBuf>,
    base: Option<BaseComparison>,
    pushed_state: PushedState,
}
```

### 12.5 ProcessSnapshot

```rust
struct ProcessSnapshot {
    captured_at: SystemTime,
    processes: Vec<ProcessSnapshotItem>,
}

struct ProcessSnapshotItem {
    pid: u32,
    parent_pid: Option<u32>,
    cwd: Option<PathBuf>,
    matched_worktree: Option<WorktreeId>,
    command: String,
    executable: Option<PathBuf>,
    label: ProcessLabel,
    cpu_percent: Option<f32>,
    memory_bytes: Option<u64>,
    started_at: Option<SystemTime>,
    runtime: Option<Duration>,
    status: Option<String>,
}
```

### 12.6 ProcessLabel

```rust
enum ProcessLabel {
    Agent,
    Test,
    Build,
    DevServer,
    Shell,
    Runtime,
    Editor,
    Unknown,
}
```

Classify by process name/command using config:

```toml
[process_labels]
claude = "agent"
codex = "agent"
npm = "task"
pnpm = "task"
bun = "task"
cargo = "task"
node = "runtime"
python = "runtime"
zsh = "shell"
bash = "shell"
pwsh = "shell"
```

### 12.7 CollisionSnapshot

```rust
struct CollisionSnapshot {
    file_path: PathBuf,
    worktrees: Vec<WorktreeId>,
    branches: Vec<String>,
    severity: CollisionSeverity,
    source: CollisionSource,
}

enum CollisionSource {
    DirtyWorkingTree,
    StagedChanges,
    CommittedSinceBase,
    Mixed,
}

enum CollisionSeverity {
    Low,
    Medium,
    High,
    Critical,
}
```

Severity rules can be path-based:

```toml
[hot_paths]
critical = [
  "package-lock.json",
  "pnpm-lock.yaml",
  "Cargo.lock",
  "prisma/schema.prisma",
  "migrations/**",
  ".github/workflows/**"
]
high = [
  "package.json",
  "src/db/**",
  "src/auth/**"
]
```

### 12.8 LiveEvent

```rust
enum LiveEvent {
    WorktreeCreated { path: PathBuf, branch: Option<String>, head: Option<String> },
    WorktreeRemoved { path: PathBuf, branch: Option<String>, last_head: Option<String> },
    WorktreePrunable { path: PathBuf, reason: Option<String> },
    BranchCreated { name: String, head: String },
    BranchDeleted { name: String, last_head: Option<String> },
    BranchAdvanced { name: String, old_head: String, new_head: String },
    BranchPushed { name: String, remote: Option<String>, head: String },
    BranchBehindChanged { name: String, ahead: Option<u32>, behind: Option<u32> },
    WorktreeDirtyChanged { path: PathBuf, old: DirtySummary, new: DirtySummary },
    FileTouched { worktree: WorktreeId, file: PathBuf },
    FileStaged { worktree: WorktreeId, file: PathBuf },
    FileUnstaged { worktree: WorktreeId, file: PathBuf },
    ProcessStarted { worktree: Option<WorktreeId>, pid: u32, command: String },
    ProcessExited { worktree: Option<WorktreeId>, pid: u32, command: String },
    CollisionCreated { file: PathBuf, worktrees: Vec<WorktreeId> },
    CollisionResolved { file: PathBuf },
    RemoteBranchCreated { name: String, head: String },
    RemoteBranchDeleted { name: String, last_head: Option<String> },
    RemoteBranchAdvanced { name: String, old_head: String, new_head: String },
}
```

### 12.9 UI transitions

```rust
struct UiTransition {
    target: UiTarget,
    kind: TransitionKind,
    started_at: Instant,
    ttl: Duration,
}

enum TransitionKind {
    Created,
    Modified,
    Pushed,
    Deleted,
    ProcessStarted,
    ProcessExited,
    CollisionStarted,
    CollisionResolved,
    Archived,
}
```

Temporary highlight overlay should be separate from persistent state.

Example:

```txt
dirty = persistent yellow badge
recently pushed = temporary green flash
collision = persistent red/orange badge
recent file save = temporary yellow pulse
```

---

## 13. Live update design

### 13.1 Main rule

Filesystem events should not directly mutate final Git state.

Use this flow:

```txt
Filesystem event under worktree
        ↓
Mark worktree as recently touched
        ↓
Debounce 150–500ms
        ↓
Run git status/diff snapshot for that worktree
        ↓
Diff old vs new Git truth
        ↓
Emit semantic events
        ↓
Update UI state and transitions
```

### 13.2 Watch targets

At startup, watch:

1. current repo root
2. every linked worktree root
3. common Git dir
4. common Git dir worktree metadata
5. parent directories where worktrees are commonly created

Example likely worktree parent directories:

```txt
repo root parent
configured worktree_dir
all parent dirs of current known worktree paths
```

Watching parent directories helps detect:

```bash
git worktree add ../repo-auth-agent feature/auth-agent
```

before the next poll.

### 13.3 Polling is still required

Use periodic scans for correctness:

```txt
local Git worktree/ref snapshot: every 1–3 seconds
process table scan: every 1–3 seconds
remote ls-remote poll: every 30–120 seconds, configurable
fetch: manual by default, optional periodic
```

Suggested defaults:

```toml
[live]
enabled = true
local_poll_ms = 1500
process_poll_ms = 2000
remote_poll_secs = 60
fetch_on_remote_poll = false
ls_remote_on_remote_poll = true
highlight_ttl_ms = 4000
debounce_fs_ms = 300
```

### 13.4 Detect worktree creation

Compare previous and current `git worktree list --porcelain -z` snapshots.

If a new path appears:

1. emit `WorktreeCreated`
2. insert node into UI tree
3. start watching the new worktree root
4. scan status
5. scan processes under that path
6. flash the node as created

Example UI:

```txt
feature/auth-agent      new  active
└─ ../repo-auth-agent   created 4s ago
```

### 13.5 Detect worktree deletion

If a path disappears from the worktree snapshot:

1. emit `WorktreeRemoved`
2. store tombstone in archive
3. remove from active tree
4. show in Archive section
5. flash red/deleted for TTL if visible

Example archive:

```txt
ARCHIVE
⌫ feature/pdf-agent      deleted 42s ago   ../repo-pdf-agent   clean
⌫ feature/old-search     deleted 4m ago    ../repo-search-old  dirty snapshot saved
```

### 13.6 Detect modifications

When filesystem events happen under a worktree:

- immediately show “recent activity” for a few seconds
- after debounce, run `git status` and `git diff` queries
- update dirty counts
- update file lists
- recalculate collisions

Example UI:

```txt
feature/search-agent   dirty   saved now
```

### 13.7 Detect pushed state

Possible definitions:

1. Branch ahead of upstream changes from `>0` to `0`.
2. Local branch OID equals upstream/remote-tracking branch OID after fetch/status update.
3. WB-300 launched the push command and saw success.
4. Remote polling saw remote branch advance to the local OID.

For v1, support detection based on branch ahead/upstream state and explicit fetch/refresh.

UI:

```txt
feature/billing-agent      pushed ✓
```

Show green for 3–5 seconds, then return to persistent status.

### 13.8 Remote live state

Remote state is not truly live unless using webhooks/provider APIs. In v1:

```txt
local: live
remote: checked 42s ago
```

Modes:

```toml
[remote]
mode = "ls-remote" # "off" | "ls-remote" | "fetch"
```

Default:

```txt
manual fetch + periodic ls-remote
```

Do not default to periodic `fetch --all --prune` unless user config opts in.

### 13.9 Ignore paths

Default ignored watch paths:

```txt
.git/objects
node_modules
target
dist
build
.next
.turbo
.cache
coverage
venv
.venv
__pycache__
vendor
```

Still let Git status be final truth, but do not wake up the dashboard for every build artifact.

---

## 14. UI design

### 14.1 Core UI idea

Default view should be **Overview**, not a plain branch tree.

The operator wants answers to:

```txt
What is running?
What changed?
What is dirty?
What is risky?
What is idle?
What can I safely clean up?
Where should I look first?
```

### 14.2 Primary tabs

Recommended tabs:

```txt
1. Overview
2. Worktrees
3. Processes
4. Collisions
5. Cleanup
6. Timeline
7. Help/Settings
```

V1 can ship with:

```txt
Overview
Worktrees
Processes
Collisions
Cleanup
Help
```

Timeline can be added once event archive is implemented.

### 14.3 Overview mockup

```txt
repo: platform-api        local live        remote checked 21s ago        9 worktrees

ACTIVE
● feature/auth-agent        dirty      claude pid 18421     modified 3s ago
  └─ ../platform-auth       8 files     collision: src/auth/session.ts

● feature/billing-agent     pushed ✓   no process          pushed 5s ago
  └─ ../platform-billing    clean       origin synced

CHANGING
◐ feature/search-agent      dirty      codex pid 18588      saved now
  └─ ../platform-search     14 files

STALE
○ feature/old-agent         dirty      no process           idle 2d

ARCHIVE
⌫ feature/pdf-agent         deleted    ../platform-pdf      deleted 42s ago
```

### 14.4 Worktree tree mockup

```txt
repo: millis-app                         fetched 3m ago   7 worktrees   4 active

main
├─ origin/main                                      remote
├─ feature/auth-agent              running  dirty   +2/-0
│  └─ ../millis-auth-agent         claude pid 8112
├─ feature/billing-agent           idle     clean   +0/-1
│  └─ ../millis-billing-agent
├─ feature/search-agent            running  dirty   overlap: 4 files
│  └─ ../millis-search-agent       codex pid 8339
└─ chore/refactor-config           stale    dirty   no process
   └─ ../millis-config-refactor

DETAILS: feature/auth-agent
path: ../millis-auth-agent
base: origin/main
upstream: origin/feature/auth-agent
status: 8 modified, 2 untracked, 3 commits ahead
processes: claude pid 18421, cargo test pid 18502
collisions:
  src/auth/session.ts also touched by feature/rbac-cleanup
last activity: file change 1m ago
```

### 14.5 Process panel mockup

```txt
Running processes inside repo worktrees

PID     Worktree              Label      Command              CPU    Mem    Runtime
8112    auth-agent            agent      claude               9%     640mb  41m
8339    search-agent          agent      codex                14%    710mb  33m
9211    billing-agent         test       npm test             88%    1.2gb  4m
```

### 14.6 Collision view mockup

```txt
COLLISIONS

CRITICAL
prisma/schema.prisma
├─ feature/billing-agent     modified, staged
├─ feature/invoice-agent     modified
└─ feature/auth-agent        committed since base

HIGH
src/auth/session.ts
├─ feature/auth-agent        modified
└─ feature/rbac-cleanup      committed since base

MEDIUM
package.json
├─ feature/search-agent      modified
└─ feature/billing-agent     modified
```

### 14.7 Cleanup view mockup

```txt
Cleanup candidates

✓ ../repo-auth-agent       clean, merged, no process
✓ ../repo-old-test         clean, branch gone, no process
! ../repo-billing-agent    dirty, no process, 2d old
✗ ../repo-search-agent     active process running

Actions:
  r remove clean worktree
  p prune stale metadata
  s snapshot dirty changes
  l lock worktree
```

### 14.8 Timeline view mockup

```txt
EVENTS
16:41:03  feature/search-agent modified src/search/index.ts
16:41:01  feature/billing-agent pushed to origin
16:40:52  feature/pdf-agent worktree deleted, archived
16:40:44  feature/auth-agent collision detected src/auth/session.ts
16:40:12  feature/auth-agent process started claude
```

---

## 15. Visual status language

Use color and icons carefully. Also support no-color mode.

### Persistent states

```txt
clean          green or neutral check
active         bright dot
idle           dim dot
dirty          yellow
staged         purple/blue
untracked      yellow/gray
conflict       red
collision      orange/red
stale          dim gray
behind base    orange
upstream gone  red/orange
locked         blue lock
prunable       gray warning
detached       magenta/gray
```

### Temporary transitions

```txt
created        cyan/blue flash
modified       yellow flash
pushed         green flash for 3–5 seconds
deleted        red flash, then archive
process start  blue/green flash
process exit   gray/yellow flash
collision new  red/orange pulse
collision gone green flash
```

### No-color fallback

Use text badges:

```txt
[active]
[dirty]
[pushed]
[collision]
[stale]
[deleted]
```

---

## 16. Keybindings

### 16.1 Default keybindings

```txt
q / Esc          quit/back
?                help
Tab              next tab
Shift+Tab        previous tab
j / Down         move down
k / Up           move up
h / Left         collapse/back
l / Right        expand/open
Enter            select/open details
Space            expand/collapse
/                search/filter
:                command palette
r                refresh local snapshot
f                fetch --all --prune, with confirmation/config
R                remote check/poll now
p                push selected branch, optional later
P                PR action, optional later
s                snapshot selected worktree
S                snapshot all dirty worktrees
o                open shell in selected worktree
v                open editor/VS Code in selected worktree
c                copy selected path
C                copy selected branch name
x                cleanup action, context-specific
K                kill process, only if enabled and confirmed
```

### 16.2 Contexts

```rust
enum KeyContext {
    Global,
    Overview,
    WorktreeTree,
    ProcessTable,
    CollisionView,
    CleanupView,
    Timeline,
    Search,
    CommandPalette,
    ConfirmDialog,
    Help,
}
```

### 16.3 Action model

```rust
enum Action {
    Quit,
    Redraw,
    NextTab,
    PrevTab,
    MoveUp,
    MoveDown,
    Expand,
    Collapse,
    Select,
    OpenSearch,
    OpenCommandPalette,
    RefreshLocal,
    RefreshRemote,
    FetchAllPrune,
    SnapshotSelected,
    SnapshotAllDirty,
    OpenShell,
    OpenEditor,
    CopyPath,
    CopyBranch,
    OpenCleanup,
    Confirm,
    Cancel,
}
```

### 16.4 Configurable keybindings

Config example:

```toml
[keybindings.global]
quit = ["q", "esc"]
help = ["?"]
command_palette = [":"]
refresh = ["r"]
fetch = ["f"]

[keybindings.navigation]
down = ["j", "down"]
up = ["k", "up"]
expand = ["l", "right", "space"]
collapse = ["h", "left"]
select = ["enter"]
search = ["/"]
```

Avoid relying on keyboard shortcuts terminals commonly mangle:

```txt
Cmd+anything
Caps Lock
Ctrl+M distinct from Enter
Shift+Enter as the only newline shortcut
Alt/Meta without fallback
```

---

## 17. Feature specification

### 17.1 Branch/worktree explorer

Show:

- local branches
- remote-tracking branches
- branches checked out in worktrees
- detached worktrees
- bare worktrees
- locked worktrees
- prunable worktrees
- missing/deleted worktrees in archive

Views/grouping:

```txt
by base branch
by worktree path
by risk
by activity
by readiness
by stale/cleanup candidate
```

### 17.2 Process-to-worktree mapping

Scan OS processes and map CWD to known worktree roots.

Algorithm:

```txt
for each process:
  get cwd if available
  normalize path
  find longest worktree root prefix
  assign process to worktree
```

Enhance with process tree:

- if parent is inside worktree, child may belong to same worktree even if CWD differs
- classify process by command/name

Show:

```txt
PID
command
label
CPU
memory
runtime
cwd
parent PID
```

### 17.3 Dirty/clean status

For each worktree show:

```txt
clean/dirty
staged count
unstaged count
untracked count
conflicted count
changed files
latest save/activity time
```

### 17.4 Collision detection

Compute changed files for each worktree from:

1. unstaged changes
2. staged changes
3. untracked files, optionally
4. committed changes since base

Then invert:

```txt
file -> worktrees touching file
```

If `len(worktrees) >= 2`, create a collision.

Severity rules:

```txt
critical: lockfiles, migrations, database schema, generated API files, CI workflows
high: same source module/package area
medium: same normal source file
low: docs/test overlap or configured low-risk paths
```

### 17.5 Hot path map

Show paths touched by many worktrees:

```txt
src/db/schema.ts          touched by 4 worktrees
package.json              touched by 3 worktrees
src/types/api.ts          touched by 3 worktrees
.env.example              touched by 2 worktrees
```

### 17.6 Readiness scoring

Each worktree should derive an operator state:

```txt
Running
Changing
Needs attention
Ready to review
Idle dirty
Blocked/collision
Merged
Safe cleanup
Stale
Archived
```

Example readiness logic:

```txt
Ready to review if:
  no active agent-like process
  worktree clean
  branch ahead of base
  no current collisions
  optional: pushed/upstream exists
  optional: PR exists
  optional: checks pass

Needs attention if:
  process exited recently
  dirty changes remain
  test/build failed
  upstream gone
  branch behind base by threshold
  high-severity collision exists

Safe cleanup if:
  clean
  no active process
  branch merged or deleted remotely
  no dirty snapshot needed
```

### 17.7 Stale detection

Flag stale if:

```txt
no process is running inside worktree
last file modification older than threshold
last commit older than threshold
dirty changes exist and no activity
branch is behind base
upstream branch is gone
worktree path missing
worktree metadata is prunable
```

Config:

```toml
[stale]
idle_after_minutes = 90
dirty_idle_after_hours = 12
branch_age_days = 7
```

### 17.8 Worktree lifecycle actions

Support eventually:

```txt
create worktree from local branch
create worktree from remote branch
create scratch/detached worktree
remove clean worktree
force-remove dirty worktree, heavily confirmed
lock/unlock worktree
move worktree
repair worktree
prune stale worktree metadata
```

Use Git commands carefully:

```bash
git worktree add <path> <branch-or-commit>
git worktree remove <path>
git worktree prune
git worktree lock <path>
git worktree unlock <path>
git worktree move <old> <new>
git worktree repair
```

### 17.9 Snapshot/rescue actions

Before destructive actions, support snapshot:

```bash
git -C <worktree> diff > <snapshot>.patch
git -C <worktree> diff --staged > <snapshot>-staged.patch
git -C <worktree> status --porcelain=v2 -z > <snapshot>-status.bin
```

Storage:

```txt
<common_git_dir>/wb300/snapshots/
```

Snapshot metadata:

```json
{
  "timestamp": "2026-06-08T16:42:00Z",
  "worktree": "/path/to/worktree",
  "branch": "feature/auth-agent",
  "head": "abc123",
  "dirty_summary": { "staged": 2, "unstaged": 8, "untracked": 2 },
  "files": ["src/auth/session.ts"]
}
```

### 17.10 Event archive

Maintain local event history in the common Git dir:

```txt
<common_git_dir>/wb300/events.jsonl
```

or later:

```txt
<common_git_dir>/wb300/events.sqlite
```

Events to archive:

- worktree created
- worktree removed
- worktree archived
- branch created/deleted/advanced
- pushed detected
- process started/exited
- collision created/resolved
- snapshot created
- cleanup action completed

Example event:

```json
{
  "type": "worktree_removed",
  "repo": "/Users/emmett/dev/app",
  "path": "/Users/emmett/dev/app-auth-agent",
  "branch": "feature/auth-agent",
  "last_head": "abc123",
  "last_dirty_summary": {
    "modified": 0,
    "staged": 0,
    "untracked": 0
  },
  "timestamp": "2026-06-08T16:42:00Z"
}
```

### 17.11 Command palette

Commands:

```txt
create worktree from remote branch
open selected in shell
open selected in VS Code
copy worktree path
copy branch name
fetch all remotes
prune stale worktrees
show collisions
run tests in selected worktree
terminate selected process
snapshot dirty changes
archive deleted worktrees
show cleanup candidates
```

### 17.12 Test/build runner

Later feature.

Config:

```toml
[commands]
test = "pnpm test"
lint = "pnpm lint"
build = "pnpm build"
typecheck = "pnpm typecheck"
```

Show latest command state:

```txt
tests running
tests passed
tests failed
not run
```

Do not auto-run heavy commands by default.

### 17.13 Optional session launcher

Future feature:

```bash
wb300 run claude --branch feature/auth-agent --worktree ../auth-agent
```

Record:

```json
{
  "session_id": "2026-06-08T14-03-auth-agent",
  "worktree": "../auth-agent",
  "branch": "feature/auth-agent",
  "command": "claude",
  "pid": 18421,
  "started_at": "...",
  "base_branch": "origin/main"
}
```

This provides richer process/session info for agents launched through WB-300, but external agents should still be detected by process CWD scanning.

### 17.14 Log tailing

If WB-300 launches a process, capture stdout/stderr:

```txt
<common_git_dir>/wb300/sessions/<session-id>.log
```

TUI can show latest output:

```txt
last output:
  Editing src/auth/session.ts...
  Running tests...
  Test failed: expected 401, got 403
```

Do not require this for external agents.

### 17.15 PR/CI integration

Future feature.

Start with GitHub CLI, if available:

```bash
gh pr status
gh pr view --json state,mergeStateStatus,statusCheckRollup,url
```

Optional providers:

```toml
[providers.github]
enabled = true
mode = "gh"
```

Keep Git-only core independent from GitHub.

### 17.16 Multi-repo mode

Future feature.

Run from parent directory:

```bash
cd ~/work
wb300 --multi
```

Show:

```txt
api-platform       7 worktrees   4 active
web-app            3 worktrees   2 active
infra              1 worktree    0 active
```

---

## 18. Config design

### 18.1 Config locations

Project-local config:

```txt
<repo-root>/.wb300.toml
```

User config:

```txt
~/.config/wb300/config.toml
```

Repo event/archive state:

```txt
<common_git_dir>/wb300/
```

### 18.2 Example config

```toml
[workspace]
base_branch = "origin/main"
worktree_dir = "../"
agent_branch_patterns = [
  "agent/*",
  "feature/*-agent",
  "claude/*",
  "codex/*"
]

[live]
enabled = true
local_poll_ms = 1500
process_poll_ms = 2000
remote_poll_secs = 60
fetch_on_remote_poll = false
ls_remote_on_remote_poll = true
highlight_ttl_ms = 4000
debounce_fs_ms = 300

[watch]
recursive = true
use_poll_fallback = true
ignore = [
  "node_modules",
  "target",
  "dist",
  "build",
  ".next",
  ".turbo",
  ".cache",
  "coverage",
  "venv",
  ".venv",
  "__pycache__",
  "vendor"
]

[remote]
mode = "ls-remote" # "off" | "ls-remote" | "fetch"

[archive]
enabled = true
keep_days = 30

[stale]
idle_after_minutes = 90
dirty_idle_after_hours = 12
branch_age_days = 7

[processes]
allow_terminate = false
allow_force_kill = false

[process_labels]
claude = "agent"
codex = "agent"
npm = "task"
pnpm = "task"
bun = "task"
cargo = "task"
node = "runtime"
python = "runtime"
zsh = "shell"
bash = "shell"
pwsh = "shell"

[hot_paths]
critical = [
  "package-lock.json",
  "pnpm-lock.yaml",
  "Cargo.lock",
  "prisma/schema.prisma",
  "migrations/**",
  ".github/workflows/**"
]
high = [
  "package.json",
  "src/db/**",
  "src/auth/**"
]

[commands]
test = "pnpm test"
lint = "pnpm lint"
build = "pnpm build"
typecheck = "pnpm typecheck"

[keybindings.global]
quit = ["q", "esc"]
help = ["?"]
command_palette = [":"]
refresh = ["r"]
fetch = ["f"]

[keybindings.navigation]
down = ["j", "down"]
up = ["k", "up"]
expand = ["l", "right", "space"]
collapse = ["h", "left"]
select = ["enter"]
search = ["/"]
```

---

## 19. CLI design

### 19.1 Main commands

```bash
wb300
```

Open live TUI in current Git repo.

```bash
wb300 --repo /path/to/repo
```

Open TUI for a specific repo.

```bash
wb300 --no-live
```

Open static snapshot mode.

```bash
wb300 --no-alt-screen
```

Fallback renderer/debug mode.

```bash
wb300 --print-selected-path
```

Used by shell function to cd into selected worktree.

### 19.2 Future commands

```bash
wb300 snapshot
wb300 cleanup
wb300 run claude --branch feature/foo --worktree ../repo-foo
wb300 archive list
wb300 doctor
wb300 config init
```

### 19.3 Shell cd integration

A child process cannot change the parent shell’s current directory.

Provide shell function:

```bash
wb300cd() {
  local dir
  dir="$(wb300 --print-selected-path "$@")" || return
  cd "$dir"
}
```

Aliases:

```bash
alias wb='wb300'
alias wbc='wb300cd'
```

---

## 20. Safety behavior

### 20.1 Terminal restoration

Always restore terminal state on exit/panic:

- disable raw mode
- leave alternate screen
- disable mouse capture
- show cursor
- flush stdout

Use RAII guard and panic hook.

### 20.2 Destructive actions

Require confirmation for:

- remove worktree
- force remove worktree
- delete branch
- prune
- terminate process
- force kill process tree
- clean untracked files
- reset/rebase/stash if ever added

For dangerous actions, show exact target:

```txt
Remove worktree?
path: /Users/emmett/dev/repo-auth-agent
branch: feature/auth-agent
status: dirty, 8 modified, 2 untracked
processes: none

Type the branch name to confirm:
```

### 20.3 Snapshot before destruction

For dirty worktrees, offer/require snapshot before removing.

### 20.4 Process termination

Default config:

```toml
[processes]
allow_terminate = false
allow_force_kill = false
```

Visibility first, control second.

### 20.5 Remote operations

Fetch/push should not happen silently by default.

Manual fetch action should show:

```txt
Run git fetch --all --prune?
```

Periodic fetch only if configured.

---

## 21. Performance design

### 21.1 Do

```txt
watch filesystem events
use debouncing
batch multiple file events per worktree
limit concurrent Git subprocesses
use cheap snapshots often
use expensive snapshots less often
cache parsed refs
cache changed-file maps
skip ignored heavy folders for watcher events
virtualize long lists
render only when state changes or transition animations are active
```

### 21.2 Do not

```txt
run git status immediately on every save event
poll every worktree every 100ms
fetch constantly
watch node_modules recursively without ignore filtering
render every event separately without batching
block the UI event loop on Git commands
render entire huge trees if filtered/virtualized view is possible
```

### 21.3 Concurrency limits

Use a semaphore for Git commands:

```rust
let git_concurrency = 4;
```

Large monorepos and many worktrees can get expensive quickly.

### 21.4 Snapshot tiers

Fast frequent local scan:

```txt
worktree list
branch refs
process scan
selected/changed worktree status
```

Slower full scan:

```txt
status for all worktrees
changed files since base for all branches
remote poll
```

---

## 22. Cross-platform notes

### 22.1 Paths

- Normalize paths before prefix matching.
- Handle Windows drive letters/case-insensitivity.
- Handle symlinks carefully.
- Prefer canonical paths when possible, but do not require all paths to exist.
- Store original display paths separately from normalized paths.

### 22.2 Git for Windows

- Use installed `git.exe`.
- Be careful with path conversion between MSYS-style paths and Windows paths.
- Test in PowerShell, Windows Terminal, Git Bash, and WSL where possible.

### 22.3 Process CWD availability

- Some OSes/users may not allow reading CWD for every process.
- Treat process mapping as best-effort.
- Surface uncertainty quietly:

```txt
process cwd unavailable
```

### 22.4 Filesystem watcher caveats

- Network filesystems may not emit events.
- Editors save differently.
- Parent deletion requires watching parent.
- Linux watch limits can be hit.
- Large directory trees can miss events.
- Docker/WSL/macOS paths may need poll fallback.

Use polling fallback if needed.

### 22.5 Terminal input

- Filter key events to press events.
- Mouse support optional.
- Alternate screen optional.
- Provide no-color mode.
- Provide `--no-alt-screen` and possibly `--classic` renderer.

### 22.6 Parent shell cd limitation

`wb300` cannot change the parent shell’s cwd directly. Provide shell integration.

---

## 23. MVP scope

The best MVP is not small, but it should be focused.

### MVP must have

1. Run `wb300` inside a Git repo.
2. Discover repo root and common Git dir.
3. Discover all Git worktrees using `git worktree list --porcelain -z`.
4. Discover local and remote-tracking branches.
5. Show branch/worktree tree.
6. Show dirty/clean/staged/untracked status for each worktree.
7. Show ahead/behind versus upstream/base where available.
8. Scan OS processes and map CWD to worktrees.
9. Show active/idle worktrees based on process mapping.
10. Watch filesystem and Git metadata for live local changes.
11. Poll local Git snapshots periodically.
12. Detect created/deleted worktrees while open.
13. Detect changed-file overlaps/collisions across worktrees.
14. Show temporary highlights for created/modified/pushed/deleted.
15. Store deleted worktree tombstones in local archive.
16. Search/filter branches/worktrees.
17. Details pane for selected worktree.
18. Fetch/refresh action.
19. Safe cleanup screen for obvious cleanup candidates.
20. Safe terminal restoration.

### MVP should not yet include

- built-in agent launcher
- log capture/tailing
- PR/CI provider integration
- process killing by default
- automatic push/rebase/reset
- multi-repo mode
- embedded PTY panes

---

## 24. Implementation phases

### Phase 0: skeleton

Goals:

- create Rust project
- parse CLI args
- initialize terminal
- render simple Ratatui layout
- restore terminal safely
- basic key handling

Tasks:

```txt
cargo new wb300
add dependencies
implement terminal guard
implement app loop
render header/footer
q exits
? opens help placeholder
```

Acceptance:

```txt
wb300 opens alternate-screen TUI and exits cleanly.
Terminal is restored after panic or Ctrl+C.
```

### Phase 1: Git discovery

Goals:

- detect repo root/common Git dir
- parse worktree list
- parse refs

Tasks:

```txt
implement git command wrapper
implement RepoIdentity
parse git worktree list --porcelain -z
parse git for-each-ref NUL format
show static tree of branches/worktrees
```

Acceptance:

```txt
Inside a repo with multiple worktrees, WB-300 shows them correctly.
Detached/locked/prunable states are displayed when present.
```

### Phase 2: status and details

Goals:

- dirty/clean status per worktree
- selected details pane

Tasks:

```txt
run git status --porcelain=v2 --branch -z per worktree
parse staged/unstaged/untracked/conflicted counts
show selected worktree details
show upstream/ahead/behind
implement refresh action
```

Acceptance:

```txt
Editing/staging/untracking files changes displayed status after refresh.
```

### Phase 3: process mapping

Goals:

- scan process table
- map process CWD to worktrees
- show active/idle status

Tasks:

```txt
integrate sysinfo
normalize paths
prefix-match cwd to worktree roots
classify process labels
render process table
render active/idle badges
```

Acceptance:

```txt
Running claude/node/cargo inside a worktree shows under that worktree.
```

### Phase 4: live local engine

Goals:

- filesystem watcher
- local polling
- debounce refreshes
- semantic events

Tasks:

```txt
integrate notify
watch repo/worktree/common git dirs
implement debouncer
implement LiveEvent enum
implement snapshot differ
implement UI transitions
highlight modified worktree on save
```

Acceptance:

```txt
When a file is saved in a worktree, WB-300 marks that worktree as recently modified and updates status without manual refresh.
```

### Phase 5: created/deleted/archive

Goals:

- detect worktree creation/deletion live
- archive tombstones

Tasks:

```txt
poll worktree list every 1-3 seconds
watch likely parent dirs
diff worktree snapshots
emit WorktreeCreated/Removed
persist events JSONL
render Archive section
```

Acceptance:

```txt
If another terminal creates a worktree, it appears live.
If another terminal removes a worktree, it moves to archive live.
```

### Phase 6: collision detection

Goals:

- changed-file overlap map
- collision view
- risk badges

Tasks:

```txt
collect unstaged/staged/untracked file sets
collect files changed since base
invert file -> worktrees
apply severity rules
render collision tab
highlight affected worktrees
```

Acceptance:

```txt
If two worktrees modify the same file, WB-300 shows a collision warning.
```

### Phase 7: remote/pushed state

Goals:

- remote checked timestamp
- `ls-remote` or fetch action
- detect pushed transition

Tasks:

```txt
implement remote poll mode config
implement manual fetch action
compare ahead/upstream states
emit BranchPushed when ahead drops to 0 or remote matches
flash green for pushed state
```

Acceptance:

```txt
After a branch is pushed and refreshed/fetched, WB-300 shows pushed green briefly and updates status.
```

### Phase 8: cleanup/safety

Goals:

- cleanup candidates
- snapshot dirty changes
- safe confirmations

Tasks:

```txt
implement cleanup view
detect safe removal candidates
implement snapshot patch export
implement worktree prune/remove commands with confirmation
protect dirty/active worktrees
```

Acceptance:

```txt
Clean inactive merged/gone worktrees are suggested for cleanup.
Dirty/active worktrees are not removed without explicit confirmation and snapshot option.
```

### Phase 9: packaging

Goals:

- build release binaries
- installable CLI

Tasks:

```txt
setup cargo-dist
setup GitHub Actions
build macOS/Linux/Windows targets
write README install instructions
```

Acceptance:

```txt
GitHub release contains wb300 binaries for common platforms.
```

---

## 25. Claude Code implementation prompt

Use this prompt to hand the project to Claude Code:

```txt
We are building WB-300, invokable as `wb300`.

WB-300 is a cross-platform Rust TUI operator console for supervising many parallel coding-agent Git worktrees. It runs inside a Git repo and shows local branches, remote-tracking branches, Git worktrees, dirty/clean status, process activity, changed-file collisions, live updates, pushed/deleted/archive transitions, and cleanup candidates.

Use Rust + Ratatui + Crossterm + Tokio. Use the installed `git` CLI via async subprocesses in v1. Do not use git2 or gix initially.

Core architecture:

KeyEvent -> KeybindingResolver -> Action -> AppState reducer -> async Git/Process/FS task -> RepoSnapshot/LiveEvent -> UI transition manager -> Ratatui render.

The app must not block the UI thread on Git commands.

Core modules:
- terminal: raw mode, alternate screen, input, teardown, panic-safe restore
- git: repo discovery, command wrapper, worktree parser, ref parser, status parser, diff parser, remote parser, snapshot diffing
- live: filesystem watcher, process scanner, Git snapshotter, remote poller, debounce scheduler, event archive
- process: sysinfo scanning, cwd-to-worktree mapping, process labels
- app: state, actions, reducer, selection, transitions
- ui: overview, worktrees, processes, collisions, cleanup, details, command palette, help
- config: settings and keybindings
- storage: event/archive store

MVP requirements:
1. `wb300` opens a Ratatui alternate-screen TUI inside a Git repo.
2. Discover repo root/common Git dir.
3. Parse `git worktree list --porcelain -z`.
4. Parse local and remote-tracking branches via `git for-each-ref` with NUL-separated fields.
5. Show a navigable branch/worktree tree.
6. Show dirty/clean/staged/unstaged/untracked/conflicted status per worktree via `git status --porcelain=v2 --branch -z`.
7. Show details for selected worktree.
8. Scan OS processes with sysinfo and map process CWD to known worktree roots.
9. Show active/idle badges and process list per worktree.
10. Watch worktree directories with notify and debounce Git refreshes.
11. Poll local Git snapshots periodically so missed filesystem events are corrected.
12. Detect worktree creation/deletion while open.
13. Store deleted worktree tombstones in `<common_git_dir>/wb300/events.jsonl`.
14. Detect changed-file collisions across worktrees.
15. Show temporary visual highlights for created, modified, pushed, deleted, process started/exited, collision created/resolved.
16. Provide search/filter.
17. Provide manual refresh and fetch actions.
18. Provide safe cleanup view.
19. Restore terminal state safely on exit/panic.

Use NUL-safe Git parsing wherever possible. Filesystem watcher events are hints only; Git snapshots are truth. Remote state should be labeled as `remote checked <time> ago`; do not silently fetch constantly by default.

Start with Phase 0 and Phase 1 from the handoff plan. Keep each module small and testable. Add unit tests for Git parsers using fixture strings.
```

---

## 26. First concrete coding tasks

### Task 1: initialize project

```bash
cargo new wb300
cd wb300
```

Add dependencies.

### Task 2: terminal guard

Implement:

```rust
struct TerminalGuard;
```

Responsibilities:

- enter alternate screen
- enable raw mode
- hide cursor
- enable mouse capture later
- restore on drop
- panic hook restoration

### Task 3: static TUI shell

Render:

- header
- tab bar
- empty worktree list
- details panel
- footer key hints

### Task 4: Git command wrapper

Implement safe async Git subprocess wrapper:

```rust
async fn git(repo: Option<&Path>, args: &[&str]) -> Result<GitOutput>;
```

Support:

- cwd
- timeout
- stdout bytes
- stderr bytes
- exit status
- structured error

### Task 5: repo discovery

Implement:

```rust
RepoIdentity::discover(start_dir: &Path) -> Result<RepoIdentity>
```

Commands:

```bash
git rev-parse --show-toplevel
git rev-parse --git-common-dir
git rev-parse --absolute-git-dir
git rev-parse --is-inside-work-tree
```

### Task 6: worktree parser

Unit-test parser for:

```bash
git worktree list --porcelain -z
```

Handle:

- normal branch worktree
- detached worktree
- bare worktree
- locked worktree
- prunable worktree
- paths with spaces/newlines if possible

### Task 7: ref parser

Unit-test parser for NUL-separated `for-each-ref` output.

### Task 8: first real screen

Show static discovered worktrees and branches in the TUI.

---

## 27. Testing strategy

### 27.1 Unit tests

Prioritize parsers:

- worktree porcelain parser
- for-each-ref parser
- status porcelain v2 parser
- NUL splitting
- path normalization
- collision detection
- snapshot diffing
- config loading
- keybinding resolution

### 27.2 Integration tests

Create temporary repos:

```bash
git init
git commit --allow-empty -m init
git worktree add ../repo-feature -b feature/foo
```

Test:

- worktree discovery
- branch discovery
- dirty status
- staged/untracked status
- worktree remove/archive detection
- collision detection

### 27.3 Manual test matrix

```txt
macOS Terminal.app
macOS iTerm2
Windows Terminal PowerShell
Windows Terminal Git Bash
Linux GNOME Terminal
Linux tmux
WSL, if feasible
```

### 27.4 Large repo tests

Test with:

- many branches
- many worktrees
- many untracked files
- node_modules present
- large monorepo paths
- long branch names
- spaces in paths

---

## 28. Important edge cases

### 28.1 Detached worktrees

A worktree can be detached without branch.

Display:

```txt
detached @ abc123
```

### 28.2 Locked worktrees

Display lock reason.

Do not suggest locked worktrees for cleanup unless explicit.

### 28.3 Prunable/missing worktrees

Show as metadata cleanup candidates.

### 28.4 Branch checked out elsewhere

Git may prevent checking out the same branch in multiple worktrees. Display branch worktree path from refs if available.

### 28.5 Upstream gone

Branch can track a remote branch that no longer exists.

Display:

```txt
upstream gone
```

### 28.6 Merge/rebase/cherry-pick state

Detect files in `.git`/worktree git dir:

```txt
MERGE_HEAD
REBASE_HEAD
rebase-merge/
rebase-apply/
CHERRY_PICK_HEAD
BISECT_LOG
```

Show:

```txt
merging
rebasing
cherry-picking
bisecting
```

### 28.7 Main worktree vs linked worktrees

The original repo root is also a worktree. Show it clearly.

### 28.8 Remote names other than origin

Do not hardcode origin everywhere. Use configured default remote, fallback to origin.

### 28.9 Multiple base branches

Allow repo config:

```toml
[workspace]
base_branch = "origin/main"
```

Later support branch pattern to base mapping.

### 28.10 Deleted worktree with dirty changes

If WB-300 saw dirty state before deletion, archive that last known dirty summary.

If not, mark uncertainty:

```txt
deleted, last dirty state unknown
```

---

## 29. Documentation to include in repo

Recommended docs:

```txt
README.md
docs/architecture.md
docs/git-parsing.md
docs/live-engine.md
docs/config.md
docs/keybindings.md
docs/safety.md
docs/platform-notes.md
```

README sections:

- what WB-300 is
- why it exists
- installation
- quick start
- screenshots/GIF later
- keybindings
- config
- safety notes
- limitations

---

## 30. Current reference links

These references were used while discussing the plan and should be revisited during implementation for latest versions and exact APIs.

- Ratatui: https://ratatui.rs/
- Ratatui docs.rs: https://docs.rs/ratatui/latest/ratatui/
- Crossterm docs.rs: https://docs.rs/crossterm/
- Crossterm event module: https://docs.rs/crossterm/latest/crossterm/event/index.html
- Notify docs.rs: https://docs.rs/notify/latest/notify/
- Sysinfo docs.rs: https://docs.rs/sysinfo/latest/sysinfo/
- Git worktree docs: https://git-scm.com/docs/git-worktree
- Git for-each-ref docs: https://git-scm.com/docs/git-for-each-ref
- Git status docs: https://git-scm.com/docs/git-status
- Git diff docs: https://git-scm.com/docs/git-diff
- Git merge-base docs: https://git-scm.com/docs/git-merge-base
- Git remote branches book section: https://git-scm.com/book/en/v2/Git-Branching-Remote-Branches
- Rust platform support: https://doc.rust-lang.org/rustc/platform-support.html
- cargo-dist: https://github.com/axodotdev/cargo-dist

---

## 31. Final product thesis

WB-300 should not merely show a pretty tree.

A pretty tree tells the operator what exists.

WB-300 should tell the operator:

```txt
what is alive
what is changing
what is risky
what is abandoned
what is ready
what needs attention
what can be cleaned safely
```

That is the difference between a Git browser and an operator console.

The most important features are:

1. **Live local updates**
2. **Process-to-worktree correlation**
3. **Changed-file collision detection**
4. **Readiness/risk lanes**
5. **Deleted worktree archive/timeline**
6. **Safe cleanup and rescue snapshots**

Build those well before adding nice-to-have Git GUI features.

