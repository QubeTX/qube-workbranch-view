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
- WB-300 now updates itself live. Save a file in a worktree and its status updates on its own —
  no need to press refresh — and the worktree briefly flashes to show it just changed. It also
  re-checks periodically as a safety net, and all of this runs in the background so the
  dashboard never freezes while Git is working. (Live watching can be turned off with a flag.)
- Made the live file-watching a good citizen of your system: it only watches your source files
  (not big folders like node_modules or build output) and caps how much it watches, so it
  won't interfere with other apps' file watching. The header shows whether you're in live,
  periodic-only, or static mode, and a hiccup while reading Git can no longer freeze the
  live updates.
- WB-300 now keeps a timeline. When worktrees are created or removed — even from another
  terminal — it notices, flashes the change, and records it to a history you can browse in the
  new Timeline tab; that history survives restarts. It's kept tidy automatically (trimmed by
  age and size), written safely in the background, and any problems are logged to a file
  instead of vanishing.
- WB-300 now warns you where agents are about to clash. It compares which files each worktree
  has changed — including files already committed since the shared branch — and flags any file
  two or more worktrees have touched, color-coded by how dangerous it is (lockfiles, database
  migrations, and CI configs are the scariest). There's a new Collisions tab and a ⚠ marker
  next to any worktree caught in one. It also does this work faster by checking worktrees in
  parallel.
