//! Pure tree state + flattening for the branch-first view.
//!
//! The tree IS the branch hierarchy: repo → trunk → workbranches → task
//! branches, with each branch's worktree, agent, and changed files as
//! attributes/children. This module is pure data — the renderer turns
//! [`TreeRow`]s into styled lines, and the reducer drives [`TreeState`].

use std::collections::{HashMap, HashSet};

use crate::git::{BranchNode, BranchRole, FileChange, RepoSnapshot, WorktreeRecord};
use crate::process::ProcessInfo;

/// Cap on file rows shown under one branch; the remainder folds into an
/// overflow row so a huge diff can't swamp the tree.
pub const MAX_FILE_ROWS: usize = 30;

/// Stable identity for a tree node. Branch nodes are keyed by repo + branch
/// NAME (not worktree path), so expansion/selection/flashes survive snapshot
/// refreshes and worktree add/remove. `repo` is the normalized common git dir
/// (globally unique across repos — see `home::snapshot::repo_key`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeId {
    Repo {
        repo: String,
    },
    Branch {
        repo: String,
        branch: String,
    },
    /// A worktree with no branch (detached HEAD / unknown), keyed by path.
    Detached {
        repo: String,
        path: String,
    },
    /// A changed file under a branch; `path` is repo-relative.
    File {
        repo: String,
        branch: String,
        path: String,
    },
}

impl NodeId {
    /// The string key used in the `Transitions` flash map. Kept in lockstep
    /// with node identity so a flash raised for a branch lands on its row.
    pub fn flash_key(&self) -> String {
        const SEP: char = '\u{1f}';
        match self {
            NodeId::Repo { repo } => format!("r:{repo}"),
            NodeId::Branch { repo, branch } => format!("b:{repo}{SEP}{branch}"),
            NodeId::Detached { repo, path } => format!("d:{repo}{SEP}{path}"),
            NodeId::File { repo, branch, path } => {
                format!("f:{repo}{SEP}{branch}{SEP}{path}")
            }
        }
    }
}

/// What one visible row is.
#[derive(Debug)]
pub enum RowKind<'a> {
    Repo {
        snap: &'a RepoSnapshot,
        branch_total: usize,
        agent_total: usize,
    },
    Branch {
        node: &'a BranchNode,
        worktree_path: Option<&'a str>,
        agent: Option<&'a ProcessInfo>,
        /// Merge-conflict risks this branch's worktree participates in.
        risk: usize,
        /// Shown but inactive (only possible when `show_all` is on).
        dimmed: bool,
        is_current: bool,
    },
    /// A worktree with no branch (detached HEAD or unparsable).
    Detached {
        wt: &'a WorktreeRecord,
        agent: Option<&'a ProcessInfo>,
    },
    File {
        file: &'a FileChange,
    },
    /// "+N more files" — the tail of a capped file list.
    FileOverflow {
        hidden: usize,
    },
}

/// One visible row of the flattened tree.
#[derive(Debug)]
pub struct TreeRow<'a> {
    pub id: NodeId,
    /// Nesting level: repo = 0, root branches = 1, their children = 2, …
    pub depth: u8,
    /// Per-ancestor "a sibling follows at that level" flags, for │ guides.
    /// Excludes this row's own connector (see `is_last_child`).
    pub guides: Vec<bool>,
    pub is_last_child: bool,
    pub expandable: bool,
    pub expanded: bool,
    pub kind: RowKind<'a>,
}

/// Expansion/selection state, owned by the reducer. Stable across snapshot
/// refreshes (keyed by [`NodeId`]).
#[derive(Debug, Default)]
pub struct TreeState {
    /// Explicit user expand/collapse choices, overriding the defaults.
    expanded_overrides: HashMap<NodeId, bool>,
    /// Selected node, re-resolved against the flattened rows each frame.
    pub selected: Option<NodeId>,
    /// Fallback row index used when the selected node vanished.
    last_index: usize,
    /// Show all local branches (true) or active-only (false, the default).
    pub show_all: bool,
}

