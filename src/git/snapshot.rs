//! `RepoSnapshot` — the captured Git source of truth the UI renders from.

use color_eyre::eyre::Result;

use super::commands::git_stdout;
use super::refs::{BranchInfo, FOR_EACH_REF_FORMAT, parse_refs};
use super::repo::RepoIdentity;
use super::status::parse_status_v2;
use super::worktree::{WorktreeRecord, parse_worktrees};
use crate::process::ProcessSnapshot;
use crate::util::paths::normalize;

/// A point-in-time view of the repository: its worktrees and refs.
#[derive(Debug, Clone)]
pub struct RepoSnapshot {
    pub repo: RepoIdentity,
    pub worktrees: Vec<WorktreeRecord>,
    pub branches: Vec<BranchInfo>,
    pub processes: ProcessSnapshot,
}

impl RepoSnapshot {
    /// Capture worktrees + refs for `repo` via the Git CLI.
    pub async fn capture(repo: RepoIdentity) -> Result<Self> {
        let root = repo.root.clone();

        let wt_bytes = git_stdout(Some(&root), &["worktree", "list", "--porcelain", "-z"]).await?;
        let mut worktrees = parse_worktrees(&wt_bytes);

        // Fill per-worktree status. Sequential is fine for a manual capture; the
        // live engine (Phase 4) moves this off the UI loop and bounds concurrency.
        for wt in worktrees.iter_mut() {
            if wt.bare {
                continue;
            }
            let path = std::path::Path::new(&wt.path);
            if let Ok(bytes) =
                git_stdout(Some(path), &["status", "--porcelain=v2", "--branch", "-z"]).await
            {
                wt.status = Some(parse_status_v2(&bytes));
            }
        }

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

        // Map OS processes to worktrees. The sysinfo scan is synchronous and a
        // little slow, so run it off the async executor.
        let roots: Vec<String> = worktrees.iter().map(|wt| wt.path.clone()).collect();
        let processes = tokio::task::spawn_blocking(move || crate::process::scan(&roots))
            .await
            .unwrap_or_default();

        Ok(Self {
            repo,
            worktrees,
            branches,
            processes,
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
