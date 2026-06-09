//! Discover Git repositories that are actively being worked on, for the
//! machine-wide home view (handoff §17.16).
//!
//! Two sources, deduplicated by the shared (common) git dir so a repo and all
//! its linked worktrees collapse to a single entry:
//!   1. **Process-driven (primary):** every repo that currently has a coding
//!      agent (Claude / Codex / …) running inside it — the "control tower of
//!      live agents" use case.
//!   2. **Configured roots (supplement, bounded):** repos under `~/git`,
//!      `~/code`, … that are set up for parallel work (≥ 2 worktrees), so the
//!      home view is useful even between agent runs.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::git::RepoIdentity;
use crate::git::commands::run_git;
use crate::git::worktree::parse_worktrees;
use crate::util::paths::normalize;

/// Hard caps keep discovery bounded so a machine with hundreds of repos can't
/// turn the home view into an unbounded sweep (the "don't hammer the box" rule).
const DEFAULT_MAX_REPOS: usize = 32;
const DEFAULT_MAX_CHILDREN: usize = 64;

/// Configuration for home-view discovery. Defaults come from the environment;
/// the config subsystem makes these user-overridable later (`[home]` table).
#[derive(Debug, Clone)]
pub struct HomeConfig {
    /// Directories scanned one level deep for parallel-work repos.
    pub roots: Vec<PathBuf>,
    /// Upper bound on repos shown (process + configured-root sources combined).
    pub max_repos: usize,
    /// Upper bound on child directories examined per configured root.
    pub max_children_per_root: usize,
}

impl HomeConfig {
    /// Default config: the usual code-home directories under `$HOME`.
    pub fn from_env() -> Self {
        let mut roots = Vec::new();
        if let Some(home) = home_dir() {
            for name in ["git", "code", "src", "dev", "Projects", "projects", "repos"] {
                roots.push(home.join(name));
            }
        }
        Self {
            roots,
            max_repos: DEFAULT_MAX_REPOS,
            max_children_per_root: DEFAULT_MAX_CHILDREN,
        }
    }
}

/// The user's home directory, cross-platform without an external dep.
pub fn home_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            return Some(PathBuf::from(profile));
        }
        // Fallback for provisioned/enterprise Windows where USERPROFILE is unset
        // but the legacy HOMEDRIVE + HOMEPATH pair is present.
        if let (Some(drive), Some(path)) =
            (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH"))
        {
            let mut home = PathBuf::from(drive);
            home.push(path);
            return Some(home);
        }
        None
    } else {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// Directory for machine-wide wb300 state (home-view log), independent of any
/// single repo. Falls back to a temp dir so logging never blocks startup.
pub fn home_state_dir() -> PathBuf {
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("wb300");
        }
    } else if let Some(xdg) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(xdg).join("wb300");
    } else if let Some(home) = home_dir() {
        return home.join(".local").join("state").join("wb300");
    }
    std::env::temp_dir().join("wb300")
}

/// Discover the active repositories to show in the home view, deduplicated and
/// capped. Process-driven entries are collected first, so when more than
/// `max_repos` repos exist, repos with a live agent win the cut. (Display order
/// is decided later by `HomeSnapshot::capture`, which surfaces active repos at
/// the top.) The returned identities are ready for `RepoSnapshot::capture`.
pub async fn discover_active_repos(config: &HomeConfig) -> Vec<RepoIdentity> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<RepoIdentity> = Vec::new();

    // 1. Process-driven: every repo with a running agent inside it. The
    //    machine-wide process scan runs off the executor (it's a full sysinfo
    //    refresh).
    let cwds = tokio::task::spawn_blocking(crate::process::scan_agent_cwds)
        .await
        .unwrap_or_default();
    for cwd in cwds {
        if out.len() >= config.max_repos {
            break;
        }
        if let Ok(id) = RepoIdentity::discover(&cwd).await {
            push_unique(&mut out, &mut seen, id);
        }
    }

    // 2. Configured roots: repos set up for parallel work (≥ 2 worktrees).
    //    Enumerate candidate child directories off the executor — `read_dir`
    //    plus a per-entry `is_dir` stat is blocking I/O, and there can be
    //    hundreds across the default roots.
    let roots = config.roots.clone();
    let cap = config.max_children_per_root;
    let candidates = tokio::task::spawn_blocking(move || {
        let mut dirs = Vec::new();
        for root in &roots {
            dirs.extend(list_dirs(root, cap));
        }
        dirs
    })
    .await
    .unwrap_or_default();

    for child in candidates {
        if out.len() >= config.max_repos {
            break;
        }
        let Ok(id) = RepoIdentity::discover(&child).await else {
            continue;
        };
        if seen.contains(&common_key(&id)) {
            continue; // already discovered (often via an agent CWD)
        }
        if has_linked_worktrees(&id).await {
            push_unique(&mut out, &mut seen, id);
        }
    }

    out
}

/// Dedup key: the normalized shared git dir, so a repo and its linked worktrees
/// (which share one common git dir) collapse to a single home-view entry.
fn common_key(id: &RepoIdentity) -> String {
    normalize(&id.common_git_dir.to_string_lossy())
}

fn push_unique(out: &mut Vec<RepoIdentity>, seen: &mut HashSet<String>, id: RepoIdentity) {
    if seen.insert(common_key(&id)) {
        out.push(id);
    }
}

/// Immediate subdirectories of `root`, capped. Returns empty if `root` is
/// missing or unreadable (most of the default roots won't exist on any one box).
fn list_dirs(root: &Path, cap: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        if dirs.len() >= cap {
            break;
        }
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs
}

/// Whether `id` has at least one *linked* worktree (≥ 2 total non-bare),
/// the signal that a repo is set up for parallel-agent work.
async fn has_linked_worktrees(id: &RepoIdentity) -> bool {
    match run_git(Some(&id.root), &["worktree", "list", "--porcelain", "-z"]).await {
        Ok(out) if out.success() => {
            parse_worktrees(&out.stdout)
                .iter()
                .filter(|w| !w.bare)
                .count()
                >= 2
        }
        _ => false,
    }
}
