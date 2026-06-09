//! `wb300 agent` — the headless JSON snapshot.
//!
//! Emits the full repo (or machine-wide) state as JSON so an orchestrating
//! agent (Claude / Codex / a script) can get an instant, structured view of the
//! worktree/branch/workbranch/agent/collision picture without driving the TUI.
//!
//! The structs here form a **stable JSON contract** (`schema: "wb300.agent.v1"`)
//! that is deliberately decoupled from the internal data models, so refactors
//! to `RepoSnapshot`/`WorktreeRecord` don't silently break downstream consumers.

use serde::Serialize;

use crate::collision::Collision;
use crate::git::{RepoSnapshot, WorktreeRecord};
use crate::home::{repo_name, workbranch_label};
use crate::process::ProcessInfo;

/// The schema identifier emitted with every report; bump on a breaking change.
const SCHEMA: &str = "wb300.agent.v1";

/// Top-level `wb300 agent` output.
#[derive(Debug, Serialize)]
pub struct Report {
    /// Stable schema tag for consumers to version against.
    pub schema: &'static str,
    pub wb300_version: &'static str,
    /// Epoch seconds when the report was produced.
    pub generated_at: u64,
    /// `"repo"` for a single repository, `"home"` for the machine-wide view.
    pub mode: &'static str,
    /// Present in `repo` mode: the current repository.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<RepoReport>,
    /// Every active repository in `home` mode; an empty array in `repo` mode.
    /// Always serialized so a consumer can key on `mode` (or on the presence of
    /// `repo`) without a missing-field ambiguity when the home view finds zero
    /// repos.
    pub repos: Vec<RepoReport>,
}

impl Report {
    /// Build a single-repository report.
    pub fn repo(snap: &RepoSnapshot) -> Self {
        Self {
            schema: SCHEMA,
            wb300_version: crate::VERSION,
            generated_at: crate::storage::events::epoch_secs(),
            mode: "repo",
            repo: Some(RepoReport::from_snapshot(snap)),
            repos: Vec::new(),
        }
    }

    /// Build a machine-wide report from already-captured repo snapshots.
    pub fn home(snaps: &[RepoSnapshot], generated_at: u64) -> Self {
        Self {
            schema: SCHEMA,
            wb300_version: crate::VERSION,
            generated_at,
            mode: "home",
            repo: None,
            repos: snaps.iter().map(RepoReport::from_snapshot).collect(),
        }
    }

    /// Serialize to pretty JSON (valid, and inspectable by a human or agent).
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self)
            .unwrap_or_else(|e| format!("{{\"schema\":\"{SCHEMA}\",\"error\":\"{e}\"}}"))
    }
}

#[derive(Debug, Serialize)]
pub struct RepoReport {
    pub name: String,
    pub root: String,
    pub common_git_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    pub local_branches: usize,
    pub remote_branches: usize,
    pub worktree_count: usize,
    /// Worktrees currently running an agent.
    pub active_count: usize,
    pub worktrees: Vec<WorktreeReport>,
    /// Worktrees grouped by their (heuristic) workbranch.
    pub workbranches: Vec<WorkbranchGroup>,
    pub collisions: Vec<CollisionReport>,
}