impl TreeState {
    pub fn is_expanded(&self, id: &NodeId, default: bool) -> bool {
        *self.expanded_overrides.get(id).unwrap_or(&default)
    }

    pub fn set_expanded(&mut self, id: NodeId, expanded: bool) {
        self.expanded_overrides.insert(id, expanded);
    }

    /// Drop overrides for nodes that no longer exist (bounded memory).
    /// `live` must be the EXISTENCE set ([`live_ids`]), never the visible
    /// rows — pruning by visibility would reset user fold state for nodes
    /// that are merely collapsed away, filtered out, or inactive.
    pub fn retain_ids(&mut self, live: &HashSet<NodeId>) {
        self.expanded_overrides.retain(|id, _| live.contains(id));
    }

    /// Resolve the selection to a row index: the selected node if it still
    /// exists, else the nearest remembered position. `None` when empty.
    pub fn selected_index(&self, rows: &[TreeRow]) -> Option<usize> {
        if rows.is_empty() {
            return None;
        }
        if let Some(sel) = &self.selected
            && let Some(i) = rows.iter().position(|r| &r.id == sel)
        {
            return Some(i);
        }
        Some(self.last_index.min(rows.len() - 1))
    }

    /// Set the selection to the row at `index`.
    pub fn select_index(&mut self, rows: &[TreeRow], index: usize) {
        if let Some(row) = rows.get(index) {
            self.selected = Some(row.id.clone());
            self.last_index = index;
        }
    }

    pub fn move_selection(&mut self, rows: &[TreeRow], delta: i32) {
        let Some(cur) = self.selected_index(rows) else {
            return;
        };
        let next = (cur as i32 + delta).clamp(0, rows.len() as i32 - 1) as usize;
        self.select_index(rows, next);
    }

    /// `l` / `→`: expand a collapsed node; on an expanded (or leaf) node, step
    /// into the first child if there is one.
    pub fn expand_selected(&mut self, rows: &[TreeRow]) {
        let Some(i) = self.selected_index(rows) else {
            return;
        };
        let row = &rows[i];
        if row.expandable && !row.expanded {
            self.set_expanded(row.id.clone(), true);
            self.selected = Some(row.id.clone());
            self.last_index = i;
            return;
        }
        // Step into the first child (the next row, iff it nests deeper).
        if let Some(next) = rows.get(i + 1)
            && next.depth > row.depth
        {
            self.select_index(rows, i + 1);
        }
    }

    /// `h` / `←`: collapse an expanded node; on a collapsed/leaf node, jump to
    /// its parent.
    pub fn collapse_selected(&mut self, rows: &[TreeRow]) {
        let Some(i) = self.selected_index(rows) else {
            return;
        };
        let row = &rows[i];
        if row.expandable && row.expanded {
            self.set_expanded(row.id.clone(), false);
            self.selected = Some(row.id.clone());
            self.last_index = i;
            return;
        }
        let depth = row.depth;
        if let Some(parent) = (0..i).rev().find(|&j| rows[j].depth < depth) {
            self.select_index(rows, parent);
        }
    }

    /// `Enter` / `Space`: toggle expansion on expandable nodes.
    pub fn toggle_selected(&mut self, rows: &[TreeRow]) {
        let Some(i) = self.selected_index(rows) else {
            return;
        };
        let row = &rows[i];
        if row.expandable {
            self.set_expanded(row.id.clone(), !row.expanded);
            self.selected = Some(row.id.clone());
            self.last_index = i;
        }
    }
}

/// Every expandable node id that EXISTS in the given snapshots — independent
/// of expansion, the name filter, and the active-only scope. This is the set
/// expansion overrides are retained against (only repo and branch nodes are
/// expandable, so only their ids matter).
pub fn live_ids(repos: &[RepoSnapshot]) -> HashSet<NodeId> {
    let mut ids = HashSet::new();
    for snap in repos {
        let repo = crate::home::snapshot::repo_key(snap);
        ids.insert(NodeId::Repo { repo: repo.clone() });
        for n in &snap.hierarchy.nodes {
            ids.insert(NodeId::Branch {
                repo: repo.clone(),
                branch: n.name.clone(),
            });
        }
    }
    ids
}

