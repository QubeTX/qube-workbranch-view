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
- Each worktree now shows its real status at a glance: how many files are staged, changed, or
  untracked, whether it's ahead of or behind its upstream, and whether its upstream branch has
  disappeared — plus a details panel for the one you've selected. Press r to refresh.
- WB-300 now shows which agent is working where. It scans running programs, works out which
  worktree each one is in, and labels them (Claude/Codex agents, build and test tasks, shells,
  editors). The Processes tab lists them with CPU, memory, and how long they've been running;
  each worktree shows a green "● claude pid 1234" marker when an agent is live there; and the
  overview counts how many worktrees are actively being worked on.
