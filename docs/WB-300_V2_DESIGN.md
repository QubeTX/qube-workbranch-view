# WB-300 v2 design — the corrected model

*2026-06-10. This document is the design source of truth for WB-300 ≥ 2.0.0. The original
`WB-300_HANDOFF_PLAN.md` remains as the v1 historical record; where they conflict, this
document wins.*

## Why v2 exists

v1 was conceived under a wrong mental model: that one branch can host many simultaneous
worktrees, letting multiple agents share a branch. Git's actual rule: **a branch is checked
out in at most one worktree** (`fatal: '<branch>' is already used by worktree …`). The correct
operating model — the one the team git-workflow encodes — is:

```
main (trunk)
 └── <dev>/wb-<YYYY-MM-DD>        daily workbranch (≤ 24h, rebases onto main)
      ├── feat/<desc>-<id>        task branch — its own worktree, one agent (≤ 2d)
      └── fix/<desc>-<id>
```

**One agent = one branch = one worktree.** The branch tree is the work; worktrees and agents
are attributes of branches. v2 rebuilds the product around that: the v1 data layer was already
1:1-correct (`WorktreeRecord.branch`, `BranchInfo.worktree_path`), but the UI was
worktree-first and the "workbranch" grouping was a name-prefix heuristic
(`feat/x` grouped under "feat"). Both are gone.

## The hierarchy (src/git/hierarchy.rs)

- **Inputs:** the existing ref scan (`for-each-ref`, now also capturing
  `%(upstream:track,nobracket)` for every branch and the `origin/HEAD` symref) plus one
  batched topology call: `git rev-list --topo-order --max-count=10000 --format=%H%x00%P
  <local-tip-oids…> --not <boundary>` where the boundary is the remote trunk tip (continuous
  rebase keeps the off-trunk graph tiny). The `commit <oid>` header lines rev-list emits with
  `--format` carry no NUL and are filtered, so no newer-git flags are needed.
- **Parent selection** per non-trunk branch, on memoized off-trunk reach-sets:
  1. nearest strict ancestor among the other local branches (stacked branches nest);
  2. equal-tip ties broken ONLY by convention (`wb-` last segment is the parent; two
     indistinguishable twins both fall to trunk — never guess);
  3. diverged (un-rebased task whose workbranch advanced): the wb-named branch is the parent;
     deepest shared base wins; `behind_parent > 0` is the needs-rebase signal;
  4. fallback: trunk.
- **Roles:** trunk; wb-named → workbranch; parent==trunk → standalone; else task.
- **Degradation:** no convention → standalones under trunk; no trunk → flat list; rev-list
  failure/cap → `approximate = true`. Never an error.
- **Caching:** a module-level map keyed by normalized common git dir; fingerprint = hash of
  every ref's (name, oid) + trunk identity, so any commit/rebase/fetch invalidates and a
  nothing-changed poll costs zero extra git processes. Lifecycle is never cached.
- **Cost:** v1.2 capture = `2 + 4W` git processes; v2 = `2 + 2W` (+1 on cache miss, +≤16
  bounded behind-counts for trunk children) — strictly cheaper.

## Lifecycle (src/git/lifecycle.rs)

Pure decision table per branch: dirty worktree → **uncommitted**; contained in parent (and not
a fresh cut) → **merged**; tip == parent tip → **fresh**; live upstream with `ahead == 0` →
**pushed**; else **committed**. A watcher-driven presentation refinement upgrades uncommitted →
**editing**. Documented limits: a squash-merged branch reads committed until deleted
(graph-invisible); upstream-gone with unmerged work reads committed (premature delete), never
merged.

## The tree (src/app/tree.rs, src/ui/tree.rs, src/ui/branches.rs)

- Node identity (`NodeId`) is repo key + branch NAME (worktree paths churn; names persist), so
  expansion, selection, and flashes survive refreshes. `flash_key()` keys the `Transitions`
  map; the engine is unchanged from v1.
- Visual layout (operator-approved): trunk and its children render as SIBLINGS under the repo
  node (everything is off trunk; nesting the world under `main` wastes an indent), tasks nest
  under their workbranch, files nest under their branch (capped at 30 + overflow row),
  detached worktrees are leaf rows.
- Active-only by default: trunk always; else worktree ∨ agent ∨ unmerged-vs-parent ∨
  unpushed-vs-upstream ∨ active descendant (a drained workbranch stays while its tasks live).
  `a` toggles; inactive branches render dimmed.
- v1's visual language is preserved: yellow uncommitted rows, magenta/green milestone flashes,
  blue ◆ save pulses (now also per-file), ✚/⌫ created/removed.
- Tabs: Branches / Processes / Merge Risk / Cleanup / Timeline / Help (1–6). Overview's counts
  live in a header strip; the flat Worktrees tab is replaced by the tree + details pane.
- The home view is `flatten()` over all repos — one tree, repo nodes at root. Mutations stay
  in the drilled-in per-repo view (the PendingGit/Kill machinery is single-repo by design).

## Notifications (src/notifications.rs, src/config.rs)

OS toasts ONLY, for exactly: branch committed (rebase-suppressed), branch pushed, new
merge-conflict risk. Never agent-exit or idle. Reducers stay pure: ingest emits archived
events; the event loops map them to `NotifyEvent`s and `try_send` (drop on backpressure); one
notifier task owns policy (1.5s coalescing, per-(repo,branch,kind) cooldown, per-kind config
gating) and calls the backend via `spawn_blocking`, disabling itself for the session on
failure. Windows AUMID `QubeTX.WB300` is self-registered under HKCU with an untagged fallback.
Config: minimal TOML (`[notifications]`), missing → defaults, malformed → warn + defaults.
`--no-notify` forces off.

**Deferred (recorded TODO):** installer-owned AUMID registration + a toast icon — touches the
four-installer lockstep contract (`wix/main.wxs`, `wix-corporate/corporate.wxs`,
`inno/*.iss` ⇄ `src/update.rs`); do it as one deliberate change. Runtime registration keeps
cargo/shell installs covered meanwhile.

## CLI

- `wb300 help` — the full manual (src/help.rs), paged on a TTY; clap's stub help subcommand is
  disabled. The manual is the canonical legend; keep it in sync with UI changes.
- `wb300 uninstall` — channel-aware (reuses `update.rs::detect_install_origin`): MSI/EXE via
  the Add/Remove registry entry launched detached; cargo direct (Unix) or delayed-detached
  (Windows); binary unlink (Unix). `--yes` / `--purge` / `--json`. Never touches repositories.
- `wb300 agent` — schema **wb300.agent.v2**: per-repo `trunk`, `hierarchy_approximate`, and
  `branches` (depth-first, parent pointers, role, lifecycle, ahead/behind vs parent and
  upstream, active, worktree path, agent, files ≤50 + `files_total`), plus the unchanged
  path-level `worktrees`/`collisions` views.

## Events

`events.jsonl` gains `branch_committed` / `branch_pushed` / `branch_merged` kinds and optional
`parent`/`repo`/`files` fields; old lines deserialize unchanged (serde defaults).

## Invariants carried forward from v1

KeyEvent → Action → reducer → async task → snapshot → render; the UI never runs Git and never
blocks; Git is truth, FS/process/remote are hints; NUL-safe parsing; unconditional terminal
restoration; no automatic network or destructive actions; changelog lockstep; man page
regenerated+committed with any CLI change; MSRV pinned in two places; installer GUIDs
permanent.
