//! `RepoSnapshot` — the captured Git source of truth the UI renders from.

use color_eyre::eyre::Result;
use futures_util::StreamExt;

use super::commands::git_stdout;
use super::refs::{BranchInfo, FOR_EACH_REF_FORMAT, parse_refs};
use super::repo::RepoIdentity;
use super::status::parse_status_v2;
use super::worktree::{WorktreeRecord, parse_worktrees};
use crate::collision::Collision;
use crate::process::ProcessSnapshot;
use crate::util::paths::normalize;

/// Max concurrent Git subprocesses during a capture (handoff §21.3).
const GIT_CONCURRENCY: usize = 4;

/// A point-in-time view of the repository: worktrees, refs, collisions, processes.
#[derive(Debug, Clone)]
pub struct RepoSnapshot {
    pub repo: RepoIdentity,
    /// Base ref used for "committed since base" comparisons (e.g. `origin/main`).
    pub base: Option<String>,
    pub worktrees: Vec<WorktreeRecord>,
    pub branches: Vec<BranchInfo>,
    pub collisions: Vec<Collision>,
    pub processes: ProcessSnapshot,
}

impl RepoSnapshot {
    /// Capture worktrees + refs + per-worktree status/touched + collisions +
    /// processes for `repo` via the Git CLI.
    pub async fn capture(repo: RepoIdentity) -> Result<Self> {
        let root = repo.root.clone();

        let wt_bytes = git_stdout(Some(&root), &["worktree", "list", "--porcelain", "-z"]).await?;
        let mut worktrees = parse_worktrees(&wt_bytes);

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
        let base = detect_base(&branches);

        // Collect owned (index, path) jobs first so the async tasks borrow
        // nothing from `worktrees` — keeping the capture future `Send + 'static`
        // for `tokio::spawn`.
        let jobs: Vec<(usize, std::path::PathBuf)> = worktrees
            .iter()
            .enumerate()
            .filter(|(_, wt)| !wt.bare)
            .map(|(idx, wt)| (idx, std::path::PathBuf::from(&wt.path)))
            .collect();

        // Per-worktree status + touched-file sets, with bounded concurrency so a
        // repo with many worktrees doesn't serialize a long chain of git calls.
        let per_worktree = futures_util::stream::iter(jobs.into_iter().map(|(idx, path)| {
            let base = base.clone();
            async move {
                let status =
                    git_stdout(Some(&path), &["status", "--porcelain=v2", "--branch", "-z"])
                        .await
                        .ok()
                        .map(|bytes| parse_status_v2(&bytes));
                let touched = super::diff::touched_files(&path, base.as_deref()).await;
                (idx, status, touched)
            }
        }))
        .buffer_unordered(GIT_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

        for (idx, status, touched) in per_worktree {
            worktrees[idx].status = status;
            worktrees[idx].touched = touched;
        }

        let collisions = crate::collision::compute(&worktrees);

        // Map OS processes to worktrees (sync sysinfo scan, off the executor).
        let roots: Vec<String> = worktrees.iter().map(|wt| wt.path.clone()).collect();
        let processes = tokio::task::spawn_blocking(move || crate::process::scan(&roots))
            .await
            .unwrap_or_default();

        Ok(Self {
            repo,
            base,
            worktrees,
            branches,
            collisions,
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

/// Pick a base branch for "committed since base" comparisons: the first of
/// `origin/main`, `origin/master`, `main`, `master` that exists.
fn detect_base(branches: &[BranchInfo]) -> Option<String> {
    const PREFERRED: &[&str] = &["origin/main", "origin/master", "main", "master"];
    PREFERRED
        .iter()
        .find(|cand| branches.iter().any(|b| b.short == **cand))
        .map(|cand| (*cand).to_string())
}
