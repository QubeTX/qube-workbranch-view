//! Worktree mutation + rescue-snapshot operations (handoff §17.8–17.9).
//!
//! These are the only Git operations that *change* repository state. They run
//! from the event loop (off the UI task) behind explicit, type-to-confirm
//! dialogs, and dirty worktrees get a rescue patch saved first.

use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, eyre};

use super::commands::{git_stdout, run_git};

/// Remove a worktree: `git -C <root> worktree remove [--force] <path>`.
pub async fn remove_worktree(root: &Path, worktree_path: &str, force: bool) -> Result<()> {
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(worktree_path);
    let out = run_git(Some(root), &args).await?;
    if !out.success() {
        return Err(eyre!(
            "git worktree remove failed: {}",
            out.stderr_str().trim()
        ));
    }
    Ok(())
}

/// Prune stale worktree metadata: `git -C <root> worktree prune`.
pub async fn prune_worktrees(root: &Path) -> Result<()> {
    let out = run_git(Some(root), &["worktree", "prune"]).await?;
    if !out.success() {
        return Err(eyre!(
            "git worktree prune failed: {}",
            out.stderr_str().trim()
        ));
    }
    Ok(())
}

/// Save rescue patches of a worktree's uncommitted work (unstaged + staged)
/// into `dest_dir`, returning the unstaged patch path. Best-effort: empty diffs
/// still write (zero-byte) files so the rescue is recorded.
pub async fn snapshot_worktree(
    worktree_path: &Path,
    dest_dir: &Path,
    label: &str,
    stamp: u64,
) -> Result<PathBuf> {
    std::fs::create_dir_all(dest_dir)?;
    let unstaged = git_stdout(Some(worktree_path), &["diff"])
        .await
        .unwrap_or_default();
    let staged = git_stdout(Some(worktree_path), &["diff", "--staged"])
        .await
        .unwrap_or_default();

    let safe_label: String = label
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let unstaged_path = dest_dir.join(format!("{stamp}-{safe_label}.patch"));
    let staged_path = dest_dir.join(format!("{stamp}-{safe_label}.staged.patch"));
    std::fs::write(&unstaged_path, &unstaged)?;
    std::fs::write(&staged_path, &staged)?;
    Ok(unstaged_path)
}
