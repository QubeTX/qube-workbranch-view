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
- Remote / pushed state: `f` runs `git fetch --all --prune` off the UI task — **explicit
  only, never automatic** (the "not a remote hammer" rule) — with a header indicator
  (`⟳ fetching…` / `remote <age> ago` / `remote: not checked`, updated only on success). A
  worktree flashes green **Pushed** when its ahead-count clears to 0 while even with a live
  upstream (heuristic, handoff §13.7).
- Review fixes: the Pushed heuristic requires `behind == 0` + a live upstream (no
  fetch-fast-forward false positive; reset-to-upstream ambiguity documented); a re-press of `f`
  during an in-flight fetch is no longer silently dropped.
- Cleanup + safety + search + palette: a **Cleanup** tab scores each worktree
  (safe / caution / dirty / active / protected) with reasons; `/` filters the worktree list,
  `:` opens a command palette, `x` removes the selected worktree behind a **type-the-name
  confirm** dialog (a rescue patch is saved to `<state_dir>/snapshots/` first for dirty
  worktrees), and `p` prunes stale metadata (also confirmed). Main / current / bare worktrees
  and any with a running process are protected. All mutations run off the UI task, behind an
  overlay/input-mode system routed through the reducer.
- Review fixes (data-loss focus): a failed rescue snapshot now **aborts** the removal (never
  force-delete unrescued work); prune requires confirmation; the confirm token is typeable for
  detached/unknown worktrees (branch → short oid → `REMOVE`).
- Full Windows distribution (parity with TR-300): four first-class installers — Global MSI
  (`wix/main.wxs`, cargo-dist-built), Corporate MSI (`wix-corporate/corporate.wxs`), Global EXE
  (`inno/global.iss`), Corporate EXE (`inno/corporate.iss`) — with permanent product GUIDs,
  per-edition install paths (`%ProgramFiles%\wb300\bin` / `%LocalAppData%\Programs\wb300\bin`),
  PATH management (system vs user), and `HKCU\Software\WB300\InstallSource` markers
  (`msi-global` / `msi-corporate` / `exe-global` / `exe-corporate`).
- `.github/workflows/windows-installers.yml`: hand-authored, chains off `release.yml` via
  `workflow_run` (the GITHUB_TOKEN-suppression-safe trigger) plus `workflow_dispatch`; verifies
  the upstream cargo-dist release is complete (probes `dist-manifest.json` + the Global MSI),
  builds the Corporate MSI via bare `candle`/`light` (`-sice:ICE38/64/91`) and the two EXEs via
  Inno Setup 6, writes `.sha256` sidecars, and `gh release upload --clobber`s the 6 add-on
  assets.
- Registry-aware self-update (`src/update.rs`, wiring `wb300 update` + `update --json`): queries
  the GitHub releases API, compares semver (prerelease/build-metadata aware), and dispatches to
  an install-origin-matched strategy. On Windows it reads the `InstallSource` marker (path-based
  fallback for cargo/PowerShell installs), downloads the matching MSI/EXE, **verifies its
  SHA-256 against the published sidecar before running it**, and confirms the post-install
  `--version`; elsewhere it prefers `cargo install` (with a crates.io-lag re-verify) then the
  shell installer. New deps: `ureq`; `winreg` + `sha2` (Windows only). 31 unit tests.
- `deploy.sh`: one-command release wrapper scripting the CLAUDE.md runbook in two PR-gated
  phases — `bump <patch|minor|major|X.Y.Z>` (version bump + lockstep-changelog guard + local
  gates + scoped commit + branch push) and `tag` (clean-tree / on-main / CI-green / no-retag
  checks, then push the `vX.Y.Z` tag). README install matrix (6 channels) and the CLAUDE.md
  deploy runbook + lockstep contract updated to match.
