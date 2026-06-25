# CLAUDE.md — wb300

Guidance for Claude Code (and any agent) working in this repository.

## Project overview

WB-300 (`wb300`) is a cross-platform Rust **TUI operator console** for supervising parallel
coding-agent work in Git. **The model (v2): one agent = one branch = one worktree** — Git
allows a branch to be checked out in at most ONE worktree, so the primary view is the real
**branch hierarchy** (trunk → daily workbranch → task branches, derived from commit topology,
never from name prefixes), with each branch carrying its lifecycle stage
(editing/uncommitted/committed/pushed/merged), its agent process, its ⌂ worktree, and the
files in flight. It also maps OS processes (Claude / Codex / builds / tests), forecasts
merge-conflict risk, records a timeline, sends OS toast notifications (commit / push /
conflict-risk only), and scores safe cleanup. Run outside a repo, it shows one unified tree
over every active repository on the machine.

- **Crate + binary:** `wb300` (lowercase, no hyphen).
- **Stack:** Rust (edition 2024) + Ratatui + Crossterm + Tokio + notify + sysinfo +
  notify-rust (toasts) + toml (config), driving the installed `git` CLI via async
  subprocesses (no `git2`/`gix`).
- **Part of the QubeTX line** (TR-300, ND-300, SD-300). License, packaging, and the
  build/deploy cycle mirror TR-300 (see "Deploy a new version" below).
- **Design source of truth:** `docs/WB-300_V2_DESIGN.md` (the corrected model).
  `docs/WB-300_HANDOFF_PLAN.md` is the v1 historical record — where they conflict, V2 wins.

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

Module layout (`src/`, all declared in `lib.rs`): `app/` (state, action, reducer, `tree.rs` —
tree state + flatten), `git/` (snapshot pipeline, `hierarchy.rs` — topology-derived branch
tree, `lifecycle.rs` — the per-branch stage table), `home/` (multi-repo discovery + snapshot
behind the machine-wide view), `ui/` (per-tab modules: `branches`/`tree` render the hierarchy;
`processes`, `merge_risk`, `cleanup`, `timeline`, `overlays`, `home`), `live/` (watcher +
debouncer), `process/` (scan + classify), `storage/` (events.jsonl), `agent.rs` (the
`wb300 agent` headless JSON snapshot — the `wb300.agent.v2` contract), `collision.rs`
(merge-conflict forecasting) and `cleanup.rs` (safe-cleanup scoring) — the engines the
`merge_risk`/`cleanup` UI tabs render, `notifications.rs` (toast policy + backends),
`config.rs` (minimal TOML), `cli.rs` (clap surface; `build.rs` `include!`s it for the man
page), `help.rs` (the `wb300 help` manual — keep it in sync with UI changes), `uninstall.rs`,
`update.rs`, `util/`, `terminal/` (guard + panic hook).

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

When scripting pass/fail of `cargo fmt`/`clippy`/`test` in bash, **don't pipe to `tail`/`grep`** —
the pipe masks the exit status (a green-looking `&&` chain can hide a `clippy -D warnings`
failure). Read `${PIPESTATUS[0]}` or run the command unpiped.

MSRV is pinned to **1.95** in both `Cargo.toml` (`rust-version`) and `rust-toolchain.toml` —
**bump both together**, never one alone.

### Subcommands & headless mode

`wb300` (no subcommand) opens the TUI; four subcommands run headless and exit:

```sh
wb300 agent              # print repo (or machine-wide) state as JSON and exit — no TUI
wb300 help               # the full manual (src/help.rs), NOT clap's usage dump
wb300 update [--json]    # registry-aware self-update (see "Windows distribution")
wb300 uninstall [-y] [--purge] [--json]
```

`wb300 agent` (`src/agent.rs`) emits the **`wb300.agent.v2`** schema — a *stable* JSON contract
that orchestrating agents and the downstream `wb300` skill consume. Treat it like the installer
lockstep: adding fields is fine, but a breaking shape change must bump the schema (`v2` → `v3`).
The global flags `--repo`, `--home`/`--multi`, and `--no-color` are accepted after a subcommand
too; `--no-live` opens the TUI in static-snapshot mode (no live engine).

## Changelog rule (lockstep)

Two changelogs, always updated in the **same commit**:

- `CHANGELOG.md` — technical, Keep a Changelog format (versions, file paths, flags).
- `HUMAN_CHANGELOG.md` — plain English, no version numbers / file paths / jargon: *what changed
  and why it matters*. Keep user-facing command/flag names; strip everything else.

Never let one drift ahead of the other.

## Deploy a new version

### Release prerequisites & gotchas (learned at v1.0.0–v2.0.0)

- `crates-publish.yml` needs the **`CARGO_REGISTRY_TOKEN`** repo secret; without it the publish
  job fails fast. The tag-triggered GitHub release (`release.yml`) is unaffected by its absence.
- **`man/wb300.1` is committed and generated by `build.rs` from `src/cli.rs`.** Regenerate +
  commit it (`cargo build`) in the same commit whenever the CLI surface (subcommands/flags)
  changes, or CI's `cargo publish --dry-run --locked` fails on a dirty tree (build.rs rewrites
  it). Output is byte-deterministic (no embedded date, LF-normalized) so a Windows-built man
  page matches Linux CI — confirm with `cargo publish --dry-run --locked` (no `--allow-dirty`).
