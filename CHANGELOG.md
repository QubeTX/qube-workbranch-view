# Changelog

All notable changes to wb300 are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/), and this project adheres to
[Semantic Versioning](https://semver.org/).

When you add or amend an entry here, update `HUMAN_CHANGELOG.md` in the same commit.

## [Unreleased]

## [1.2.0] - 2026-06-09

A live-activity and agent-control release: a refined per-worktree visual language, the ability
to terminate forgotten/stuck agent processes, an honest agent classifier, and the collisions
view reframed as merge-conflict risk.

### Added
- **Kill a process — on request, never automatically.** `K` on the Worktrees tab terminates the
  agent attached to the selected worktree; the Processes tab is now selectable (`j`/`k`) with `K`
  to terminate any mapped process. Both behind a type-the-PID confirm showing `pid · name · cwd`,
  with a PID-reuse guard (`process::scanner::kill` re-verifies the executable name before
  signalling) and an on-screen result in the footer (`✓ terminated …` / `⚠ kill failed: …`).
  wb300 refuses to terminate itself and never lists its own process.
- **Live save activity** now plays as an ordered flash *sequence*: a back-to-back commit+push
  shows magenta then green (`app::transitions` is now a timed sequence with per-kind TTLs and a
  priority guard; `change_kind` returns an ordered `Vec<TransitionKind>`).
- **Overview live summary + freshness:** replaces the old placeholder with `◆ N editing right
  now` plus a freshness line (`● live · updated Ns ago` / `◐ poll-only` / `○ static` / `⚠ stale
  — last good capture Ns ago`), driven by a new `AppState::last_updated`.
- **Merge-conflict-risk log:** a new `EventKind::ConflictRisk` archives each newly-appearing
  cross-worktree file overlap (keyed by file + worktree set) to the Timeline.

### Changed
- **Per-worktree visuals:** a leading live dot (blue `◆` while editing a clean tree) plus the
  whole row recolored by state — **whole-line yellow while uncommitted** (dot included), a
  **magenta** commit / **green** push milestone flash over the whole row (precedence over
  yellow), and **dim** when a worktree's status is unknown. Worktree-list selection no longer
  overrides the row foreground, so flashes stay visible on the selected row.
- **Collisions → "Merge Risk".** The tab is renamed and the panel reframed as a merge forecast:
  *files changed on 2+ worktrees, likely to conflict when merged into `<base>`*, annotated with
  each worktree's agent. (Cross-worktree overlap is merge-conflict risk, not a live collision —
  worktrees are isolated copies.)
- **Honest agent classifier:** Claude *Desktop* (the Electron GUI app and its
  gpu/renderer/crashpad helpers) is no longer mistaken for a coding agent
  (`process::classifier` excludes `--type=` helpers and the packaged app path); real agent CLIs
  (`.local/bin/claude.exe`, `…/claude-code/<ver>/claude.exe`, `claude-agent-sdk`) still count.
- **Terminology:** user-facing "dirty" is now "uncommitted" throughout.
- Removed worktrees on a `main`/`master` branch are now protected from removal alongside the
  primary checkout, current worktree, and bare repos (`AppState::is_protected`).

### Fixed
- Per-worktree status failures already logged (v1.1.0) are reinforced by a dim "unknown" row
  affordance so an unreadable `git status` never renders as clean.

## [1.1.0] - 2026-06-08

Live activity signals on the lowest-level node (the worktree), so an operator watching a swarm
of agents can see what's being written, what's uncommitted, and what just shipped — at a glance.

### Added
- **Live save marker.** A blue `◆` flashes on a worktree row while files are being created /
  modified / deleted inside it, with a short save-driven TTL (~0.6s) so it tracks the file being
  written and goes dark within ~½s of saves stopping. Driven by a new immediate (un-debounced)
  activity lane: `live/fs_watcher.rs` now forwards the changed `Vec<PathBuf>` and
  `live/debouncer.rs` splits it into a lossy activity lane (`AppState::note_activity` /
  `HomeState::note_activity` → `note_activity_for`, mapping each path to its worktree via
  `util::paths::longest_prefix_match`) plus the existing coalesced refresh. Works in both the
  per-repo and `--home` views.
