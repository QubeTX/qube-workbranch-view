//! Repository discovery — locate the work tree root and the (common) git dir.

use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, eyre};

use super::commands::run_git;

/// Identity of the repository wb300 is operating on.
#[derive(Debug, Clone)]
pub struct RepoIdentity {
    /// The directory wb300 was launched from / pointed at.
    pub start_dir: PathBuf,
    /// The work-tree root (`git rev-parse --show-toplevel`).
    pub root: PathBuf,
    /// This work tree's git dir (`--absolute-git-dir`). For a linked worktree
    /// this is `<main>/.git/worktrees/<name>`.
    pub git_dir: PathBuf,
    /// The shared git dir (`--git-common-dir`) — where refs/objects live and
    /// where wb300 stores its per-repo state.
    pub common_git_dir: PathBuf,
    /// True when launched inside a linked worktree (git_dir != common_git_dir).
    pub is_worktree: bool,
}

impl RepoIdentity {
    /// Discover the repository containing `start_dir`. Returns an error when
    /// `start_dir` is not inside a Git work tree (the caller decides whether to
    /// show a friendly message or open the machine-wide home view).
    pub async fn discover(start_dir: &Path) -> Result<Self> {
        // `--path-format=absolute` makes every path output absolute (git >= 2.31),
        // sidestepping cwd-relative `--git-common-dir`. Results print in option order.
        let out = run_git(
            Some(start_dir),
            &[
                "rev-parse",
                "--path-format=absolute",
                "--is-inside-work-tree",
                "--show-toplevel",
                "--absolute-git-dir",
                "--git-common-dir",
            ],
        )
        .await?;

        if !out.success() {
            return Err(eyre!("not a Git repository: {}", out.stderr_str().trim()));
        }

        let text = out.stdout_str();
        let mut lines = text.lines();
        let inside = lines.next().unwrap_or("").trim();
        if inside != "true" {
            return Err(eyre!("not inside a Git work tree"));
        }
        let root = next_path(&mut lines, "work-tree root")?;
        let git_dir = next_path(&mut lines, "git dir")?;
        let common_git_dir = next_path(&mut lines, "common git dir")?;
        let is_worktree = git_dir != common_git_dir;

        Ok(Self {
            start_dir: start_dir.to_path_buf(),
            root,
            git_dir,
            common_git_dir,
            is_worktree,
        })
    }

    /// Directory where wb300 stores per-repo state (events, snapshots).
    pub fn state_dir(&self) -> PathBuf {
        self.common_git_dir.join("wb300")
    }
}

fn next_path<'a>(lines: &mut impl Iterator<Item = &'a str>, what: &str) -> Result<PathBuf> {
    lines
        .next()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| eyre!("git rev-parse did not return the {what}"))
}
