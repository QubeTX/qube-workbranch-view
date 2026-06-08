# Human Changelog

A plain-English companion to `CHANGELOG.md` — what changed and why it matters, without version
numbers, file paths, or jargon. (Agents: keep this in lockstep with `CHANGELOG.md`.)

## Unreleased

### Added
- The first working skeleton of WB-300. It opens a full-screen terminal dashboard with tabs
  (Overview, Worktrees, Processes, Collisions, Cleanup, Help) you can move between, and it
  always restores your terminal cleanly when you quit — even if something crashes mid-run.
- All the plumbing to ship it like the other QubeTX tools: it can be built and released for
  Mac, Windows, and Linux (both Intel and ARM), installed several ways, and published
  automatically when a new version is tagged.
- WB-300 now reads your repository. It finds the repo, lists every worktree (marking the one
  you're in and flagging detached, locked, or leftover ones), and counts your local and remote
  branches — shown live in the dashboard, and you can move through the list with j/k. Run it
  somewhere that isn't a Git repo and it tells you politely instead of crashing.
