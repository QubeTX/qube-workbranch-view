//! `RepoSnapshot` — the captured Git source of truth the UI renders from.

use color_eyre::eyre::Result;
use futures_util::StreamExt;

use super::commands::git_stdout;
use super::refs::{BranchInfo, FOR_EACH_REF_FORMAT, parse_refs};
use super::repo::RepoIdentity;
use super::status::{ChangeKind, FileChange, WorktreeStatus, parse_status_v2};
use super::worktree::{WorktreeRecord, parse_worktrees};
use crate::collision::Collision;
use crate::process::ProcessSnapshot;
use crate::util::paths::normalize;

/// Max concurrent Git subprocesses during a capture (handoff §21.3).
const GIT_CONCURRENCY: usize = 4;

/// A point-in-time view of the repository: worktrees, refs, the derived
/// branch hierarchy, collisions, processes.
#[derive(Debug, Clone)]
pub struct RepoSnapshot {
    pub repo: RepoIdentity,
    /// Base ref used for "committed since base" comparisons (e.g. `origin/main`).
    pub base: Option<String>,
    pub worktrees: Vec<WorktreeRecord>,
    pub branches: Vec<BranchInfo>,
    /// The derived branch tree (trunk → workbranches → tasks), with per-branch
    /// lifecycle and worktree attachment.
    pub hierarchy: super::hierarchy::BranchHierarchy,
    pub collisions: Vec<Collision>,
    pub processes: ProcessSnapshot,
    /// Epoch seconds when this capture completed (drives staleness chips).
    pub captured_at: u64,
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

        // The hierarchy skeleton (topology + parentage) — one cached rev-list,
        // run serially here so the per-worktree stream below keeps the git
        // concurrency cap intact. Worktree indices and lifecycle are resolved
        // after statuses land.
        let hierarchy = super::hierarchy::compute(&repo, &branches).await;

        // Collect owned (index, path) jobs first so the async tasks borrow
        // nothing from `worktrees` — keeping the capture future `Send + 'static`
        // for `tokio::spawn`.
        let jobs: Vec<(usize, std::path::PathBuf)> = worktrees
            .iter()
            .enumerate()
            .filter(|(_, wt)| !wt.bare)
            .map(|(idx, wt)| (idx, std::path::PathBuf::from(&wt.path)))
            .collect();

        // Per-worktree status + committed-since-base files, with bounded
        // concurrency so a repo with many worktrees doesn't serialize a long
        // chain of git calls. Working-tree changes come from the status parse
        // itself — no separate diff calls needed for them.
        let per_worktree = futures_util::stream::iter(jobs.into_iter().map(|(idx, path)| {
            let base = base.clone();
            async move {
                let status =
                    match git_stdout(Some(&path), &["status", "--porcelain=v2", "--branch", "-z"])
                        .await
                    {
                        Ok(bytes) => Some(parse_status_v2(&bytes)),
                        // Was silently `.ok()`-dropped — a failed status used to read
                        // as "clean" and hide that worktree's dirt and collisions.
                        Err(err) => {
                            tracing::warn!("status capture failed for {}: {err}", path.display());
                            None
                        }
                    };
                let committed = match base.as_deref() {
                    Some(base) => super::diff::committed_files(&path, base).await,
                    None => Vec::new(),
                };
                (idx, status, committed)
            }
        }))
        .buffer_unordered(GIT_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

        for (idx, status, committed) in per_worktree {
            worktrees[idx].touched = merge_touched(status.as_ref(), committed);
            worktrees[idx].status = status;
        }

        let hierarchy = hierarchy.finalize(&branches, &worktrees);
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
            hierarchy,
            collisions,
            processes,
            captured_at: crate::storage::events::epoch_secs(),
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

/// Union of a worktree's working-tree changes and its committed-since-base
/// files, keyed by path. The working-tree kind wins (it is the current state).
/// Sorted by path, de-duplicated.
fn merge_touched(status: Option<&WorktreeStatus>, committed: Vec<FileChange>) -> Vec<FileChange> {
    use std::collections::BTreeMap;
    let mut by_path: BTreeMap<String, ChangeKind> =
        committed.into_iter().map(|f| (f.path, f.kind)).collect();
    if let Some(s) = status {
        for f in &s.changes {
            by_path.insert(f.path.clone(), f.kind);
        }
    }
    by_path
        .into_iter()
        .map(|(path, kind)| FileChange { path, kind })
        .collect()
}

/// Pick a base branch for "committed since base" comparisons: the remote
/// default branch (the `origin/HEAD` symref target) when present, else the
/// first of `origin/main`, `origin/master`, `main`, `master` that exists.
fn detect_base(branches: &[BranchInfo]) -> Option<String> {
    if let Some(target) = branches.iter().find_map(|b| b.symref_target.as_deref())
        && branches.iter().any(|b| b.short == target)
    {
        return Some(target.to_string());
    }
    const PREFERRED: &[&str] = &["origin/main", "origin/master", "main", "master"];
    PREFERRED
        .iter()
        .find(|cand| branches.iter().any(|b| b.short == **cand))
        .map(|cand| (*cand).to_string())
}
