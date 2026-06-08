# CLAUDE.md — wb300

Guidance for Claude Code (and any agent) working in this repository.

## Project overview

WB-300 (`wb300`) is a cross-platform Rust **TUI operator console** for supervising many
parallel coding-agent workspaces through Git worktrees. It runs inside a Git repo and shows
branches, worktrees, dirty/clean status, the OS processes running inside each worktree
(Claude / Codex / builds / tests), changed-file collisions, remote drift, and safe-cleanup
candidates. Run outside a repo, it opens a machine-wide view of every repo being worked on.

- **Crate + binary:** `wb300` (lowercase, no hyphen).
- **Stack:** Rust (edition 2024) + Ratatui + Crossterm + Tokio + notify + sysinfo, driving the
  installed `git` CLI via async subprocesses (no `git2`/`gix` in v1).
- **Part of the QubeTX line** (TR-300, ND-300, SD-300). License, packaging, and the
  build/deploy cycle mirror TR-300 (see "Deploy a new version" below).
- **Design source of truth:** `docs/WB-300_HANDOFF_PLAN.md`.

## Architecture (the one rule that matters)

```txt
KeyEvent → resolve_key → Action → AppState::apply (reducer)
        → async Git/Process/FS task → RepoSnapshot / LiveEvent
        → UI transition → Ratatui render
```

- **UI never runs Git or mutates state directly.** Input resolves to an `Action`; the reducer
  (`app::AppState::apply`) is the single place state changes; async tasks produce snapshots and
  events that feed the reducer.
- **The UI thread never blocks on Git.** Git runs on Tokio with a small concurrency cap.
- **Git is truth; filesystem/process/remote signals are hints.** `RepoSnapshot` is canonical;
  UI/tree state is derived.
- **NUL-safe parsing everywhere** (`-z` / porcelain formats).
- **Terminal restoration is unconditional** — `terminal::TerminalGuard` (RAII) + the panic hook
  from `terminal::install_panic_hook` restore raw mode / the alternate screen on every exit.

Module layout: `app/` (state, action, reducer), `terminal/` (guard + panic hook), `ui/`
(render + theme), and — added phase by phase — `git/`, `live/`, `process/`, `config/`,
`storage/`, `update.rs`.

## Build & run

```sh
cargo run                                   # launch the TUI in the current repo
cargo run -- --repo /path/to/repo
cargo run -- --no-alt-screen                # fallback renderer
cargo build --release                       # release binary (also regenerates man/wb300.1)
cargo test                                  # unit + integration tests
cargo clippy --all-targets --workspace -- -D warnings
cargo fmt --all -- --check
```

MSRV is pinned to **1.95** in both `Cargo.toml` (`rust-version`) and `rust-toolchain.toml` —
**bump both together**, never one alone.

## Changelog rule (lockstep)

Two changelogs, always updated in the **same commit**:

- `CHANGELOG.md` — technical, Keep a Changelog format (versions, file paths, flags).
- `HUMAN_CHANGELOG.md` — plain English, no version numbers / file paths / jargon: *what changed
  and why it matters*. Keep user-facing command/flag names; strip everything else.

Never let one drift ahead of the other.

## Deploy a new version

The release model is **bump version → merge → push tag** (tag-triggered, cargo-dist-native, no
accidental deploys). The full cycle:

```txt
edit on workbranch → PR → CI green → merge to main (crates-publish runs)
                   → push vX.Y.Z tag → release.yml → windows-installers.yml
```

When the operator says *"deploy a new version"*:

1. **Bump the version.** `[package] version` in `Cargo.toml` is the single source; refresh
   `Cargo.lock`. Tag is `vX.Y.Z`. SemVer: patch = fixes, minor = features, major = breaking.
   (MSRV in `Cargo.toml` + `rust-toolchain.toml` moves only on a toolchain bump, not per release.)
2. **Update both changelogs** (`CHANGELOG.md` + `HUMAN_CHANGELOG.md`) in the same commit; update
   README/docs if user-visible.
3. **Verify locally:** `cargo fmt --all -- --check`, `cargo test`, `cargo clippy --all-targets
   --workspace -- -D warnings`, `cargo publish --dry-run --locked`, `cargo package --list --locked`.
4. **Commit specific files** (never `git add -A`) with a conventional message + the
   `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` trailer; open the PR;
   **merge to `main`** (the gate). CI runs; `crates-publish.yml` publishes to crates.io when the
   version is new (idempotent — no-ops if already published).
5. **Push the tag:** `git tag vX.Y.Z && git push origin vX.Y.Z` → fires `release.yml`
   (cargo-dist builds the 6-target artifacts + GitHub Release) → `windows-installers.yml` attaches
   the Windows add-on installers.
6. **Watch CI via Monitor** — never foreground `gh run watch` (it burns context).
7. **Verify:** `git ls-remote --tags origin | grep vX.Y.Z`, `gh release view vX.Y.Z`,
   `cargo install wb300 --force`, `wb300 update`.
8. **Fix-forward on failure:** fix the root cause, **bump the patch version** (never re-tag an
   existing version, never force-push `main`), re-run.

Once Phase 9 lands, `./deploy.sh bump <patch|minor|major|X.Y.Z>` then `./deploy.sh tag`
automate steps 1–5.

### Downstream homepage sync

The end-user marketing page lives in the sibling repo **`qube-machine-report-homepage`**
(find it by name; don't hardcode a machine path). After a deploy, it normally needs **no**
change: its version badge is fetched live (`useGitHubVersion('QubeTX/qube-workbranch-view', …)`)
and its install links point at `releases/latest/download/…` — both version-agnostic. Update the
homepage **only** when something **outward-facing** changes: new features, new/changed install
methods, renamed commands/flags, a new tagline, or new screenshots.

## Windows distribution (Phase 9 — in progress)

Four first-class installers (Global / Corporate × MSI / EXE) via `wix/` + `wix-corporate/` +
`inno/`, plus a registry-aware `wb300 update`. **Lockstep contract:** the install paths and
`HKCU\Software\WB300\InstallSource` marker values in `wix/`, `wix-corporate/`, and `inno/` must
change together with `src/update.rs::detect_install_origin()` in the same commit. Product GUIDs
are **permanent** — regenerating them breaks in-place upgrades.

## Git workflow

This repo follows the team git-workflow: worktrees-always, a daily `<dev>/wb-<date>` workbranch,
task worktrees off it, gated PRs at both merge moments. No direct pushes to `main`.
