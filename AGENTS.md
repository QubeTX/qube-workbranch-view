# AGENTS.md — wb300

Agent-facing tour and release checklist. Full guidance lives in `CLAUDE.md`; the design source
of truth is `docs/WB-300_HANDOFF_PLAN.md`.

## Orient

- `wb300` is a Rust + Ratatui TUI for supervising parallel coding-agent Git worktrees.
- Architecture: `KeyEvent → Action → AppState::apply (reducer) → async Git/FS/process task →
  RepoSnapshot/LiveEvent → render`. The UI never runs Git or mutates state directly; the
  reducer is the single mutation point; the UI thread never blocks on Git.
- Git is the source of truth; filesystem/process/remote signals are hints. Parse with NUL-safe
  formats. Terminal restoration is unconditional (`terminal::TerminalGuard` + panic hook).

## Before you commit

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cargo test`
- Update `CHANGELOG.md` **and** `HUMAN_CHANGELOG.md` together.
- Stage specific files (never `git add -A`). Conventional Commit message + the
  `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` trailer.

## Release checklist

See `CLAUDE.md` → "Deploy a new version". Summary: bump `Cargo.toml` version → update both
changelogs → local verify → PR → merge to `main` (crates-publish runs) → push `vX.Y.Z` tag
(release.yml + windows-installers.yml run) → verify → fix-forward (bump patch, never re-tag).

## Git workflow

Worktrees-always; daily `<dev>/wb-<date>` workbranch; task worktrees off it; gated PRs; no
direct pushes to `main`.
