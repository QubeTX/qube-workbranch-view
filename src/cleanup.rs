//! Cleanup readiness scoring (handoff §17.6–17.7): is a worktree safe to remove?

use serde::Serialize;

use crate::git::WorktreeRecord;

/// How safe a worktree is to clean up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupState {
    /// Main or current worktree — never offered for removal.
    Protected,
    /// A process/agent is running inside it.
    Active,
    /// Has uncommitted changes (removable, but only with a rescue snapshot).
    Dirty,
    /// Clean but has unpushed commits — don't lose them silently.
    Caution,
    /// Clean, idle, nothing unpushed — safe to remove.
    Safe,
}

impl CleanupState {
    pub fn glyph(self) -> &'static str {
        match self {
            CleanupState::Safe => "✓",
            CleanupState::Caution | CleanupState::Dirty => "!",
            CleanupState::Active => "✗",
            CleanupState::Protected => "•",
        }
    }

    /// Whether `x` (remove) may be offered for this worktree.
    pub fn removable(self) -> bool {
        matches!(
            self,
            CleanupState::Safe | CleanupState::Caution | CleanupState::Dirty
        )
    }

    /// Sort key: safest-to-remove first.
    pub fn rank(self) -> u8 {
        match self {
            CleanupState::Safe => 0,
            CleanupState::Caution => 1,
            CleanupState::Dirty => 2,
            CleanupState::Active => 3,
            CleanupState::Protected => 4,
        }
    }
}

/// Assess a worktree's cleanup state and a one-line reason.
pub fn assess(wt: &WorktreeRecord, protected: bool, has_agent: bool) -> (CleanupState, String) {
    if protected {
        return (
            CleanupState::Protected,
            "main / current worktree".to_string(),
        );
    }
    if has_agent {
        return (
            CleanupState::Active,
            "a process is running inside it".to_string(),
        );
    }
    let Some(status) = wt.status.as_ref() else {
        return (CleanupState::Caution, "status unavailable".to_string());
    };
    if !status.clean {
        let n = status.staged + status.unstaged + status.untracked + status.conflicted;
        return (CleanupState::Dirty, format!("{n} uncommitted change(s)"));
    }
    let ahead = status.ahead.unwrap_or(0);
    if ahead > 0 {
        return (
            CleanupState::Caution,
            format!("clean, but {ahead} unpushed commit(s)"),
        );
    }
    if wt.prunable.is_some() {
        return (CleanupState::Safe, "prunable (worktree gone)".to_string());
    }
    if status.upstream_gone {
        return (
            CleanupState::Safe,
            "clean; upstream branch gone".to_string(),
        );
    }
    (
        CleanupState::Safe,
        "clean, idle, nothing unpushed".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::WorktreeStatus;

    fn wt(status: Option<WorktreeStatus>) -> WorktreeRecord {
        WorktreeRecord {
            path: "/r".into(),
            status,
            ..Default::default()
        }
    }

    #[test]
    fn protected_and_active_take_precedence() {
        assert_eq!(assess(&wt(None), true, false).0, CleanupState::Protected);
        assert_eq!(assess(&wt(None), false, true).0, CleanupState::Active);
    }

    #[test]
    fn dirty_clean_and_unpushed() {
        let dirty = WorktreeStatus {
            unstaged: 2,
            ..Default::default()
        };
        assert_eq!(
            assess(&wt(Some(dirty)), false, false).0,
            CleanupState::Dirty
        );

        let clean = WorktreeStatus {
            clean: true,
            ..Default::default()
        };
        assert_eq!(assess(&wt(Some(clean)), false, false).0, CleanupState::Safe);

        let unpushed = WorktreeStatus {
            clean: true,
            ahead: Some(3),
            ..Default::default()
        };
        assert_eq!(
            assess(&wt(Some(unpushed)), false, false).0,
            CleanupState::Caution
        );
    }
}
