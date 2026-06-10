//! Per-branch lifecycle: where a branch sits in the operator's pipeline
//! (editing → uncommitted → committed → pushed → merged), derived fresh on
//! every snapshot from worktree status, upstream tracking, and hierarchy facts.

/// Where a branch sits in the work pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BranchLifecycle {
    /// Files in its worktree are being written right now — a presentation-only
    /// refinement of `Uncommitted` driven by the live watcher (see
    /// [`refine_with_activity`]); never reported by one-shot captures.
    Editing,
    /// Its checked-out worktree has uncommitted changes.
    Uncommitted,
    /// Clean, with work that hasn't reached its upstream (or no upstream yet).
    #[default]
    Committed,
    /// Clean and fully on a live upstream (nothing left to push).
    Pushed,
    /// Contained in its parent branch — merged via merge commit, fast-forward,
    /// or rebase. (A squash merge is graph-invisible: a squash-merged branch
    /// keeps reading `Committed` until it is deleted, by design.)
    Merged,
    /// Tip == parent tip and clean: the branch was cut but has no work yet.
    Fresh,
}

impl BranchLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            BranchLifecycle::Editing => "editing",
            BranchLifecycle::Uncommitted => "uncommitted",
            BranchLifecycle::Committed => "committed",
            BranchLifecycle::Pushed => "pushed",
            BranchLifecycle::Merged => "merged",
            BranchLifecycle::Fresh => "fresh",
        }
    }
}

/// Inputs to the lifecycle decision table — extracted into a plain struct so
/// the table stays a pure, exhaustively-testable function.
#[derive(Debug, Clone, Copy, Default)]
pub struct LifecycleInputs {
    /// The branch's checked-out worktree is dirty (always false when the
    /// branch has no worktree — a branch with no checkout cannot be dirty).
    pub dirty: bool,
    /// The branch has a configured upstream.
    pub has_upstream: bool,
    /// The configured upstream no longer exists (deleted on the remote).
    pub upstream_gone: bool,
    /// Commits ahead of the upstream (`None` when unknown / no upstream).
    pub ahead: Option<u32>,
    /// Commits on this branch not reachable from its parent branch.
    pub ahead_of_parent: u32,
    /// The branch tip is exactly its parent's tip.
    pub tip_equals_parent: bool,
    /// The branch has a parent in the hierarchy (trunk has none).
    pub has_parent: bool,
}

/// The decision table. First match wins:
///
/// 1. dirty worktree → `Uncommitted`
/// 2. work fully contained in the parent (and not just a fresh cut) → `Merged`
/// 3. tip == parent tip, clean → `Fresh`
/// 4. live upstream with nothing left to push → `Pushed`
/// 5. otherwise → `Committed` (covers: ahead of upstream; no upstream yet;
///    upstream deleted while unmerged work remains — a premature delete reads
///    `Committed`, never `Merged`)
pub fn derive(i: LifecycleInputs) -> BranchLifecycle {
    if i.dirty {
        return BranchLifecycle::Uncommitted;
    }
    if i.has_parent && i.ahead_of_parent == 0 && !i.tip_equals_parent {
        return BranchLifecycle::Merged;
    }
    if i.has_parent && i.tip_equals_parent {
        return BranchLifecycle::Fresh;
    }
    if i.has_upstream && !i.upstream_gone && i.ahead == Some(0) {
        return BranchLifecycle::Pushed;
    }
    BranchLifecycle::Committed
}

