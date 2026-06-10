//! `HomeSnapshot` — the machine-wide view: one captured `RepoSnapshot` per
//! active repository. Each per-repo snapshot reuses the full per-repo pipeline
//! (worktrees, status, collisions, process→worktree mapping), so the home view
//! gets agent labels and dirty/ahead-behind for free.

use std::cmp::Reverse;

use futures_util::StreamExt;

use super::discovery::{HomeConfig, discover_active_repos};
use crate::git::RepoSnapshot;

/// Capture at most this many repos concurrently. Each `RepoSnapshot::capture`
/// already runs its own bounded git concurrency, so this caps the multiplier
/// (≤ 3 repos × 4 git subprocesses) — keeping a busy machine from being swamped.
const HOME_CAPTURE_CONCURRENCY: usize = 3;

/// A point-in-time machine-wide view: the active repos, each fully captured.
#[derive(Debug, Clone)]
pub struct HomeSnapshot {
    pub repos: Vec<RepoSnapshot>,
    /// Epoch seconds when this scan completed (for the header "scanned N ago").
    pub scanned_at: u64,
}

impl HomeSnapshot {
    /// Discover the active repos and capture each one. Repos that fail to
    /// capture (e.g. removed between discovery and capture) are dropped. Sorted
    /// by display name for a stable card order across refreshes.
    pub async fn capture(config: &HomeConfig) -> Self {
        let ids = discover_active_repos(config).await;

        let mut repos: Vec<RepoSnapshot> =
            futures_util::stream::iter(ids.into_iter().map(|id| async move {
                // Log rather than silently drop a repo whose capture failed, so a
                // wedged repo disappearing from the board is at least diagnosable.
                match RepoSnapshot::capture(id).await {
                    Ok(snap) => Some(snap),
                    Err(err) => {
                        tracing::warn!("home: repo capture failed, dropping from view: {err}");
                        None
                    }
                }
            }))
            .buffer_unordered(HOME_CAPTURE_CONCURRENCY)
            .filter_map(|r| async move { r })
            .collect()
            .await;

        // Surface repos with a live agent first (the control-tower thesis), then
        // alphabetically — a stable, predictable card order across refreshes.
        repos.sort_by_cached_key(|r| (Reverse(repo_active_count(r)), repo_name(r)));

        Self {
            repos,
            scanned_at: crate::storage::events::epoch_secs(),
        }
    }

    /// Total worktrees across all repos (header summary).
    pub fn total_worktrees(&self) -> usize {
        self.repos.iter().map(|r| r.worktrees.len()).sum()
    }

    /// Total worktrees currently running an agent, across all repos.
    pub fn total_active(&self) -> usize {
        self.repos
            .iter()
            .map(|r| {
                (0..r.worktrees.len())
                    .filter(|&i| r.processes.worktree_is_active(i))
                    .count()
            })
            .sum()
    }

    /// All worktree paths across all repos (for the live filesystem watcher).
    pub fn watch_paths(&self) -> Vec<std::path::PathBuf> {
        let mut paths = Vec::new();
        for repo in &self.repos {
            for wt in &repo.worktrees {
                if !wt.bare {
                    paths.push(std::path::PathBuf::from(&wt.path));
                }
            }
        }
        paths.sort();
        paths.dedup();
        paths
    }
}

/// Worktrees in `snap` currently running an agent (drives the card sort order).
fn repo_active_count(snap: &RepoSnapshot) -> usize {
    (0..snap.worktrees.len())
        .filter(|&i| snap.processes.worktree_is_active(i))
        .count()
}

/// Stable identity key for a repo across scans: its normalized common git dir
/// (shared by a repo and all its linked worktrees). Used to match a repo to its
/// previous-scan counterpart when raising live transition flashes.
pub fn repo_key(snap: &RepoSnapshot) -> String {
    crate::util::paths::normalize(&snap.repo.common_git_dir.to_string_lossy())
}

/// Display name for a repo card: the basename of its main checkout (the first
/// non-bare worktree, which `git worktree list` always reports first), falling
/// back to the discovered root.
pub fn repo_name(snap: &RepoSnapshot) -> String {
    let main = snap
        .worktrees
        .iter()
        .find(|w| !w.bare)
        .map(|w| w.path.as_str())
        .unwrap_or_else(|| {
            // No worktrees parsed — fall back to the identity root.
            snap.repo.root.to_str().unwrap_or("")
        });
    basename(main)
}

/// Final path component (handles both `/` and `\` separators).
fn basename(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_both_separators() {
        assert_eq!(basename("/home/u/git/myrepo"), "myrepo");
        assert_eq!(basename(r"C:\Users\u\git\myrepo"), "myrepo");
        assert_eq!(basename("/home/u/git/myrepo/"), "myrepo");
        assert_eq!(basename("myrepo"), "myrepo");
    }
}
