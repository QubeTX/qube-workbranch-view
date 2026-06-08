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
