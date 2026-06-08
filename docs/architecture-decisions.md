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