impl RepoReport {
    fn from_snapshot(snap: &RepoSnapshot) -> Self {
        let worktrees: Vec<WorktreeReport> = snap
            .worktrees
            .iter()
            .enumerate()
            .map(|(idx, wt)| WorktreeReport::from_record(snap, idx, wt))
            .collect();

        // Group worktree display names by workbranch, preserving first-seen order.
        let mut workbranches: Vec<WorkbranchGroup> = Vec::new();
        for wt in &snap.worktrees {
            if wt.bare {
                continue;
            }
            let label = workbranch_label(wt.branch_short());
            let name = wt.display_name();
            match workbranches.iter_mut().find(|g| g.workbranch == label) {
                Some(group) => group.worktrees.push(name),
                None => workbranches.push(WorkbranchGroup {
                    workbranch: label,
                    worktrees: vec![name],
                }),
            }
        }

        let active_count = (0..snap.worktrees.len())
            .filter(|&i| snap.processes.worktree_is_active(i))
            .count();

        Self {
            name: repo_name(snap),
            root: snap.repo.root.display().to_string(),
            common_git_dir: snap.repo.common_git_dir.display().to_string(),
            base: snap.base.clone(),
            local_branches: snap.local_branch_count(),
            remote_branches: snap.remote_branch_count(),
            worktree_count: snap.worktrees.len(),
            active_count,
            worktrees,
            workbranches,
            collisions: snap
                .collisions
                .iter()
                .map(|c| CollisionReport::from_collision(snap, c))
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct WorktreeReport {
    pub path: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub workbranch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
    pub prunable: bool,
    /// Working-tree status, when captured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<StatusReport>,
    /// Number of detected collisions this worktree participates in.
    pub collisions: usize,
    /// The primary coding agent running in this worktree, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<ProcReport>,
    /// All processes mapped into this worktree.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub processes: Vec<ProcReport>,
}

impl WorktreeReport {
    fn from_record(snap: &RepoSnapshot, idx: usize, wt: &WorktreeRecord) -> Self {
        let collisions = snap
            .collisions
            .iter()
            .filter(|c| c.worktrees.contains(&idx))
            .count();
        let processes: Vec<ProcReport> = snap
            .processes
            .for_worktree(idx)
            .map(ProcReport::from_info)
            .collect();
        let agent = snap
            .processes
            .agent_for_worktree(idx)
            .map(ProcReport::from_info);

        Self {
            path: wt.path.clone(),
            name: wt.display_name(),
            branch: wt.branch_short().map(str::to_string),
            workbranch: workbranch_label(wt.branch_short()),
            head: wt.short_head().map(str::to_string),
            detached: wt.detached,
            bare: wt.bare,
            locked: wt.locked.is_some(),
            prunable: wt.prunable.is_some(),
            status: wt.status.as_ref().map(StatusReport::from_status),
            collisions,
            agent,
            processes,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StatusReport {
    pub clean: bool,
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicted: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ahead: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behind: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    pub upstream_gone: bool,
}

impl StatusReport {
    fn from_status(s: &crate::git::WorktreeStatus) -> Self {
        Self {
            clean: s.clean,
            staged: s.staged,
            unstaged: s.unstaged,
            untracked: s.untracked,
            conflicted: s.conflicted,
            ahead: s.ahead,
            behind: s.behind,
            upstream: s.upstream.clone(),
            upstream_gone: s.upstream_gone,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ProcReport {
    pub pid: u32,
    pub name: String,
    pub label: String,
    pub cmd: String,
    pub cpu: f32,
    pub memory_bytes: u64,
    pub run_secs: u64,
}

impl ProcReport {
    fn from_info(p: &ProcessInfo) -> Self {
        Self {
            pid: p.pid,
            name: p.name.clone(),
            label: p.label.as_str().to_string(),
            cmd: p.cmd.clone(),
            // Sanitize: sysinfo can report NaN/Inf for cpu (especially on the
            // first sample), which serde_json renders as `null` — silently
            // breaking the numeric contract. Clamp non-finite to 0.0.
            cpu: if p.cpu.is_finite() { p.cpu } else { 0.0 },
            memory_bytes: p.memory_bytes,
            run_secs: p.run_secs,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct WorkbranchGroup {
    pub workbranch: String,
    pub worktrees: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CollisionReport {
    pub file: String,
    pub severity: String,
    /// Display names of the worktrees touching this file.
    pub worktrees: Vec<String>,
}

impl CollisionReport {
    fn from_collision(snap: &RepoSnapshot, c: &Collision) -> Self {
        Self {
            file: c.file.clone(),
            severity: c.severity.label().to_string(),
            worktrees: c
                .worktrees
                .iter()
                .filter_map(|&i| snap.worktrees.get(i).map(|wt| wt.display_name()))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{RepoIdentity, RepoSnapshot, WorktreeRecord, WorktreeStatus};
    use crate::process::ProcessSnapshot;

    fn snapshot() -> RepoSnapshot {
        RepoSnapshot {
            repo: RepoIdentity {
                start_dir: "/repo".into(),
                root: "/repo".into(),
                git_dir: "/repo/.git".into(),
                common_git_dir: "/repo/.git".into(),
                is_worktree: false,
            },
            base: Some("origin/main".into()),
            worktrees: vec![
                WorktreeRecord {
                    path: "/repo".into(),
                    branch: Some("refs/heads/main".into()),
                    head: Some("abcdef1234".into()),
                    ..Default::default()
                },
                WorktreeRecord {
                    path: "/repo-feat".into(),
                    branch: Some("refs/heads/feat/x-1".into()),
                    status: Some(WorktreeStatus {
                        clean: false,
                        staged: 1,
                        ahead: Some(2),
                        upstream: Some("origin/feat/x-1".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ],
            branches: Vec::new(),
            collisions: Vec::new(),
            processes: ProcessSnapshot::default(),
        }
    }

    #[test]
    fn repo_report_shapes_the_snapshot() {
        let report = Report::repo(&snapshot());
        assert_eq!(report.mode, "repo");
        assert_eq!(report.schema, "wb300.agent.v1");
        let repo = report.repo.as_ref().unwrap();
        assert_eq!(repo.worktree_count, 2);
        assert_eq!(repo.worktrees[1].branch.as_deref(), Some("feat/x-1"));
        assert_eq!(repo.worktrees[1].workbranch, "feat");
        assert_eq!(repo.worktrees[1].status.as_ref().unwrap().staged, 1);
        // Grouped by workbranch: main, then feat.
        let labels: Vec<&str> = repo
            .workbranches
            .iter()
            .map(|g| g.workbranch.as_str())
            .collect();
        assert_eq!(labels, vec!["main", "feat"]);
    }

    #[test]
    fn report_serializes_to_valid_json() {
        let json = Report::repo(&snapshot()).to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["schema"], "wb300.agent.v1");
        assert_eq!(parsed["mode"], "repo");
        assert_eq!(parsed["repo"]["worktree_count"], 2);
        // `repo` present in repo mode; `repos` is always present (empty here).
        assert!(parsed.get("repo").is_some());
        assert!(parsed["repos"].as_array().unwrap().is_empty());
    }

    #[test]
    fn home_report_lists_repos() {
        let report = Report::home(std::slice::from_ref(&snapshot()), 42);
        assert_eq!(report.mode, "home");
        assert_eq!(report.generated_at, 42);
        assert_eq!(report.repos.len(), 1);
        let json = report.to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["repos"][0]["name"], "repo");
        assert!(parsed.get("repo").is_none());
    }
}