/// Flatten one or more repos into visible rows, honoring active-only
/// filtering, the branch-name filter, and expansion state.
pub fn flatten<'a>(
    repos: &'a [RepoSnapshot],
    state: &TreeState,
    filter: Option<&str>,
) -> Vec<TreeRow<'a>> {
    let query = filter.map(str::to_lowercase);
    let mut rows = Vec::new();
    for snap in repos {
        flatten_repo(snap, state, query.as_deref(), &mut rows);
    }
    rows
}

fn flatten_repo<'a>(
    snap: &'a RepoSnapshot,
    state: &TreeState,
    query: Option<&str>,
    rows: &mut Vec<TreeRow<'a>>,
) {
    let repo = crate::home::snapshot::repo_key(snap);
    let h = &snap.hierarchy;

    // Children by parent name, preserving hierarchy (depth-first) order.
    let mut children: HashMap<Option<&str>, Vec<&BranchNode>> = HashMap::new();
    for n in &h.nodes {
        children.entry(n.parent.as_deref()).or_default().push(n);
    }

    // A branch is visible when it passes the active filter and (its name, or
    // any visible descendant's name, matches the query). `is_active` already
    // keeps ancestors of active branches alive.
    let visible = |node: &BranchNode| -> bool {
        (state.show_all || h.is_active(node)) && subtree_matches(node, &children, query)
    };

    let detached: Vec<usize> = snap
        .worktrees
        .iter()
        .enumerate()
        .filter(|(_, w)| !w.bare && w.branch_short().is_none())
        .filter(|(_, w)| match query {
            None => true,
            Some(q) => {
                w.display_name().to_lowercase().contains(q) || w.path.to_lowercase().contains(q)
            }
        })
        .map(|(i, _)| i)
        .collect();

    // Top level under the repo: trunk first, then trunk's children rendered as
    // its SIBLINGS (the approved layout — everything is off trunk, so nesting
    // the whole tree under `main` would waste an indent level), then any
    // parentless branches. A node whose parent isn't among the nodes (which a
    // bug — never real data — could produce) is treated as a root rather than
    // silently dropped.
    let names: HashSet<&str> = h.nodes.iter().map(|n| n.name.as_str()).collect();
    let is_root = |n: &BranchNode| match n.parent.as_deref() {
        None => true,
        Some(p) => !names.contains(p),
    };
    let mut roots: Vec<&BranchNode> = Vec::new();
    for n in h.nodes.iter().filter(|n| is_root(n) && visible(n)) {
        roots.push(n);
        if Some(n.name.as_str()) == h.trunk.as_deref()
            && let Some(kids) = children.get(&Some(n.name.as_str()))
        {
            roots.extend(kids.iter().copied().filter(|k| visible(k)));
        }
    }

    let repo_id = NodeId::Repo { repo: repo.clone() };
    let expandable = !roots.is_empty() || !detached.is_empty();
    let expanded = state.is_expanded(&repo_id, true);
    let agent_total = (0..snap.worktrees.len())
        .filter(|&i| snap.processes.worktree_is_active(i))
        .count();
    rows.push(TreeRow {
        id: repo_id,
        depth: 0,
        guides: Vec::new(),
        is_last_child: true,
        expandable,
        expanded,
        kind: RowKind::Repo {
            snap,
            branch_total: h.nodes.len(),
            agent_total,
        },
    });
    if !expanded {
        return;
    }

    let total_roots = roots.len() + detached.len();
    for (i, node) in roots.iter().enumerate() {
        let is_last = i + 1 == total_roots;
        // Trunk's branch children were hoisted to the top level above, so the
        // trunk row itself only expands into its own files.
        let hoisted = Some(node.name.as_str()) == h.trunk.as_deref();
        walk_branch(
            snap,
            &repo,
            node,
            &children,
            &visible,
            state,
            Vec::new(),
            is_last,
            hoisted,
            rows,
        );
    }
    for (j, &idx) in detached.iter().enumerate() {
        let wt = &snap.worktrees[idx];
        let is_last = roots.len() + j + 1 == total_roots;
        rows.push(TreeRow {
            id: NodeId::Detached {
                repo: repo.clone(),
                path: wt.path.clone(),
            },
            depth: 1,
            guides: Vec::new(),
            is_last_child: is_last,
            expandable: false,
            expanded: false,
            kind: RowKind::Detached {
                wt,
                agent: snap.processes.agent_for_worktree(idx),
            },
        });
    }
}

