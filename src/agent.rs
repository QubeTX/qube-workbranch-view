//! `wb300 agent` — the headless JSON snapshot.
//!
//! Emits the full repo (or machine-wide) state as JSON so an orchestrating
//! agent (Claude / Codex / a script) can get an instant, structured view of
//! the branch hierarchy — trunk → workbranch → task branches, each with its
//! lifecycle stage, worktree, attached agent, and changed files — plus the
//! worktree-level and collision views, without driving the TUI.
//!
//! The structs here form a **stable JSON contract** (`schema: "wb300.agent.v2"`)
//! that is deliberately decoupled from the internal data models, so refactors
//! to `RepoSnapshot`/`WorktreeRecord` don't silently break downstream
//! consumers. v2 replaced v1's name-prefix "workbranch" grouping with the real
//! branch hierarchy (`branches`, depth-first with parent pointers).

use serde::Serialize;

use crate::collision::Collision;
use crate::git::{BranchNode, RepoSnapshot, WorktreeRecord};
use crate::home::repo_name;
use crate::process::ProcessInfo;

/// The schema identifier emitted with every report; bump on a breaking change.
const SCHEMA: &str = "wb300.agent.v2";

/// Cap on the per-branch changed-file list in the report; `files_total`
/// always carries the real count.
pub const MAX_REPORT_FILES: usize = 50;

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
    /// Local short name of the recognized trunk branch, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trunk: Option<String>,
    /// True when branch parentage degraded (topology walk failed/capped).
    pub hierarchy_approximate: bool,
    pub local_branches: usize,
    pub remote_branches: usize,
    pub worktree_count: usize,
    /// Worktrees currently running an agent.
    pub active_count: usize,
    /// The branch hierarchy in depth-first order (trunk first, each branch
    /// followed by its subtree) — reads as a tree top-to-bottom; rebuild the
    /// nesting from `parent` pointers.
    pub branches: Vec<BranchReport>,
    /// The path-level view: every worktree on disk (incl. detached/bare).
    pub worktrees: Vec<WorktreeReport>,
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

        let branches: Vec<BranchReport> = snap
            .hierarchy
            .nodes
            .iter()
            .map(|n| BranchReport::from_node(snap, n))
            .collect();

        let active_count = (0..snap.worktrees.len())
            .filter(|&i| snap.processes.worktree_is_active(i))
            .count();

        Self {
            name: repo_name(snap),
            root: snap.repo.root.display().to_string(),
            common_git_dir: snap.repo.common_git_dir.display().to_string(),
            base: snap.base.clone(),
            trunk: snap.hierarchy.trunk.clone(),
            hierarchy_approximate: snap.hierarchy.approximate,
            local_branches: snap.local_branch_count(),
            remote_branches: snap.remote_branch_count(),
            worktree_count: snap.worktrees.len(),
            active_count,
            branches,
            worktrees,
            collisions: snap
                .collisions
                .iter()
                .map(|c| CollisionReport::from_collision(snap, c))
                .collect(),
        }
    }
}

/// One branch in the hierarchy: where it sits (role/parent), where its work
/// stands (lifecycle, ahead/behind), and what is physically attached to it
/// (worktree, agent, changed files).
#[derive(Debug, Serialize)]
pub struct BranchReport {
    /// Short name, e.g. `feat/csv-export-142`.
    pub name: String,
    /// `"trunk"` | `"workbranch"` | `"task"` | `"standalone"`.
    pub role: String,
    /// Parent branch short name; absent for trunk (or when no trunk exists).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub oid: String,
    /// `"editing"`* | `"uncommitted"` | `"committed"` | `"pushed"` |
    /// `"merged"` | `"fresh"`. (*never emitted by one-shot captures — live
    /// editing state needs the running TUI's filesystem watcher.)
    pub lifecycle: String,
    /// Commits on this branch not reachable from its parent.
    pub ahead_of_parent: u32,
    /// Commits on the parent not on this branch ("needs rebase" when > 0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behind_parent: Option<u32>,
    pub merged_into_parent: bool,
    /// Whether the branch matters right now: a worktree, unmerged/unpushed
    /// work, or an active descendant (trunk is always active).
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ahead: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behind: Option<u32>,
    pub upstream_gone: bool,
    /// Path of the worktree where this branch is checked out, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// The primary coding agent running in this branch's worktree, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<ProcReport>,
    /// Changed files (working tree + committed since base), capped at
    /// [`MAX_REPORT_FILES`]; `files_total` is the uncapped count.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileReport>,
    pub files_total: usize,
    /// Number of merge-conflict risks this branch's worktree participates in.
    pub collisions: usize,
}

