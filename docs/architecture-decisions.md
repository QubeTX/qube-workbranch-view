# Architecture decisions

Long-form rationale for the load-bearing choices. The full product design is in
`WB-300_HANDOFF_PLAN.md`; this file records *why* the major decisions were made and what was
rejected.

## Language & TUI stack: Rust + Ratatui + Crossterm + Tokio

WB-300 is filesystem-, process-, and Git-heavy. Rust gives native binaries, fast startup, and
robust path/process handling; Ratatui is the modern Rust immediate-mode TUI; Crossterm is the
cross-platform terminal backend (raw mode, alternate screen, keyboard/mouse/resize, restore);
Tokio lets Git subprocesses, filesystem events, process scans, remote polls, and input run
without blocking the render loop. TypeScript/Ink was considered and rejected — it's a weaker fit
for Git/process/filesystem orchestration.

## Git via the installed CLI, not git2/gix (v1)

We shell out to the user's `git` binary through async subprocesses. This respects their actual
config, credential helpers, SSH setup, corporate config, and Git-for-Windows quirks, and keeps
behavior identical to the command line they already trust. `git2`/`gix` are possible later if a
specific subprocess path becomes a measured bottleneck. All parsing uses NUL-delimited
(`-z` / porcelain) formats.

## State model: snapshot is truth, UI is derived

`RepoSnapshot` (built from Git) is canonical. Filesystem/process/remote events are *hints* that
trigger refreshes; they never decide final state. Input resolves to an `Action`, and the reducer
(`AppState::apply`) is the single place state changes — so the live engine and command palette
can feed the same path. UI widgets never run Git or mutate state.

## Edition 2024

WB-300 uses Rust edition 2024 (the latest). Editions are per-crate language epochs, not
toolchain or dependency constraints — 2024 only requires Rust ≥ 1.85, and our MSRV is 1.95.
The build/deploy cycle (cargo-dist, CI, crates.io, the Windows installers) is edition-agnostic,
so nothing about distribution changes. The sibling tools (TR-300/ND-300) remain on edition 2021;
editions don't affect interop, so the divergence is harmless.

## Distribution: full TR-300 parity

License (PolyForm Noncommercial 1.0.0, committed `LICENSE`), packaging (cargo-dist 0.31.0 across
six targets), the four-workflow CI/build/deploy cycle, the four-installer Windows matrix, and the
registry-aware self-update all mirror TR-300 exactly, so the whole QubeTX line behaves
identically. The single deliberate divergence is a `deploy.sh` wrapper (TR-300 deploys via the
manual CLAUDE.md procedure; WB-300 keeps the same cycle but scripts it).

## Release model: tag-triggered (bump → merge → tag)

`release.yml` fires only on a pushed `vX.Y.Z` tag; `crates-publish.yml` publishes from a green
CI run on `main`. Deploying is deliberate (no accidental release on every push to `main`), which
is why the trigger is a tag rather than a branch push.

## v2: the branch tree is the product (2026-06-10)

v1 was worktree-first with a name-prefix "workbranch" grouping; that encoded a wrong mental
model (one branch ↔ many worktrees). Git enforces one checkout per branch, so v2 makes the
branch hierarchy the primary view and derives it from commit topology — one batched,
fingerprint-cached `rev-list` over the off-trunk history, nearest-strict-ancestor parentage,
with naming conventions only breaking genuine ties. Rejected alternatives: pure name heuristics
(wrong, the v1 mistake), per-branch `merge-base` pairs (O(B²) subprocesses), and recording
parentage at branch-creation time (wb300 doesn't create branches and can't see other tools').
Repos that follow no convention degrade to a flat list rather than failing.

## v2: OS toasts only, for exactly three events

The operator chose native notifications over an in-TUI alert area: the point of a tap on the
shoulder is being heard when the terminal is NOT focused. Scope is deliberately tight — commit,
push, merge-conflict risk; never agent-exit or idle — to keep toasts meaning something. Policy
(coalescing, cooldown, gating) is a pure, tested struct; the backend (notify-rust; zbus on
Linux to avoid the libdbus link; a self-registered HKCU AppUserModelID on Windows) fails soft:
log once, disable for the session, never block the UI. Installer-owned AUMID + icon is a
recorded TODO because it touches the four-installer lockstep contract.

## v2: unified home tree, mutations stay behind drill-in

The machine-wide view renders the same tree widget with repo nodes at the root (one window,
whole fleet) — but `x`/`K`/`f`/`p` remain in the drilled-in per-repo view. Replicating the
PendingGit/Kill/event-store machinery multi-repo was judged real risk for marginal gain; the
home view observes, the per-repo view acts.

## v2: clean break to agent schema v2

`wb300.agent.v1`'s `workbranches` grouping reproduced the wrong model, so v2 replaces it with
the real hierarchy (flat, depth-first, parent pointers — trivially reconstructable as a tree
and easier to evolve than nesting). A clean break (major version bump, schema tag) was chosen
over emitting both schemas: the only known consumer is the operator's own tooling, and dual
output would have doubled the golden-test surface for no consumer benefit.