/// The branch's own name, or any descendant's, matches the query.
fn subtree_matches(
    node: &BranchNode,
    children: &HashMap<Option<&str>, Vec<&BranchNode>>,
    query: Option<&str>,
) -> bool {
    let Some(q) = query else {
        return true;
    };
    if node.name.to_lowercase().contains(q) {
        return true;
    }
    children
        .get(&Some(node.name.as_str()))
        .is_some_and(|kids| kids.iter().any(|k| subtree_matches(k, children, query)))
}

#[allow(clippy::too_many_arguments)] // internal recursion carrying display context
fn walk_branch<'a>(
    snap: &'a RepoSnapshot,
    repo: &str,
    node: &'a BranchNode,
    children: &HashMap<Option<&str>, Vec<&'a BranchNode>>,
    visible: &dyn Fn(&BranchNode) -> bool,
    state: &TreeState,
    guides: Vec<bool>,
    is_last: bool,
    children_hoisted: bool,
    rows: &mut Vec<TreeRow<'a>>,
) {
    let id = NodeId::Branch {
        repo: repo.to_string(),
        branch: node.name.clone(),
    };

    let kids: Vec<&BranchNode> = if children_hoisted {
        Vec::new()
    } else {
        children
            .get(&Some(node.name.as_str()))
            .map(|v| v.iter().copied().filter(|n| visible(n)).collect())
            .unwrap_or_default()
    };
    let files: &[FileChange] = node
        .worktree
        .and_then(|i| snap.worktrees.get(i))
        .map(|w| w.touched.as_slice())
        .unwrap_or(&[]);

    let default_expanded = match node.role {
        BranchRole::Trunk | BranchRole::Workbranch => true,
        BranchRole::Task | BranchRole::Standalone => node.worktree.is_some(),
    };
    let expandable = !kids.is_empty() || !files.is_empty();
    let expanded = state.is_expanded(&id, default_expanded);

    let worktree_path = node
        .worktree
        .and_then(|i| snap.worktrees.get(i))
        .map(|w| w.path.as_str());
    let (agent, risk) = match node.worktree {
        Some(i) => (
            snap.processes.agent_for_worktree(i),
            crate::collision::count_for_worktree(&snap.collisions, i),
        ),
        None => (None, 0),
    };
    let is_current = node.worktree.is_some() && node.worktree == snap.current_worktree_index();

    rows.push(TreeRow {
        id,
        depth: (guides.len() + 1) as u8,
        guides: guides.clone(),
        is_last_child: is_last,
        expandable,
        expanded,
        kind: RowKind::Branch {
            node,
            worktree_path,
            agent,
            risk,
            dimmed: !snap.hierarchy.is_active(node),
            is_current,
        },
    });
    if !expanded {
        return;
    }

    let shown_files = files.len().min(MAX_FILE_ROWS);
    let overflow = files.len() - shown_files;
    let child_total = kids.len() + shown_files + usize::from(overflow > 0);

    let mut child_guides = guides;
    child_guides.push(!is_last);

    for (i, kid) in kids.iter().enumerate() {
        let last = i + 1 == child_total;
        walk_branch(
            snap,
            repo,
            kid,
            children,
            visible,
            state,
            child_guides.clone(),
            last,
            false,
            rows,
        );
    }
    for (j, file) in files.iter().take(shown_files).enumerate() {
        let last = kids.len() + j + 1 == child_total;
        rows.push(TreeRow {
            id: NodeId::File {
                repo: repo.to_string(),
                branch: node.name.clone(),
                path: file.path.clone(),
            },
            depth: (child_guides.len() + 1) as u8,
            guides: child_guides.clone(),
            is_last_child: last,
            expandable: false,
            expanded: false,
            kind: RowKind::File { file },
        });
    }
    if overflow > 0 {
        rows.push(TreeRow {
            id: NodeId::File {
                repo: repo.to_string(),
                branch: node.name.clone(),
                path: "\u{1f}more".to_string(),
            },
            depth: (child_guides.len() + 1) as u8,
            guides: child_guides,
            is_last_child: true,
            expandable: false,
            expanded: false,
            kind: RowKind::FileOverflow { hidden: overflow },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{BranchHierarchy, BranchLifecycle, ChangeKind, RepoIdentity, WorktreeRecord};
    use crate::process::ProcessSnapshot;

    fn bnode(name: &str, role: BranchRole, parent: Option<&str>) -> BranchNode {
        BranchNode {
            name: name.to_string(),
            oid: format!("oid-{name}"),
            role,
            parent: parent.map(str::to_string),
            ahead_of_parent: 1,
            behind_parent: Some(0),
            merged_into_parent: false,
            upstream: None,
            ahead: None,
            behind: None,
            upstream_gone: false,
            committer_date: None,
            lifecycle: BranchLifecycle::Committed,
            worktree: None,
        }
    }

    fn snap(nodes: Vec<BranchNode>, worktrees: Vec<WorktreeRecord>) -> RepoSnapshot {
        RepoSnapshot {
            repo: RepoIdentity {
                start_dir: "/repo".into(),
                root: "/repo".into(),
                git_dir: "/repo/.git".into(),
                common_git_dir: "/repo/.git".into(),
                is_worktree: false,
            },
            base: None,
            worktrees,
            branches: Vec::new(),
            hierarchy: BranchHierarchy {
                trunk: Some("main".to_string()),
                nodes,
                approximate: false,
            },
            collisions: Vec::new(),
            processes: ProcessSnapshot::default(),
            captured_at: 0,
        }
    }

    /// main → wb → (feat/a with worktree+files, feat/b inactive-merged)
    fn fixture() -> RepoSnapshot {
        let mut trunk = bnode("main", BranchRole::Trunk, None);
        trunk.ahead_of_parent = 0;
        let wb = bnode("emmett/wb-1", BranchRole::Workbranch, Some("main"));
        let mut a = bnode("feat/a", BranchRole::Task, Some("emmett/wb-1"));
        a.worktree = Some(0);
        let mut b = bnode("feat/b", BranchRole::Task, Some("emmett/wb-1"));
        b.ahead_of_parent = 0;
        b.merged_into_parent = true;
        b.lifecycle = BranchLifecycle::Merged;
        let wt = WorktreeRecord {
            path: "/repo-feat-a".into(),
            branch: Some("refs/heads/feat/a".into()),
            touched: vec![
                FileChange {
                    path: "src/x.rs".into(),
                    kind: ChangeKind::Modified,
                },
                FileChange {
                    path: "src/y.rs".into(),
                    kind: ChangeKind::Added,
                },
            ],
            ..Default::default()
        };
        snap(vec![trunk, wb, a, b], vec![wt])
    }

    fn names(rows: &[TreeRow]) -> Vec<String> {
        rows.iter()
            .map(|r| match &r.id {
                NodeId::Repo { .. } => "(repo)".to_string(),
                NodeId::Branch { branch, .. } => branch.clone(),
                NodeId::Detached { path, .. } => format!("detached:{path}"),
                NodeId::File { path, .. } => format!("file:{path}"),
            })
            .collect()
    }

    #[test]
    fn flattens_active_only_with_files() {
        let s = fixture();
        let state = TreeState::default();
        let rows = flatten(std::slice::from_ref(&s), &state, None);
        // feat/b is merged+inactive → hidden by default; feat/a expands its files.
        assert_eq!(
            names(&rows),
            vec![
                "(repo)",
                "main",
                "emmett/wb-1",
                "feat/a",
                "file:src/x.rs",
                "file:src/y.rs"
            ]
        );
        // Depths: repo 0, main/wb 1, feat/a 2, files 3.
        let depths: Vec<u8> = rows.iter().map(|r| r.depth).collect();
        assert_eq!(depths, vec![0, 1, 1, 2, 3, 3]);
    }

    #[test]
    fn show_all_reveals_inactive_dimmed() {
        let s = fixture();
        let state = TreeState {
            show_all: true,
            ..Default::default()
        };
        let rows = flatten(std::slice::from_ref(&s), &state, None);
        let b = rows
            .iter()
            .find(|r| matches!(&r.id, NodeId::Branch { branch, .. } if branch == "feat/b"))
            .expect("feat/b visible under show_all");
        match &b.kind {
            RowKind::Branch { dimmed, .. } => assert!(dimmed),
            _ => panic!("expected branch row"),
        }
    }

    #[test]
    fn filter_keeps_ancestors_of_matches() {
        let s = fixture();
        let state = TreeState::default();
        let rows = flatten(std::slice::from_ref(&s), &state, Some("feat/a"));
        let n = names(&rows);
        assert!(n.contains(&"emmett/wb-1".to_string()), "ancestor kept");
        assert!(n.contains(&"feat/a".to_string()));
        assert!(
            !n.contains(&"feat/b".to_string()),
            "non-matching siblings hidden"
        );
    }

    #[test]
    fn fold_state_survives_pruning_while_hidden() {
        // Regression (review finding): collapsing a branch, then HIDING it
        // (collapse its ancestor / filter / active-only) and pruning must NOT
        // reset its fold — the node still exists.
        let s = fixture();
        let mut state = TreeState::default();
        let rows = flatten(std::slice::from_ref(&s), &state, None);
        let a_idx = rows
            .iter()
            .position(|r| matches!(&r.id, NodeId::Branch { branch, .. } if branch == "feat/a"))
            .unwrap();
        state.select_index(&rows, a_idx);
        state.collapse_selected(&rows); // fold feat/a's files away

        // Collapse the repo row → feat/a becomes invisible.
        let rows = flatten(std::slice::from_ref(&s), &state, None);
        state.select_index(&rows, 0);
        state.collapse_selected(&rows);

        // The snapshot-refresh prune: existence-based, so the fold survives.
        state.retain_ids(&live_ids(std::slice::from_ref(&s)));

        // Re-expand the repo: feat/a must still be folded (no file rows).
        let rows = flatten(std::slice::from_ref(&s), &state, None);
        state.select_index(&rows, 0);
        state.expand_selected(&rows);
        let rows = flatten(std::slice::from_ref(&s), &state, None);
        assert!(
            !names(&rows).iter().any(|n| n.starts_with("file:")),
            "feat/a's collapse override must survive being hidden"
        );
    }

    #[test]
    fn filter_applies_to_detached_worktrees_too() {
        let mut s = fixture();
        s.worktrees.push(WorktreeRecord {
            path: "/repo-det".into(),
            detached: true,
            head: Some("abcdef12".into()),
            ..Default::default()
        });
        let state = TreeState::default();
        let rows = flatten(std::slice::from_ref(&s), &state, Some("feat/a"));
        assert!(
            !rows
                .iter()
                .any(|r| matches!(r.kind, RowKind::Detached { .. })),
            "a non-matching detached worktree is filtered out"
        );
        let rows = flatten(std::slice::from_ref(&s), &state, Some("detached"));
        assert!(
            rows.iter()
                .any(|r| matches!(r.kind, RowKind::Detached { .. })),
            "a matching detached worktree stays"
        );
    }

    #[test]
    fn collapse_override_hides_children_and_survives_reflatten() {
        let s = fixture();
        let mut state = TreeState::default();
        let rows = flatten(std::slice::from_ref(&s), &state, None);
        let a_idx = rows
            .iter()
            .position(|r| matches!(&r.id, NodeId::Branch { branch, .. } if branch == "feat/a"))
            .unwrap();
        state.select_index(&rows, a_idx);
        state.collapse_selected(&rows);
        let rows = flatten(std::slice::from_ref(&s), &state, None);
        assert!(
            !names(&rows).iter().any(|n| n.starts_with("file:")),
            "files hidden after collapse"
        );
        // The override persists across a fresh flatten (same ids).
        let rows = flatten(std::slice::from_ref(&s), &state, None);
        assert!(!names(&rows).iter().any(|n| n.starts_with("file:")));
    }

    #[test]
    fn selection_falls_back_to_nearest_row_when_node_vanishes() {
        let s = fixture();
        let mut state = TreeState::default();
        let rows = flatten(std::slice::from_ref(&s), &state, None);
        state.select_index(&rows, rows.len() - 1); // last file row
        // The branch's worktree disappears → file rows vanish.
        let mut s2 = fixture();
        s2.hierarchy.nodes[2].worktree = None;
        s2.worktrees.clear();
        let rows2 = flatten(std::slice::from_ref(&s2), &state, None);
        let idx = state.selected_index(&rows2).expect("fallback selection");
        assert!(idx < rows2.len());
    }

    #[test]
    fn expand_steps_into_first_child_and_collapse_jumps_to_parent() {
        let s = fixture();
        let mut state = TreeState::default();
        let rows = flatten(std::slice::from_ref(&s), &state, None);
        let a_idx = rows
            .iter()
            .position(|r| matches!(&r.id, NodeId::Branch { branch, .. } if branch == "feat/a"))
            .unwrap();
        state.select_index(&rows, a_idx);
        // Already expanded → step into first child (file row).
        state.expand_selected(&rows);
        assert!(matches!(state.selected, Some(NodeId::File { .. })));
        // Leaf → collapse jumps to the parent branch.
        state.collapse_selected(&rows);
        assert!(
            matches!(&state.selected, Some(NodeId::Branch { branch, .. }) if branch == "feat/a")
        );
    }

    #[test]
    fn file_rows_cap_with_overflow() {
        let mut s = fixture();
        s.worktrees[0].touched = (0..40)
            .map(|i| FileChange {
                path: format!("src/f{i:02}.rs"),
                kind: ChangeKind::Modified,
            })
            .collect();
        let state = TreeState::default();
        let rows = flatten(std::slice::from_ref(&s), &state, None);
        let file_rows = rows
            .iter()
            .filter(|r| matches!(r.kind, RowKind::File { .. }))
            .count();
        assert_eq!(file_rows, MAX_FILE_ROWS);
        assert!(
            rows.iter()
                .any(|r| matches!(r.kind, RowKind::FileOverflow { hidden: 10 }))
        );
    }

    #[test]
    fn detached_worktrees_appear_under_the_repo() {
        let mut s = fixture();
        s.worktrees.push(WorktreeRecord {
            path: "/repo-det".into(),
            detached: true,
            head: Some("abcdef12".into()),
            ..Default::default()
        });
        let state = TreeState::default();
        let rows = flatten(std::slice::from_ref(&s), &state, None);
        let det = rows
            .iter()
            .find(|r| matches!(r.kind, RowKind::Detached { .. }))
            .expect("detached row");
        assert_eq!(det.depth, 1);
        assert!(det.is_last_child);
    }

    #[test]
    fn flash_keys_are_distinct_per_node_kind() {
        let b = NodeId::Branch {
            repo: "r".into(),
            branch: "feat/a".into(),
        };
        let f = NodeId::File {
            repo: "r".into(),
            branch: "feat/a".into(),
            path: "x".into(),
        };
        assert_ne!(b.flash_key(), f.flash_key());
    }
}