- First release of the crate: the version is already at the target, so skip `./deploy.sh bump`
  and `./deploy.sh tag` the current green commit directly.
- `jq` is not installed on this machine — use `gh ... --jq` or PowerShell `ConvertFrom-Json`
  (an external-jq pipeline inside a CI monitor fails silently: no events, then timeout).
- A PowerShell safety hook blocks commands containing a literal `/I` — write PR bodies that
  mention MSI flags to a temp file and use `gh pr create --body-file`.
- `gh pr merge --delete-branch` can't delete branches checked out in worktrees and may exit
  nonzero even when the merge succeeded — verify with `gh pr view --json state`, then remove
  the worktree and delete the branch manually.

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

### `deploy.sh` (the canonical one-command path)

`./deploy.sh` scripts steps 1–5 in two explicit phases that respect the PR gate:

```sh
# Phase 1 — on the workbranch (bumps, guards, gates, commits, pushes):
./deploy.sh bump <patch|minor|major|X.Y.Z>
#   → bumps [package] version + refreshes Cargo.lock
#   → lockstep guard: hard-stops unless CHANGELOG.md has a `## [X.Y.Z]` section
#     AND HUMAN_CHANGELOG.md changed since the last tag
#   → runs fmt / clippy -D warnings / test --locked / publish --dry-run --locked
#   → commits ONLY Cargo.toml, Cargo.lock, CHANGELOG.md, HUMAN_CHANGELOG.md,
#     man/wb300.1 (build.rs regenerates it with the new version string)
#     (+ the Co-Authored-By trailer) and pushes the branch
# Then YOU merge the PR to main (the gate). crates-publish.yml runs.

# Phase 2 — on main, after the merge (verifies, tags, pushes the tag):
./deploy.sh tag
#   → refuses a dirty tree, a non-main branch, a divergent main, or a CI run
#     that isn't green (override with DEPLOY_SKIP_CI_CHECK=1)
#   → refuses to re-tag an existing version; never force-pushes
#   → creates + pushes vX.Y.Z → fires release.yml → windows-installers.yml
```

Prefer `deploy.sh`; the manual steps above are what it automates (run/verify either).

### Downstream homepage sync

The end-user marketing page lives in the sibling repo **`qube-machine-report-homepage`**
(find it by name; don't hardcode a machine path). After a deploy, it normally needs **no**
change: its version badge is fetched live (`useGitHubVersion('QubeTX/qube-workbranch-view', …)`)
and its install links point at `releases/latest/download/…` — both version-agnostic. Update the
homepage **only** when something **outward-facing** changes: new features, new/changed install
methods, renamed commands/flags, a new tagline, or new screenshots.

The live site is `reports.qubetx.com` (not qubetx.com); merging to its `main` auto-deploys on
Vercel. To confirm a deploy landed, compare the local `dist/assets/index-*.js` bundle hash
against the served HTML (the SPA shell carries no page content, so grep for the hash, not copy).

## Windows distribution

Four first-class installers (Global / Corporate × MSI / EXE) via `wix/main.wxs` (Global MSI,
cargo-dist-built during `release.yml`), `wix-corporate/corporate.wxs` (Corporate MSI),
`inno/global.iss` + `inno/corporate.iss` (the two EXEs) — all attached by
`.github/workflows/windows-installers.yml`, which chains off `release.yml` via `workflow_run`
and uploads the 6 add-on assets (3 installers + 3 `.sha256` sidecars). Plus a registry-aware
`wb300 update` (`src/update.rs`) that reads the install marker and self-updates via the
matching installer (SHA-256-verified) on Windows, or `cargo` / shell installer elsewhere.

**Lockstep contract:** the install paths and `HKCU\Software\WB300\InstallSource` marker values
in `wix/main.wxs`, `wix-corporate/corporate.wxs`, `inno/global.iss`, and `inno/corporate.iss`
must change **together with** `src/update.rs::detect_install_origin()` (and its
`read_install_source_marker` arms) in the same commit. `src/uninstall.rs` consumes the same
origin detection and the installers' Add/Remove entries — keep it in the same lockstep. The
four edition values are `msi-global` / `msi-corporate` / `exe-global` / `exe-corporate`.
Product GUIDs (MSI `UpgradeCode`s, Inno `AppId`s) are **permanent** — regenerating them breaks
in-place upgrades.

**Toast AUMID (future lockstep item):** notifications self-register the
`HKCU\Software\Classes\AppUserModelId\QubeTX.WB300` key at runtime
(`src/notifications.rs::register_aumid`). If/when the four installers take ownership of that
registration (plus a toast icon), change the installers AND `register_aumid` AND
`uninstall.rs::purge_windows` together — same lockstep discipline as `InstallSource`.

## Git workflow

This repo follows the team git-workflow: worktrees-always, a daily `<dev>/wb-<date>` workbranch,
task worktrees off it, gated PRs at both merge moments. No direct pushes to `main`.