- **Persistent uncommitted highlight.** A worktree's name is held yellow the whole time it has
  uncommitted changes (`ui/mod.rs`, `ui/home.rs`); an *unknown* status (a failed `git status`)
  renders dim, never as clean.
- **Commit / push milestone flashes.** The whole row briefly recolors (~1.8s) — magenta on a
  commit (HEAD moved, new `TransitionKind::Committed` in `app::state::change_kind`), green on a
  push — then settles back. New `theme::ACTIVITY` (blue) and `theme::COMMITTED` (magenta).
- Per-entry TTLs and a priority guard on `Transitions` so the frequent activity pulse never
  stomps a live milestone/structural flash (`app/transitions.rs`).
- Render-buffer tests (ratatui `TestBackend`) asserting the painted colors for the dirty-yellow
  name, the blue activity marker, and the magenta/green milestone recolors.

### Changed
- Worktree-list selection is now marked by bold + the `▸ ` cursor only (no foreground override),
  so live flashes and the uncommitted-yellow name stay visible on the *selected* row too
  (`ui/mod.rs`).
- Unified the "pushed" color across views to green (the home view previously used cyan).
- Removed the old git-status-delta "modified" flash (`TransitionKind::Modified`); it is
  superseded by the blue activity marker (transient) plus the persistent uncommitted-yellow name.

### Fixed
- A failed `RepoSnapshot::capture` no longer silently shows stale data: it is logged and a
  `⚠ stale` flag is surfaced in the header so "● live" can't imply fresh data when it isn't
  (`lib.rs`, `app/state.rs`, `ui/mod.rs`).
- Per-worktree `git status` failures are logged instead of being silently dropped and read as
  "clean" (`git/snapshot.rs`); the home view logs a repo whose capture fails rather than letting
  it vanish silently (`home/snapshot.rs`).

## [1.0.0] - 2026-06-08

First public release. Everything below shipped in `v1.0.0`.

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
- Machine-wide **home view** (`src/home/`, `src/ui/home.rs`): run `wb300` outside a Git repo —
  or with `--home` / `--multi` — to open a control tower over every repo being actively worked
  on. Discovery is process-driven first (every repo with a running agent, via a machine-wide
  `sysinfo` scan → `RepoIdentity::discover` of each agent's CWD) then supplemented by a bounded
  scan of `~/git`, `~/code`, … for repos set up for parallel work (≥ 2 worktrees), deduplicated
  by shared git dir and capped (≤ 32 repos). Each repo is captured with the full per-repo
  pipeline; cards (active-agent repos first) break worktrees down by workbranch with agent
  labels (`● claude pid N`), dirty/↑↓ badges, ⚠ collisions, and the same created/modified/
  pushed/deleted colour flashes (keyed by worktree path, reusing `change_kind`). `j`/`k` select,
  `Enter` drills into a repo's full per-repo view (returning to home on quit), `r` rescans.
- Home live engine: a `tokio::select!` loop with a filesystem watcher across all discovered
  worktree roots + a 2.5s rescan backstop, captures running off the UI task at bounded
  concurrency (≤ 3 repos at once). Blocking directory enumeration runs on `spawn_blocking`;
  the drill-in drains the watch backlog on return so it rescans once, not in a burst.
- `--home` (alias `--multi`) flag; an explicit `--repo` pointing at a non-repository now reports
  an error instead of silently opening the home view. Process scanner gains `scan_agent_cwds`
  (machine-wide agent CWDs); home-mode diagnostics log to a machine-wide state dir.
- **`wb300 agent`** (`src/agent.rs`): a headless JSON snapshot subcommand (no TUI) for
  orchestrating agents. Emits the full repository state — worktrees with branch/workbranch/
  head/flags/status/collisions and mapped processes (incl. the live agent), workbranch groups,
  branch counts, base — under a stable `"schema": "wb300.agent.v1"` contract decoupled from the
  internal models. `"mode"` is `"repo"` (current repo) or `"home"` (machine-wide — automatic
  outside a repo, or with `--home`). stdout is pure JSON (dispatched before the panic hook so no
  terminal escape can leak; non-finite cpu sanitized; `repos` always present). `--repo` /
  `--home` / `--no-color` are now global so they work after a subcommand (e.g. `agent --home`).
