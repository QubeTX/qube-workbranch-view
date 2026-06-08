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
