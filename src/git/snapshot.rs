//! `RepoSnapshot` — the captured Git source of truth the UI renders from.

use color_eyre::eyre::Result;

use super::commands::git_stdout;
use super::refs::{BranchInfo, FOR_EACH_REF_FORMAT, parse_refs};
use super::repo::RepoIdentity;
use super::worktree::{WorktreeRecord, parse_worktrees};

/// A point-in-time view of the repository: its worktrees and refs.
#[derive(Debug, Clone)]
pub struct RepoSnapshot {
    pub repo: RepoIdentity,
    pub worktrees: Vec<WorktreeRecord>,
    pub branches: Vec<BranchInfo>,
}

impl RepoSnapshot {
    /// Capture worktrees + refs for `repo` via the Git CLI.
    pub async fn capture(repo: RepoIdentity) -> Result<Self> {
        let root = repo.root.clone();

        let wt_bytes = git_stdout(Some(&root), &["worktree", "list", "--porcelain", "-z"]).await?;
        let worktrees = parse_worktrees(&wt_bytes);

        let fmt = format!("--format={FOR_EACH_REF_FORMAT}");
        let ref_bytes = git_stdout(
            Some(&root),
            &[
                "for-each-ref",
                "--sort=-committerdate",
                &fmt,
                "refs/heads",
                "refs/remotes",
            ],
        )
        .await?;
        let branches = parse_refs(&ref_bytes);

        Ok(Self {
            repo,
            worktrees,
            branches,
        })
    }

    pub fn local_branch_count(&self) -> usize {
        self.branches.iter().filter(|b| !b.is_remote).count()
    }

    pub fn remote_branch_count(&self) -> usize {
        self.branches.iter().filter(|b| b.is_remote).count()
    }

    /// Index of the worktree matching the launch root, if present.
    pub fn current_worktree_index(&self) -> Option<usize> {
        let root = normalize(&self.repo.root.to_string_lossy());
        self.worktrees
            .iter()
            .position(|wt| normalize(&wt.path) == root)
    }
}

/// Normalize a path string for comparison: unify separators, drop a trailing
/// slash, and lowercase on Windows (case-insensitive FS). Sufficient for
/// matching git's own output to itself; richer normalization for process→worktree
/// mapping arrives in Phase 3.
fn normalize(path: &str) -> String {
    let unified = path.replace('\\', "/");
    let trimmed = unified.trim_end_matches('/');
    if cfg!(windows) {
        trimmed.to_lowercase()
    } else {
        trimmed.to_string()
    }
}
