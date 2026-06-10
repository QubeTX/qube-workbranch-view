# Human Changelog

A plain-English companion to `CHANGELOG.md` — what changed and why it matters, without version
numbers, file paths, or jargon. (Agents: keep this in lockstep with `CHANGELOG.md`.)

## Unreleased

## The board now shows your branches the way Git actually works

We discovered the original design rested on a misunderstanding: we'd assumed one branch could
live in many working folders at once. In reality, Git allows each branch to be checked out in
exactly one folder — so the true picture of a fleet of agents is a *family tree of branches*:
your main line, today's working branch off it, and one task branch per agent. This release
rebuilds WB-300 around that truth.

- **The main view is now that family tree.** Your main line at the top, today's working branch
  beside it, and each agent's task branch nested underneath — with the agent's name on its
  branch, the folder it lives in, and the exact files it's changing right now, expandable like
  a file explorer. The tree is read from Git's actual history, not guessed from branch names.
- **Every branch shows where its work stands** at a glance: being edited right now → has
  unsaved-to-history changes (yellow) → committed but not yet sent (↑) → safely on the server
  (✓) → folded into its parent (done). You can also see when a branch has fallen behind its
  parent and needs to catch up.
- **By default you only see what matters now** — branches with an agent, a folder, or
  unfinished work. One key reveals everything else, dimmed.
- **WB-300 can now tap you on the shoulder.** It sends a real system notification when a branch
  gets new commits, when work reaches the server, and when two branches start changing the same
  file — and never for anything else. A burst of activity becomes one notification ("3 branches
  pushed"), repeats are quieted, and you can turn any of it off with a flag or a small settings
  file.
- **The whole-machine view is now one tree too**: every active project as a top-level entry,
  every branch and agent beneath it — truly one window over the entire fleet. Press Enter on a
  project to step inside it.
- **A real manual, built in.** `wb300 help` prints the complete guide — what every view, color,
  symbol, and key means — right in your terminal.
- **A clean way out.** `wb300 uninstall` removes WB-300 properly no matter how you installed
  it, asks before doing anything, and can optionally clear its settings and history too. It
  never touches your projects.
- The machine-readable snapshot other AI agents consume was upgraded to carry the real branch
  tree; anything reading the old format will need the new one (the format is clearly labeled).

## Shut down a runaway agent, and a clearer live picture

This release is about seeing the truth and acting on it:

- **You can now shut down a forgotten or stuck agent with a keystroke.** Select a worktree and
  press a key, confirm by typing the process number, and WB-300 ends that agent's process. You
  can also do it from the process list for any program. It always asks first, shows you exactly
  what you're about to close (name, number, folder), refuses to close itself, and tells you on
  screen whether it worked or why it didn't. WB-300 never closes anything on its own — only when
  you ask.
- **WB-300 stopped mistaking the Claude desktop app for a working agent.** It now only counts the
  real coding-agent programs, so "which branches actually have an agent on them" is trustworthy.
- **The board reads more clearly.** A worktree's whole line turns yellow the entire time it has
  unsaved-to-history changes; a small dot shows the file being written right now; the row flashes
  magenta the moment it commits and green the moment it pushes (and if you commit and push
  back-to-back, it flashes magenta then green). The home screen overview now tells you how many
  worktrees are being edited right now and how fresh the information is — and warns you plainly if
  it ever can't read the latest, instead of pretending everything's current.
- **"Collisions" is now "Merge Risk"** and says what it really means: which files have been
  changed on more than one branch and will likely clash when those branches are merged back
  together — with the agent on each side shown. (Two agents editing the same-named file in two
  separate worktrees aren't actually fighting — each has its own copy — but they will conflict at
  merge time, and that's what this now forecasts.) New conflicts are also recorded to the history.
- We made sure you can never accidentally remove your main worktree.

## Watch your agents work, live

WB-300 now shows you what's happening across your worktrees as it happens, so you can glance at
the board and feel the machine breathing:

- **A blue mark pulses on a worktree while a file inside it is being saved** — and goes dark
  within about half a second of the writing stopping. With a swarm of agents each editing
  different files, you can literally see where the activity is and watch it hop around.
- **A worktree's name stays yellow the whole time it has uncommitted changes**, so you can tell
  at a glance which ones have work that hasn't been saved to history yet. (If WB-300 can't read a
  worktree's status, its name goes grey instead of pretending it's clean.)
- **When a worktree commits, its whole row flashes magenta; when it pushes, the row flashes
  green** — a clear, obvious "something just shipped here" — then settles back to normal.
- These live signals now show up on the worktree you've got selected too, not just the others.
- The same activity, uncommitted, commit, and push signals work in the machine-wide view, so one
  window shows you the whole fleet breathing at once.

We also made the dashboard more honest about its own health: if it ever can't read Git, it now
shows a "stale" warning in the header instead of quietly leaving old information on screen
looking live, and problems reading a single worktree (or a whole repo, in the machine-wide view)
are recorded to the log instead of vanishing.

## First release

Everything below is in the first public version of WB-300.

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