impl BranchReport {
    fn from_node(snap: &RepoSnapshot, node: &BranchNode) -> Self {
        let wt = node.worktree.and_then(|i| snap.worktrees.get(i));
        let files: Vec<FileReport> = wt
            .map(|w| {
                w.touched
                    .iter()
                    .take(MAX_REPORT_FILES)
                    .map(|f| FileReport {
                        path: f.path.clone(),
                        kind: f.kind.as_str().to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let files_total = wt.map_or(0, |w| w.touched.len());
        let collisions = node.worktree.map_or(0, |i| {
            crate::collision::count_for_worktree(&snap.collisions, i)
        });
        let agent = node
            .worktree
            .and_then(|i| snap.processes.agent_for_worktree(i))
            .map(ProcReport::from_info);

        Self {
            name: node.name.clone(),
            role: node.role.as_str().to_string(),
            parent: node.parent.clone(),
            oid: node.oid.clone(),
            lifecycle: node.lifecycle.as_str().to_string(),
            ahead_of_parent: node.ahead_of_parent,
            behind_parent: node.behind_parent,
            merged_into_parent: node.merged_into_parent,
            active: snap.hierarchy.is_active(node),
            upstream: node.upstream.clone(),
            ahead: node.ahead,
            behind: node.behind,
            upstream_gone: node.upstream_gone,
            worktree: wt.map(|w| w.path.clone()),
            agent,
            files,
            files_total,
            collisions,
        }
    }
}

/// One changed file under a branch.
#[derive(Debug, Serialize)]
pub struct FileReport {
    /// Repo-relative path.
    pub path: String,
    /// `"modified"` | `"added"` | `"deleted"` | `"renamed"` | `"untracked"` |
    /// `"conflicted"`.
    pub kind: String,
}

#[derive(Debug, Serialize)]
pub struct WorktreeReport {
    pub path: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
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
    use crate::git::{
        BranchHierarchy, BranchLifecycle, BranchRole, ChangeKind, FileChange, RepoIdentity,
        RepoSnapshot, WorktreeRecord, WorktreeStatus,
    };
    use crate::process::ProcessSnapshot;

    fn node(
        name: &str,
        role: BranchRole,
        parent: Option<&str>,
        worktree: Option<usize>,
    ) -> BranchNode {
        BranchNode {
            name: name.to_string(),
            oid: format!("oid-{name}"),
            role,
            parent: parent.map(str::to_string),
            ahead_of_parent: if role == BranchRole::Trunk { 0 } else { 1 },
            behind_parent: Some(0),
            merged_into_parent: false,
            upstream: Some(format!("origin/{name}")),
            ahead: Some(2),
            behind: Some(0),
            upstream_gone: false,
            committer_date: None,
            lifecycle: BranchLifecycle::Committed,
            worktree,
        }
    }

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
                    touched: vec![FileChange {
                        path: "src/x.rs".into(),
                        kind: ChangeKind::Modified,
                    }],
                    ..Default::default()
                },
            ],
            branches: Vec::new(),
            hierarchy: BranchHierarchy {
                trunk: Some("main".into()),
                nodes: vec![
                    node("main", BranchRole::Trunk, None, Some(0)),
                    node(
                        "emmett/wb-2026-06-10",
                        BranchRole::Workbranch,
                        Some("main"),
                        None,
                    ),
                    node(
                        "feat/x-1",
                        BranchRole::Task,
                        Some("emmett/wb-2026-06-10"),
                        Some(1),
                    ),
                ],
                approximate: false,
            },
            collisions: Vec::new(),
            processes: ProcessSnapshot::default(),
            captured_at: 0,
        }
    }

    #[test]
    fn repo_report_shapes_the_hierarchy() {
        let report = Report::repo(&snapshot());
        assert_eq!(report.mode, "repo");
        assert_eq!(report.schema, "wb300.agent.v2");
        let repo = report.repo.as_ref().unwrap();
        assert_eq!(repo.trunk.as_deref(), Some("main"));
        assert!(!repo.hierarchy_approximate);

        // Depth-first hierarchy with parent pointers.
        let names: Vec<&str> = repo.branches.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, vec!["main", "emmett/wb-2026-06-10", "feat/x-1"]);
        let task = &repo.branches[2];
        assert_eq!(task.role, "task");
        assert_eq!(task.parent.as_deref(), Some("emmett/wb-2026-06-10"));
        assert_eq!(task.lifecycle, "committed");
        assert_eq!(task.worktree.as_deref(), Some("/repo-feat"));
        assert_eq!(task.files.len(), 1);
        assert_eq!(task.files[0].kind, "modified");
        assert_eq!(task.files_total, 1);

        // The path-level worktree view survives (without the old fake
        // workbranch label).
        assert_eq!(repo.worktree_count, 2);
        assert_eq!(repo.worktrees[1].branch.as_deref(), Some("feat/x-1"));
        assert_eq!(repo.worktrees[1].status.as_ref().unwrap().staged, 1);
    }

    #[test]
    fn report_serializes_to_valid_v2_json() {
        let json = Report::repo(&snapshot()).to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["schema"], "wb300.agent.v2");
        assert_eq!(parsed["mode"], "repo");
        assert_eq!(parsed["repo"]["trunk"], "main");
        assert_eq!(parsed["repo"]["branches"][2]["name"], "feat/x-1");
        assert_eq!(
            parsed["repo"]["branches"][2]["parent"],
            "emmett/wb-2026-06-10"
        );
        // The v1 grouping is gone.
        assert!(parsed["repo"].get("workbranches").is_none());
        assert!(parsed["repo"]["worktrees"][0].get("workbranch").is_none());
        // `repos` is always present (empty here).
        assert!(parsed["repos"].as_array().unwrap().is_empty());
    }

    #[test]
    fn per_branch_files_are_capped_with_a_real_total() {
        let mut snap = snapshot();
        snap.worktrees[1].touched = (0..60)
            .map(|i| FileChange {
                path: format!("src/f{i:02}.rs"),
                kind: ChangeKind::Modified,
            })
            .collect();
        let report = Report::repo(&snap);
        let task = &report.repo.as_ref().unwrap().branches[2];
        assert_eq!(task.files.len(), MAX_REPORT_FILES);
        assert_eq!(task.files_total, 60);
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
        assert_eq!(parsed["repos"][0]["branches"][0]["role"], "trunk");
        assert!(parsed.get("repo").is_none());
    }
}
