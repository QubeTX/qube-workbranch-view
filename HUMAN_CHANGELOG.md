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
- Press f to fetch from your remotes — WB-300 never reaches out to the network on its own. The
  header shows how long ago the remote was last checked, and a worktree flashes green the
  moment its commits make it to the remote.
- WB-300 can now help you tidy up — safely. A new Cleanup tab shows which worktrees are safe to
  remove, which have unsaved work, and which are busy. Press / to filter the list, : for a
  command menu, and x to remove the selected worktree — but only after you type its name to
  confirm. If it has uncommitted changes, WB-300 saves a rescue copy first, and it will NOT
  delete anything if saving that rescue fails. Cleaning up leftover bookkeeping also asks first.
- WB-300 now installs the same way everywhere the rest of the toolkit does. On Mac and Linux
  there's a one-line installer; on Windows there are four ready-to-run installers — a regular
  one that needs admin and a "corporate" one that installs just for you with no admin rights,
  each available as a classic MSI or a setup EXE. You can also get it from Cargo or build it
  from source. Pick whichever fits your machine.
- `wb300 update` now upgrades WB-300 in place. On Windows it remembers how you installed it and
  fetches the matching installer, checks the download is genuine before running it, and confirms
  the new version actually took — so the no-admin "corporate" install upgrades without ever
  asking for admin. Add `--json` if a script or agent needs to read the result.
- Releasing a new version is now a single command for maintainers, split into a safe
  "prepare" step and a "ship" step, with built-in guards so a release can't go out with mismatched
  notes or before its checks pass.
- Run WB-300 from outside any project — like your home folder — and it now opens a
  machine-wide control tower instead of just saying "not a repo." It finds every project that's
  being actively worked on right now (anywhere an agent like Claude or Codex is running, plus
  projects you've set up with multiple worktrees), and shows them side by side: which agent is
  in which workspace, what's changed, and the same live colour flashes as the single-project
  view — so you can watch several agents across several projects at a glance. Pick a project and
  press Enter to drop into its full view, or Esc to come back. You can also force this view from
  inside a project with `--home`. It only ever looks at a sensible, capped set of folders, so it
  stays light on your machine.
- New `wb300 agent` command: instead of opening the dashboard, it prints everything WB-300 knows
  about your project as plain JSON and exits. That lets another AI assistant or a script ask
  WB-300 for an instant, machine-readable picture — which branches and worktrees exist, which
  agent is working where, and where things are about to collide — without a person watching a
  screen. Add `--home` to get the same for every active project on the machine at once.