/// Presentation refinement: an `Uncommitted` branch whose worktree shows live
/// filesystem activity is `Editing`. The TUI calls this with the activity
/// transition state; `wb300 agent` reports the unrefined lifecycle (a one-shot
/// capture honestly cannot observe "right now").
pub fn refine_with_activity(l: BranchLifecycle, live_activity: bool) -> BranchLifecycle {
    if l == BranchLifecycle::Uncommitted && live_activity {
        BranchLifecycle::Editing
    } else {
        l
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> LifecycleInputs {
        LifecycleInputs {
            has_parent: true,
            ..Default::default()
        }
    }

    #[test]
    fn dirty_wins_over_everything() {
        let i = LifecycleInputs {
            dirty: true,
            has_upstream: true,
            ahead: Some(0),
            ahead_of_parent: 0,
            tip_equals_parent: false,
            ..base()
        };
        assert_eq!(derive(i), BranchLifecycle::Uncommitted);
    }

    #[test]
    fn contained_in_parent_is_merged() {
        let i = LifecycleInputs {
            ahead_of_parent: 0,
            tip_equals_parent: false,
            ..base()
        };
        assert_eq!(derive(i), BranchLifecycle::Merged);
    }

    #[test]
    fn merged_wins_over_pushed() {
        // A merged branch whose remote still exists and is in sync reads
        // Merged, not Pushed — merged is further down the pipeline.
        let i = LifecycleInputs {
            ahead_of_parent: 0,
            tip_equals_parent: false,
            has_upstream: true,
            ahead: Some(0),
            ..base()
        };
        assert_eq!(derive(i), BranchLifecycle::Merged);
    }

    #[test]
    fn fresh_cut_is_fresh_not_merged() {
        let i = LifecycleInputs {
            ahead_of_parent: 0,
            tip_equals_parent: true,
            ..base()
        };
        assert_eq!(derive(i), BranchLifecycle::Fresh);
    }

    #[test]
    fn clean_synced_with_live_upstream_is_pushed() {
        let i = LifecycleInputs {
            ahead_of_parent: 2,
            has_upstream: true,
            ahead: Some(0),
            ..base()
        };
        assert_eq!(derive(i), BranchLifecycle::Pushed);
    }

    #[test]
    fn ahead_of_upstream_is_committed() {
        let i = LifecycleInputs {
            ahead_of_parent: 2,
            has_upstream: true,
            ahead: Some(1),
            ..base()
        };
        assert_eq!(derive(i), BranchLifecycle::Committed);
    }

    #[test]
    fn no_upstream_with_work_is_committed() {
        let i = LifecycleInputs {
            ahead_of_parent: 3,
            ..base()
        };
        assert_eq!(derive(i), BranchLifecycle::Committed);
    }

    #[test]
    fn premature_remote_delete_is_committed_not_merged() {
        // Upstream deleted but unmerged work remains: never call it Merged.
        let i = LifecycleInputs {
            ahead_of_parent: 2,
            has_upstream: true,
            upstream_gone: true,
            ..base()
        };
        assert_eq!(derive(i), BranchLifecycle::Committed);
    }

    #[test]
    fn gone_upstream_never_reads_pushed() {
        let i = LifecycleInputs {
            ahead_of_parent: 1,
            has_upstream: true,
            upstream_gone: true,
            ahead: Some(0),
            ..base()
        };
        assert_eq!(derive(i), BranchLifecycle::Committed);
    }

    #[test]
    fn trunk_without_parent_never_reads_merged_or_fresh() {
        let synced = LifecycleInputs {
            has_parent: false,
            has_upstream: true,
            ahead: Some(0),
            ..Default::default()
        };
        assert_eq!(derive(synced), BranchLifecycle::Pushed);
        let local_only = LifecycleInputs {
            has_parent: false,
            ..Default::default()
        };
        assert_eq!(derive(local_only), BranchLifecycle::Committed);
    }

    #[test]
    fn activity_refines_only_uncommitted() {
        assert_eq!(
            refine_with_activity(BranchLifecycle::Uncommitted, true),
            BranchLifecycle::Editing
        );
        assert_eq!(
            refine_with_activity(BranchLifecycle::Uncommitted, false),
            BranchLifecycle::Uncommitted
        );
        assert_eq!(
            refine_with_activity(BranchLifecycle::Pushed, true),
            BranchLifecycle::Pushed
        );
    }
}
