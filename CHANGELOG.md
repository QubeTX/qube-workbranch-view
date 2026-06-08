# Changelog

All notable changes to wb300 are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/), and this project adheres to
[Semantic Versioning](https://semver.org/).

When you add or amend an entry here, update `HUMAN_CHANGELOG.md` in the same commit.

## [Unreleased]

### Added
- Project scaffold: cross-platform Rust + Ratatui TUI skeleton (`wb300`, edition 2024) with a
  panic-safe `TerminalGuard` (RAII restore + panic hook), alternate-screen rendering, a tabbed
  shell (Overview / Worktrees / Processes / Collisions / Cleanup / Help), and a
  `KeyEvent → Action → reducer` input loop with `KeyEventKind::Press` filtering.
- `--repo`, `--no-live`, `--no-alt-screen`, `--no-color`, `--print-selected-path` flags and an
  `update` subcommand (stub).
- Full QubeTX packaging/distribution wiring mirroring TR-300: cargo-dist 0.31.0 (shell /
  PowerShell / MSI across the six targets — macOS Intel+ARM, Linux x64 glibc+musl+ARM64,
  Windows x64), tag-triggered `release.yml`, push-gated `crates-publish.yml`, and `ci.yml`
  (fmt / clippy / test / build / audit / dist-plan).
- PolyForm Noncommercial 1.0.0 `LICENSE`, MSRV pin (1.95) in lockstep with
  `rust-toolchain.toml`, `clap_mangen` man-page generation via `build.rs`, and project docs
  (`CLAUDE.md`, `AGENTS.md`, `docs/`).
- Git layer: `RepoIdentity::discover` (repo root + git/common dir via `rev-parse
  --path-format=absolute`) and an async, timeout-guarded `git` CLI wrapper that suppresses
  interactive prompts (`GIT_TERMINAL_PROMPT=0`).
- Fixture-tested NUL-safe parsers for `git worktree list --porcelain -z` and
  `git for-each-ref`, captured into a `RepoSnapshot` (worktrees + local/remote refs).
- Worktrees tab renders the live worktree list — current-worktree marker plus
  detached/locked/prunable flags — with `j`/`k` navigation; Overview shows worktree and
  local/remote branch counts. Launching outside a repo prints a friendly message and exits
  non-zero (the machine-wide view for non-repo directories is deferred to a later phase).
- Per-worktree status via `git status --porcelain=v2 --branch -z` (fixture-tested parser that
  handles rename records, conflicts, and the upstream-gone case): staged/unstaged/untracked/
  conflicted counts, ahead/behind, upstream, and upstream-gone.
- Worktrees tab now shows status badges (dirty counts, ↑/↓ divergence, upstream-gone) and a
  details pane for the selected worktree; Overview shows a dirty-worktree count. `r` refreshes
  the snapshot.
- Integration tests against real temporary repositories (dirty status + linked-worktree
  discovery), guarding the parsers against real-world format drift.
- Process→worktree mapping (`sysinfo`): each process's CWD is matched to a worktree
  (longest-prefix, path-boundary aware, case-insensitive on Windows) and classified
  (agent / task / runtime / shell / editor), recognizing Claude/Codex/etc. by executable name
  or via a runtime wrapper (`node …/claude.js`), with a guard against agent-named directories.
- Processes tab: a table (pid · worktree · label · command · cpu · mem · runtime) with agent
  rows highlighted; the Worktrees list shows a live `● <agent> pid N` badge and Overview shows
  an active-worktree count. The scan runs off the async executor (`spawn_blocking`).
- Unit tests for classification and prefix matching (`src/util/paths.rs`,
  `src/process/classifier.rs`).
- Live local engine: a `notify` filesystem watcher (ignoring `.git/objects` + heavy build
  dirs) feeds a debouncer that coalesces bursts into a single refresh. The event loop is now a
  `tokio::select!` over terminal input, debounced refreshes, a periodic poll backstop (1.5s),
  an animation tick, and completed captures — which run on spawned tasks so the UI never blocks
  on Git.
- Transient highlights (4s TTL): worktrees flash Created / Modified when a snapshot diff
  detects a change. `--no-live` disables the watcher + poll (manual `r` still refreshes).
- Watcher hardening (post-review): watches the *pruned* source tree (skips
  `.git`/`node_modules`/`target`/…), non-recursive per directory, **capped at 2048 watches** —
  bounded on every OS so it never exhausts the shared Linux `inotify` budget. A header
  indicator shows `live` / `poll-only` / `static`.
- Fixed: a failed capture no longer wedges the live engine (the in-flight guard always resets
  via an `Option` result channel); `r` now routes through the reducer (a pending-refresh flag)
  rather than being intercepted, keeping the reducer the single mutation point.
- Event archive + Timeline: created/removed worktrees are detected on each snapshot diff,
  flashed (Created/Deleted), and recorded to `<common_git_dir>/wb300/events.jsonl`; a new
  **Timeline** tab (keys now 1–7) shows the history newest-first, surviving across sessions.
- Bounded + safe (post-review): a single dedicated writer thread owns the file (ordered,
  non-interleaved appends); the in-memory archive is a capped `VecDeque` (2000) and the on-disk
  log is pruned by age (30 days) + count and compacted at startup. `tracing` diagnostics now go
  to `<state_dir>/wb300.log`, so watcher/archive failures are no longer silent.
- Collision detection: per-worktree "touched files" (unstaged + staged + committed-since-base
  via `<base>...HEAD`) are inverted to find files touched by ≥2 worktrees, ranked by built-in
  hot-path severity (Critical: lockfiles / migrations / schema / CI → High: manifests / db /
  auth → Medium: source → Low: docs / tests). New **Collisions** tab grouped by severity, a
  `⚠ N` worktree badge, and an Overview count. Capture detects a base ref (origin/main →
  origin/master → main → master) and surfaces when none is found (committed-conflict detection
  inactive). Per-worktree git work now runs with bounded concurrency (4).
- Review fixes: `schema.prisma` over-match tightened to `/schema.prisma`; defensive
  worktree-name fallback in collision rows.
