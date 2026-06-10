//! Branch hierarchy: which branch forked from which, derived from Git facts.
//!
//! Git's rule is that a branch is checked out in at most ONE worktree, so the
//! real shape of parallel agent work is a branch tree — trunk → daily
//! workbranch (`<dev>/wb-<date>`) → task branches — with worktrees and agents
//! as attributes of branches. This module derives that tree from commit
//! topology (one batched `rev-list` over the off-trunk history), using the
//! team naming convention only as a tie-break hint. Repos that don't follow
//! the convention degrade gracefully: everything hangs off trunk as
//! standalone branches, and a repo with no recognizable trunk yields a flat
//! list — never an error.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use super::commands::run_git;
use super::lifecycle::{BranchLifecycle, LifecycleInputs, derive};
use super::refs::BranchInfo;
use super::repo::RepoIdentity;
use super::worktree::WorktreeRecord;

/// Cap on the off-trunk commits walked per topology capture. Branches live
/// ≤2 days under the team flow, so the off-trunk graph is normally tiny; the
/// cap only matters for pathological repos, which degrade to `approximate`.
const MAX_TOPOLOGY_COMMITS: usize = 10_000;

/// Cap on the extra `rev-list --left-right --count` calls used to fill
/// `behind_parent` for direct trunk children.
const MAX_BEHIND_QUERIES: usize = 16;

/// What tier of the hierarchy a branch occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchRole {
    /// The integration trunk (`main`/`master`/the remote default branch).
    Trunk,
    /// A daily integration branch (`<dev>/wb-<date>` by convention).
    Workbranch,
    /// A task branch parented by a workbranch (or another task branch when
    /// stacked).
    Task,
    /// A direct child of trunk that isn't a workbranch (hotfixes, repos not
    /// using the convention).
    Standalone,
}

impl BranchRole {
    pub fn as_str(self) -> &'static str {
        match self {
            BranchRole::Trunk => "trunk",
            BranchRole::Workbranch => "workbranch",
            BranchRole::Task => "task",
            BranchRole::Standalone => "standalone",
        }
    }
}

/// One local branch placed in the hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchNode {
    /// Short name, e.g. `feat/csv-export-142`.
    pub name: String,
    /// Tip commit oid.
    pub oid: String,
    pub role: BranchRole,
    /// Short name of the parent branch; `None` for trunk (or every branch when
    /// no trunk exists).
    pub parent: Option<String>,
    /// Commits on this branch not reachable from its parent.
    pub ahead_of_parent: u32,
    /// Commits on the parent not reachable from this branch ("needs rebase"
    /// when > 0). `None` when the bounded extra query was skipped or failed.
    /// For direct trunk children this is measured against the REMOTE trunk tip
    /// (the rebase target under the team flow), which can differ from the
    /// local trunk when local commits haven't been pushed.
    pub behind_parent: Option<u32>,
    /// Fully contained in the parent (and not just a fresh, workless cut).
    pub merged_into_parent: bool,
    /// Upstream tracking, copied from the ref scan so the node is
    /// self-contained for lifecycle/report purposes.
    pub upstream: Option<String>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub upstream_gone: bool,
    /// Committer date of the tip (ISO-8601 strict), for ordering.
    pub committer_date: Option<String>,
    /// Lifecycle stage — filled by [`BranchHierarchy::finalize`].
    pub lifecycle: BranchLifecycle,
    /// Index into the snapshot's worktree list when checked out — filled by
    /// [`BranchHierarchy::finalize`].
    pub worktree: Option<usize>,
}

/// The derived branch tree for one repository, in depth-first order (trunk
/// first, each child followed by its subtree).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchHierarchy {
    /// Local short name of the trunk branch, if one was recognized.
    pub trunk: Option<String>,
    pub nodes: Vec<BranchNode>,
    /// True when the topology walk failed or was capped — parentage degraded
    /// to "everything standalone under trunk".
    pub approximate: bool,
}

impl BranchHierarchy {
    /// Look up a node by branch short name.
    pub fn node(&self, name: &str) -> Option<&BranchNode> {
        self.nodes.iter().find(|n| n.name == name)
    }

    /// Direct children of `name`, in hierarchy (depth-first) order.
    pub fn children_of<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a BranchNode> {
        self.nodes
            .iter()
            .filter(move |n| n.parent.as_deref() == Some(name))
    }

    /// Whether a branch matters right now: trunk always; otherwise a checked
    /// out worktree, unmerged work vs the parent, unpushed work vs the
    /// upstream, or any active descendant (a drained workbranch stays visible
    /// while its tasks are live).
    pub fn is_active(&self, node: &BranchNode) -> bool {
        if self.self_active(node) {
            return true;
        }
        self.children_of(&node.name).any(|c| self.is_active(c))
    }

    fn self_active(&self, node: &BranchNode) -> bool {
        matches!(node.role, BranchRole::Trunk)
            || node.worktree.is_some()
            || (node.ahead_of_parent > 0 && node.lifecycle != BranchLifecycle::Merged)
            || node.ahead.unwrap_or(0) > 0
    }

    /// Count of nodes passing [`Self::is_active`].
    pub fn active_count(&self) -> usize {
        self.nodes.iter().filter(|n| self.is_active(n)).count()
    }

    /// Resolve each node's worktree index and lifecycle against the captured
    /// worktrees. Called once per snapshot — lifecycle is never cached, since
    /// statuses change without any oid moving. Upstream-tracking facts are
    /// refreshed from the CURRENT ref scan here too: `git branch
    /// --set-upstream-to`/`--unset-upstream` changes them without moving any
    /// ref, so the cached skeleton's copies can be stale.
    pub fn finalize(mut self, branches: &[BranchInfo], worktrees: &[WorktreeRecord]) -> Self {
        use crate::util::paths::normalize;
        let wt_by_path: HashMap<String, usize> = worktrees
            .iter()
            .enumerate()
            .map(|(i, w)| (normalize(&w.path), i))
            .collect();
        let local_by_name: HashMap<&str, &BranchInfo> = branches
            .iter()
            .filter(|b| !b.is_remote)
            .map(|b| (b.short.as_str(), b))
            .collect();
        let oid_by_name: HashMap<String, String> = self
            .nodes
            .iter()
            .map(|n| (n.name.clone(), n.oid.clone()))
            .collect();

        for node in &mut self.nodes {
            let info = local_by_name.get(node.name.as_str());
            if let Some(info) = info {
                node.upstream = info.upstream.clone();
                node.ahead = info.ahead;
                node.behind = info.behind;
                node.upstream_gone = info.upstream_gone;
            }
            node.worktree = info
                .and_then(|b| b.worktree_path.as_deref())
                .and_then(|p| wt_by_path.get(&normalize(p)))
                .copied();
            let dirty = node
                .worktree
                .and_then(|i| worktrees.get(i))
                .and_then(|w| w.status.as_ref())
                .is_some_and(|s| !s.clean);
            let parent_oid = node.parent.as_ref().and_then(|p| oid_by_name.get(p));
            node.lifecycle = derive(LifecycleInputs {
                dirty,
                has_upstream: node.upstream.is_some(),
                upstream_gone: node.upstream_gone,
                ahead: node.ahead,
                ahead_of_parent: node.ahead_of_parent,
                tip_equals_parent: parent_oid == Some(&node.oid),
                has_parent: node.parent.is_some(),
            });
        }
        self
    }
}

/// True for the team's workbranch naming convention: the last path segment
/// starts with `wb-` (e.g. `emmett/wb-2026-06-10`).
pub fn wb_named(short: &str) -> bool {
    short
        .rsplit('/')
        .next()
        .is_some_and(|seg| seg.starts_with("wb-"))
}

/// Compute the hierarchy skeleton for a repo (everything except worktree
/// indices and lifecycle — call [`BranchHierarchy::finalize`] for those).
/// Cached per repo, keyed by a fingerprint of every ref's (name, oid): any
/// commit, rebase, fetch, or branch create/delete invalidates it; the common
/// nothing-changed poll costs zero extra git processes.
pub async fn compute(repo: &RepoIdentity, branches: &[BranchInfo]) -> BranchHierarchy {
    let trunk = detect_trunk(branches);
    let print = fingerprint(branches, trunk.as_ref());
    let cache_key = crate::util::paths::normalize(&repo.common_git_dir.to_string_lossy());

    if let Some(hit) = cache_get(&cache_key, print) {
        return hit;
    }

    let tips = local_tips(branches);
    let skeleton = match &trunk {
        None => build_skeleton(&tips, None, &Graph::new(), false),
        Some(trunk) => {
            let tip_oids: Vec<&str> = {
                let mut seen = HashSet::new();
                tips.iter()
                    .map(|t| t.oid.as_str())
                    .filter(|o| seen.insert(*o))
                    .collect()
            };
            match fetch_graph(&repo.root, &tip_oids, &trunk.boundary_oid).await {
                Ok(graph) => {
                    // A walk that filled the cap was almost certainly
                    // truncated: reach sets (and therefore parents and
                    // ahead-counts) may be wrong, so say so.
                    let capped = graph.len() >= MAX_TOPOLOGY_COMMITS;
                    if capped {
                        tracing::warn!(
                            "off-trunk history hit the {MAX_TOPOLOGY_COMMITS}-commit cap — \
                             hierarchy marked approximate"
                        );
                    }
                    let mut sk =
                        build_skeleton(&tips, Some(trunk.local_short.as_str()), &graph, capped);
                    fill_trunk_child_behind(repo, &mut sk, &trunk.boundary_oid).await;
                    sk
                }
                Err(err) => {
                    tracing::warn!("branch topology unavailable, degrading: {err}");
                    build_skeleton(&tips, Some(trunk.local_short.as_str()), &Graph::new(), true)
                }
            }
        }
    };

    cache_put(cache_key, print, skeleton.clone());
    skeleton
}

// ---------------------------------------------------------------------------
// Trunk detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct TrunkInfo {
    /// The LOCAL trunk branch short name (the tree root).
    local_short: String,
    /// The oid used as the off-trunk boundary: the remote trunk tip when it
    /// exists (workbranches rebase onto origin/<trunk>, keeping the off-trunk
    /// graph tiny), else the local trunk tip.
    boundary_oid: String,
}

/// Recognize the trunk: prefer the remote default branch (the
/// `refs/remotes/origin/HEAD` symref target) when a matching LOCAL branch
/// exists, else `main`, else `master`.
fn detect_trunk(branches: &[BranchInfo]) -> Option<TrunkInfo> {
    // Prefer origin's HEAD symref specifically when several remotes carry one,
    // and take the LAST path segment — remote names may themselves contain
    // slashes (`work/origin/main` → branch `main`).
    let symref_local = branches
        .iter()
        .filter(|b| b.symref_target.is_some())
        .min_by_key(|b| b.full_ref != "refs/remotes/origin/HEAD")
        .and_then(|b| b.symref_target.as_deref())
        .and_then(|t| t.rsplit_once('/').map(|(_, name)| name.to_string()));
    let candidates: Vec<String> = symref_local
        .into_iter()
        .chain(["main".to_string(), "master".to_string()])
        .collect();

    for name in candidates {
        let Some(local) = branches.iter().find(|b| !b.is_remote && b.short == name) else {
            continue;
        };
        let boundary = branches
            .iter()
            .find(|b| b.is_remote && b.short == format!("origin/{name}"))
            .map_or_else(|| local.oid.clone(), |r| r.oid.clone());
        return Some(TrunkInfo {
            local_short: name,
            boundary_oid: boundary,
        });
    }
    None
}

// ---------------------------------------------------------------------------
// Topology
// ---------------------------------------------------------------------------

/// Off-trunk commit graph: oid → parent oids. Commits reachable from the
/// boundary are simply absent (a parent with no entry marks the trunk edge).
type Graph = HashMap<String, Vec<String>>;

/// One batched `rev-list` over every local tip, stopping at the boundary.
/// `--format` makes rev-list print a `commit <oid>` header line before each
/// formatted line; header lines carry no NUL, so they filter out cleanly —
/// no dependency on newer-git flags.
async fn fetch_graph(
    root: &std::path::Path,
    tip_oids: &[&str],
    boundary_oid: &str,
) -> color_eyre::eyre::Result<Graph> {
    use color_eyre::eyre::eyre;
    if tip_oids.is_empty() {
        return Ok(Graph::new());
    }
    let max = format!("--max-count={MAX_TOPOLOGY_COMMITS}");
    let mut args: Vec<&str> = vec![
        "rev-list",
        "--topo-order",
        max.as_str(),
        "--format=%H%x00%P",
    ];
    args.extend(tip_oids);
    args.push("--not");
    args.push(boundary_oid);

    let out = run_git(Some(root), &args).await?;
    if !out.success() {
        return Err(eyre!("rev-list failed: {}", out.stderr_str().trim()));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut graph = Graph::new();
    for line in text.lines() {
        let Some((oid, parents)) = line.split_once('\0') else {
            continue; // "commit <oid>" header line
        };
        graph.insert(
            oid.to_string(),
            parents.split_whitespace().map(str::to_string).collect(),
        );
    }
    Ok(graph)
}

/// Off-graph commits reachable from `tip` (including `tip` itself when it is
/// in the graph).
fn reach<'g>(tip: &str, graph: &'g Graph) -> HashSet<&'g str> {
    let mut seen: HashSet<&'g str> = HashSet::new();
    let Some((key, _)) = graph.get_key_value(tip) else {
        return seen;
    };
    let mut stack: Vec<&'g str> = vec![key];
    while let Some(oid) = stack.pop() {
        if !seen.insert(oid) {
            continue;
        }
        if let Some(parents) = graph.get(oid) {
            for p in parents {
                if let Some((pk, _)) = graph.get_key_value(p.as_str())
                    && !seen.contains(pk.as_str())
                {
                    stack.push(pk);
                }
            }
        }
    }
    seen
}

// ---------------------------------------------------------------------------
// Skeleton construction (pure — unit-tested on synthetic graphs)
// ---------------------------------------------------------------------------

/// A local branch tip with the ref-scan facts the hierarchy needs.
#[derive(Debug, Clone, Default)]
pub(crate) struct LocalTip {
    pub name: String,
    pub oid: String,
    pub committer_date: Option<String>,
    pub upstream: Option<String>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub upstream_gone: bool,
}

fn local_tips(branches: &[BranchInfo]) -> Vec<LocalTip> {
    branches
        .iter()
        .filter(|b| !b.is_remote && !b.short.is_empty())
        .map(|b| LocalTip {
            name: b.short.clone(),
            oid: b.oid.clone(),
            committer_date: b.committer_date.clone(),
            upstream: b.upstream.clone(),
            ahead: b.ahead,
            behind: b.behind,
            upstream_gone: b.upstream_gone,
        })
        .collect()
}

/// Pure core: place every local tip in the tree.
///
/// Parent selection per non-trunk branch `b` (reach = off-trunk commits
/// reachable from a tip):
/// 1. **Nearest strict ancestor** among the other local branches — minimizes
///    `|reach(b)| - |reach(p)|`, so stacked branches nest correctly.
/// 2. **Equal-oid tie** (fresh cut, tip == parent tip): direction comes from
///    the naming convention — the `wb-`-named side is the parent. Two
///    indistinguishable twins both fall to trunk; we never guess.
/// 3. **Diverged** (no strict ancestor but shared off-trunk history — the
///    un-rebased task whose workbranch advanced): the `wb-`-named branch is
///    the parent of the non-`wb-`-named one; deepest shared base wins.
/// 4. **Fallback**: trunk.
fn build_skeleton(
    tips: &[LocalTip],
    trunk: Option<&str>,
    graph: &Graph,
    approximate: bool,
) -> BranchHierarchy {
    let reaches: HashMap<&str, HashSet<&str>> = tips
        .iter()
        .map(|t| (t.name.as_str(), reach(&t.oid, graph)))
        .collect();

    let mut nodes: Vec<BranchNode> = Vec::with_capacity(tips.len());
    for b in tips {
        let is_trunk = Some(b.name.as_str()) == trunk;
        let (parent, ahead, behind) = if is_trunk || trunk.is_none() {
            (None, 0u32, None)
        } else {
            select_parent(b, tips, trunk, &reaches)
        };

        let role = if is_trunk {
            BranchRole::Trunk
        } else if wb_named(&b.name) {
            BranchRole::Workbranch
        } else if parent.as_deref() == trunk || parent.is_none() {
            BranchRole::Standalone
        } else {
            BranchRole::Task
        };

        let parent_oid = parent
            .as_ref()
            .and_then(|p| tips.iter().find(|t| &t.name == p))
            .map(|t| t.oid.as_str());
        nodes.push(BranchNode {
            name: b.name.clone(),
            oid: b.oid.clone(),
            role,
            merged_into_parent: parent.is_some()
                && ahead == 0
                && parent_oid != Some(b.oid.as_str()),
            parent,
            ahead_of_parent: ahead,
            behind_parent: behind,
            upstream: b.upstream.clone(),
            ahead: b.ahead,
            behind: b.behind,
            upstream_gone: b.upstream_gone,
            committer_date: b.committer_date.clone(),
            lifecycle: BranchLifecycle::default(),
            worktree: None,
        });
    }

    break_cycles(&mut nodes, trunk, &reaches);
    BranchHierarchy {
        trunk: trunk.map(str::to_string),
        nodes: depth_first(nodes, trunk),
        approximate,
    }
}

/// Defensive backstop: parent pointers must form a forest. The selection
/// rules cannot produce a cycle (rule 1 refuses to parent a wb-named branch
/// under a non-wb branch, and a DAG can't make two distinct tips mutual
/// ancestors), but a future rule change could — and a cycle silently drops
/// its members from the tree. Detect any cycle and cut it at its wb-named
/// (else first-seen) member, reattaching that member to trunk.
fn break_cycles(
    nodes: &mut [BranchNode],
    trunk: Option<&str>,
    reaches: &HashMap<&str, HashSet<&str>>,
) {
    let index_of: HashMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.name.clone(), i))
        .collect();
    let mut cleared: HashSet<usize> = HashSet::new(); // known cycle-free
    for start in 0..nodes.len() {
        let mut path: Vec<usize> = Vec::new();
        let mut on_path: HashSet<usize> = HashSet::new();
        let mut i = start;
        loop {
            if cleared.contains(&i) {
                break;
            }
            if on_path.contains(&i) {
                // Found a cycle: the members are `path` from `i` onward.
                let cycle_start = path.iter().position(|&j| j == i).unwrap_or(0);
                let members = &path[cycle_start..];
                let cut = members
                    .iter()
                    .copied()
                    .find(|&j| wb_named(&nodes[j].name))
                    .unwrap_or(members[0]);
                tracing::warn!(
                    "branch parent cycle detected ({}); reattaching {} to trunk",
                    members
                        .iter()
                        .map(|&j| nodes[j].name.as_str())
                        .collect::<Vec<_>>()
                        .join(" → "),
                    nodes[cut].name
                );
                let rb = &reaches[nodes[cut].name.as_str()];
                let ahead = match trunk.and_then(|t| reaches.get(t)) {
                    Some(rt) => rb.difference(rt).count() as u32,
                    None => rb.len() as u32,
                };
                nodes[cut].parent = trunk.map(str::to_string);
                nodes[cut].ahead_of_parent = ahead;
                nodes[cut].behind_parent = None;
                nodes[cut].merged_into_parent = trunk.is_some() && ahead == 0;
                if !wb_named(&nodes[cut].name) {
                    nodes[cut].role = BranchRole::Standalone;
                }
                break;
            }
            on_path.insert(i);
            path.push(i);
            match nodes[i].parent.as_ref().and_then(|p| index_of.get(p)) {
                Some(&next) => i = next,
                None => break,
            }
        }
        cleared.extend(path);
    }
}

/// Pick the parent for one non-trunk branch. Returns
/// `(parent_name, ahead_of_parent, behind_parent)`.
fn select_parent(
    b: &LocalTip,
    tips: &[LocalTip],
    trunk: Option<&str>,
    reaches: &HashMap<&str, HashSet<&str>>,
) -> (Option<String>, u32, Option<u32>) {
    let rb = &reaches[b.name.as_str()];

    // 1+2: strict ancestors and equal-oid ties, scored by distance.
    let mut best: Option<(&LocalTip, usize)> = None;
    for p in tips {
        if p.name == b.name {
            continue;
        }
        let candidate = if p.oid == b.oid {
            // Equal tips: only the convention can give a direction. Trunk
            // always wins it; otherwise the wb-named side is the parent.
            let p_is_trunk = Some(p.name.as_str()) == trunk;
            (p_is_trunk || (wb_named(&p.name) && !wb_named(&b.name))).then_some(0usize)
        } else if rb.contains(p.oid.as_str()) {
            // A workbranch that has MERGED one of its task branches sees that
            // task's tip as a strict ancestor — but the task is simultaneously
            // parented to the workbranch by the diverged rule, which would
            // form a parent CYCLE (and drop both from the tree). The
            // convention is authoritative here: a wb-named branch is never
            // parented by a non-wb, non-trunk branch.
            let p_is_trunk = Some(p.name.as_str()) == trunk;
            if wb_named(&b.name) && !wb_named(&p.name) && !p_is_trunk {
                None
            } else {
                Some(rb.len() - reaches[p.name.as_str()].len())
            }
        } else {
            None
        };
        if let Some(dist) = candidate {
            let better = match best {
                None => true,
                Some((cur, cur_dist)) => {
                    dist < cur_dist
                        || (dist == cur_dist
                            && (wb_named(&p.name) && !wb_named(&cur.name)
                                || (wb_named(&p.name) == wb_named(&cur.name) && p.name < cur.name)))
                }
            };
            if better {
                best = Some((p, dist));
            }
        }
    }
    if let Some((p, _)) = best {
        let rp = &reaches[p.name.as_str()];
        let ahead = rb.difference(rp).count() as u32;
        // A strict ancestor's reach is a subset of ours, so behind is exact 0
        // (equal-oid ties likewise). Trunk children get patched separately.
        let behind = if Some(p.name.as_str()) == trunk {
            None
        } else {
            Some(rp.difference(rb).count() as u32)
        };
        return (Some(p.name.clone()), ahead, behind);
    }

    // 3: diverged — shared off-trunk history with a wb-named branch.
    if !wb_named(&b.name) {
        let mut diverged: Option<(&LocalTip, usize)> = None;
        for p in tips {
            if p.name == b.name || !wb_named(&p.name) {
                continue;
            }
            let rp = &reaches[p.name.as_str()];
            let shared = rb.intersection(rp).count();
            if shared > 0 && diverged.is_none_or(|(_, s)| shared > s) {
                diverged = Some((p, shared));
            }
        }
        if let Some((p, _)) = diverged {
            let rp = &reaches[p.name.as_str()];
            let ahead = rb.difference(rp).count() as u32;
            let behind = rp.difference(rb).count() as u32;
            return (Some(p.name.clone()), ahead, Some(behind));
        }
    }

    // 4: fallback to trunk.
    let trunk_reach = trunk.and_then(|t| reaches.get(t));
    let ahead = match trunk_reach {
        Some(rt) => rb.difference(rt).count() as u32,
        None => rb.len() as u32,
    };
    (trunk.map(str::to_string), ahead, None)
}

/// Order nodes depth-first: trunk first, then under each parent — workbranches
/// before standalones, newest committer date first under trunk; task branches
/// by name. Unreachable nodes (defensive: a cycle could only come from a bug)
/// are appended at the end.
fn depth_first(nodes: Vec<BranchNode>, trunk: Option<&str>) -> Vec<BranchNode> {
    let index_of: HashMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.name.clone(), i))
        .collect();
    let mut children: HashMap<Option<String>, Vec<usize>> = HashMap::new();
    for (i, n) in nodes.iter().enumerate() {
        children.entry(n.parent.clone()).or_default().push(i);
    }
    for kids in children.values_mut() {
        kids.sort_by(|&a, &b| {
            let (na, nb) = (&nodes[a], &nodes[b]);
            let rank = |n: &BranchNode| match n.role {
                BranchRole::Trunk => 0u8,
                BranchRole::Workbranch => 1,
                BranchRole::Task => 2,
                BranchRole::Standalone => 3,
            };
            rank(na)
                .cmp(&rank(nb))
                .then_with(|| nb.committer_date.cmp(&na.committer_date)) // newest first
                .then_with(|| na.name.cmp(&nb.name))
        });
    }

    let mut order: Vec<usize> = Vec::with_capacity(nodes.len());
    let mut visited: HashSet<usize> = HashSet::new();
    let roots: Vec<usize> = match trunk.and_then(|t| index_of.get(t)) {
        Some(&t) => vec![t],
        None => children.get(&None).cloned().unwrap_or_default(),
    };
    let mut stack: Vec<usize> = roots.into_iter().rev().collect();
    while let Some(i) = stack.pop() {
        if !visited.insert(i) {
            continue;
        }
        order.push(i);
        if let Some(kids) = children.get(&Some(nodes[i].name.clone())) {
            for &k in kids.iter().rev() {
                if !visited.contains(&k) {
                    stack.push(k);
                }
            }
        }
    }
    for i in 0..nodes.len() {
        if !visited.contains(&i) {
            order.push(i);
        }
    }

    let mut slots: Vec<Option<BranchNode>> = nodes.into_iter().map(Some).collect();
    order
        .into_iter()
        .map(|i| slots[i].take().expect("each index ordered once"))
        .collect()
}

/// Fill `behind_parent` for direct trunk children via bounded
/// `rev-list --left-right --count <tip>...<boundary>` calls (the off-trunk
/// graph can't see trunk-side commits). Skipped beyond [`MAX_BEHIND_QUERIES`].
async fn fill_trunk_child_behind(
    repo: &RepoIdentity,
    skeleton: &mut BranchHierarchy,
    boundary_oid: &str,
) {
    let Some(trunk) = skeleton.trunk.clone() else {
        return;
    };
    let mut budget = MAX_BEHIND_QUERIES;
    for node in &mut skeleton.nodes {
        if node.parent.as_deref() != Some(trunk.as_str()) || budget == 0 {
            continue;
        }
        budget -= 1;
        let range = format!("{}...{}", node.oid, boundary_oid);
        let Ok(out) = run_git(
            Some(&repo.root),
            &["rev-list", "--left-right", "--count", &range],
        )
        .await
        else {
            continue;
        };
        if !out.success() {
            continue;
        }
        // Output: "<only-in-tip>\t<only-in-boundary>"
        let text = out.stdout_str();
        let mut it = text.split_whitespace();
        let (_left, right) = (it.next(), it.next());
        node.behind_parent = right.and_then(|r| r.parse().ok());
    }
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

struct CacheEntry {
    fingerprint: u64,
    skeleton: BranchHierarchy,
}

fn cache() -> &'static Mutex<HashMap<String, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_get(key: &str, fingerprint: u64) -> Option<BranchHierarchy> {
    let map = cache().lock().ok()?;
    map.get(key)
        .filter(|e| e.fingerprint == fingerprint)
        .map(|e| e.skeleton.clone())
}

fn cache_put(key: String, fingerprint: u64, skeleton: BranchHierarchy) {
    if let Ok(mut map) = cache().lock() {
        map.insert(
            key,
            CacheEntry {
                fingerprint,
                skeleton,
            },
        );
    }
}

/// Hash every ref's (short, oid) — locals AND remotes, since a fetch moves
/// remote tips (changing upstream ahead/behind and the boundary) without any
/// local oid moving — plus the trunk identity.
fn fingerprint(branches: &[BranchInfo], trunk: Option<&TrunkInfo>) -> u64 {
    let mut pairs: Vec<(&str, &str)> = branches
        .iter()
        .map(|b| (b.short.as_str(), b.oid.as_str()))
        .collect();
    pairs.sort_unstable();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    pairs.hash(&mut h);
    trunk
        .map(|t| (&t.local_short, &t.boundary_oid))
        .hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tip(name: &str, oid: &str) -> LocalTip {
        LocalTip {
            name: name.to_string(),
            oid: oid.to_string(),
            ..Default::default()
        }
    }

    /// Build a graph from (oid, parents) pairs.
    fn graph(edges: &[(&str, &[&str])]) -> Graph {
        edges
            .iter()
            .map(|(oid, parents)| {
                (
                    oid.to_string(),
                    parents.iter().map(|p| p.to_string()).collect(),
                )
            })
            .collect()
    }

    fn node<'a>(h: &'a BranchHierarchy, name: &str) -> &'a BranchNode {
        h.node(name).unwrap_or_else(|| panic!("node {name}"))
    }

    #[test]
    fn convention_tree_nests_workbranch_and_task() {
        // main(boundary) ← w (wb tip) ← a (task tip)
        let tips = vec![
            tip("main", "m0"),
            tip("emmett/wb-2026-06-10", "w1"),
            tip("feat/x-1", "a1"),
        ];
        let g = graph(&[("w1", &["m0"]), ("a1", &["w1"])]);
        let h = build_skeleton(&tips, Some("main"), &g, false);

        assert_eq!(node(&h, "main").role, BranchRole::Trunk);
        let wb = node(&h, "emmett/wb-2026-06-10");
        assert_eq!(wb.role, BranchRole::Workbranch);
        assert_eq!(wb.parent.as_deref(), Some("main"));
        assert_eq!(wb.ahead_of_parent, 1);
        let task = node(&h, "feat/x-1");
        assert_eq!(task.role, BranchRole::Task);
        assert_eq!(task.parent.as_deref(), Some("emmett/wb-2026-06-10"));
        assert_eq!(task.ahead_of_parent, 1);
        assert_eq!(task.behind_parent, Some(0));
        // Depth-first order: trunk, workbranch, its task.
        let names: Vec<&str> = h.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["main", "emmett/wb-2026-06-10", "feat/x-1"]);
    }

    #[test]
    fn stacked_tasks_nest_under_the_nearest_ancestor() {
        let tips = vec![
            tip("main", "m0"),
            tip("emmett/wb-2026-06-10", "w1"),
            tip("feat/parent-1", "a1"),
            tip("feat/stacked-2", "b1"),
        ];
        let g = graph(&[("w1", &["m0"]), ("a1", &["w1"]), ("b1", &["a1"])]);
        let h = build_skeleton(&tips, Some("main"), &g, false);
        assert_eq!(
            node(&h, "feat/stacked-2").parent.as_deref(),
            Some("feat/parent-1"),
            "stacked branch must pick the nearest ancestor, not the workbranch"
        );
    }

    #[test]
    fn fresh_task_at_workbranch_tip_parents_by_convention() {
        let tips = vec![
            tip("main", "m0"),
            tip("emmett/wb-2026-06-10", "w1"),
            tip("feat/fresh-3", "w1"), // same oid as the workbranch
        ];
        let g = graph(&[("w1", &["m0"])]);
        let h = build_skeleton(&tips, Some("main"), &g, false);
        let fresh = node(&h, "feat/fresh-3");
        assert_eq!(fresh.parent.as_deref(), Some("emmett/wb-2026-06-10"));
        assert_eq!(fresh.ahead_of_parent, 0);
        assert!(!fresh.merged_into_parent, "a fresh cut is not 'merged'");
    }

    #[test]
    fn equal_oid_twins_without_convention_fall_to_trunk() {
        let tips = vec![tip("main", "m0"), tip("alpha", "x1"), tip("beta", "x1")];
        let g = graph(&[("x1", &["m0"])]);
        let h = build_skeleton(&tips, Some("main"), &g, false);
        assert_eq!(node(&h, "alpha").parent.as_deref(), Some("main"));
        assert_eq!(node(&h, "beta").parent.as_deref(), Some("main"));
    }

    #[test]
    fn unrebased_task_still_parents_its_advanced_workbranch() {
        // Task forked at w1; the workbranch advanced to w2 (task not rebased):
        // no strict ancestor, but shared off-trunk history + wb naming.
        let tips = vec![
            tip("main", "m0"),
            tip("emmett/wb-2026-06-10", "w2"),
            tip("feat/stale-4", "a1"),
        ];
        let g = graph(&[("w1", &["m0"]), ("w2", &["w1"]), ("a1", &["w1"])]);
        let h = build_skeleton(&tips, Some("main"), &g, false);
        let task = node(&h, "feat/stale-4");
        assert_eq!(task.parent.as_deref(), Some("emmett/wb-2026-06-10"));
        assert_eq!(task.ahead_of_parent, 1);
        assert_eq!(task.behind_parent, Some(1), "needs-rebase signal");
    }

    #[test]
    fn hotfix_off_main_is_standalone() {
        let tips = vec![tip("main", "m0"), tip("hotfix/crash", "h1")];
        let g = graph(&[("h1", &["m0"])]);
        let h = build_skeleton(&tips, Some("main"), &g, false);
        let hf = node(&h, "hotfix/crash");
        assert_eq!(hf.role, BranchRole::Standalone);
        assert_eq!(hf.parent.as_deref(), Some("main"));
        assert_eq!(hf.ahead_of_parent, 1);
    }

    #[test]
    fn merged_workbranch_is_contained_in_trunk() {
        // The workbranch tip is reachable from the boundary → empty reach →
        // ahead 0 → merged.
        let tips = vec![tip("main", "m0"), tip("emmett/wb-2026-06-09", "old")];
        let g = Graph::new(); // everything reachable from boundary
        let h = build_skeleton(&tips, Some("main"), &g, false);
        let wb = node(&h, "emmett/wb-2026-06-09");
        assert_eq!(wb.ahead_of_parent, 0);
        assert!(wb.merged_into_parent);
    }

    #[test]
    fn no_trunk_yields_flat_standalones() {
        let tips = vec![tip("devel", "d1"), tip("feature", "f1")];
        let h = build_skeleton(&tips, None, &Graph::new(), false);
        assert_eq!(h.trunk, None);
        assert!(h.nodes.iter().all(|n| n.parent.is_none()));
        assert!(
            h.nodes
                .iter()
                .all(|n| n.role == BranchRole::Standalone || n.role == BranchRole::Workbranch)
        );
        assert_eq!(h.nodes.len(), 2);
    }

    #[test]
    fn is_active_keeps_drained_workbranch_with_live_task() {
        // The workbranch is fully merged (its tip sits in trunk history, so it
        // is absent from the off-trunk graph). A fresh task cut at the same
        // tip still has unpushed work (ahead of its upstream).
        let mut task = tip("feat/live-5", "w0");
        task.ahead = Some(1);
        let tips = vec![tip("main", "m0"), tip("emmett/wb-2026-06-10", "w0"), task];
        let h = build_skeleton(&tips, Some("main"), &Graph::new(), false);

        let task = node(&h, "feat/live-5");
        assert_eq!(
            task.parent.as_deref(),
            Some("emmett/wb-2026-06-10"),
            "equal-oid tie resolves to the wb-named parent"
        );
        let wb = node(&h, "emmett/wb-2026-06-10");
        assert_eq!(wb.ahead_of_parent, 0, "workbranch is drained");
        assert!(
            h.is_active(wb),
            "drained workbranch stays visible while its task is active"
        );
        // Without the live task it would be inactive.
        let alone = build_skeleton(
            &[tip("main", "m0"), tip("emmett/wb-2026-06-10", "w0")],
            Some("main"),
            &Graph::new(),
            false,
        );
        let wb_alone = node(&alone, "emmett/wb-2026-06-10");
        assert!(!alone.is_active(wb_alone));
    }

    #[test]
    fn merging_a_task_into_its_workbranch_does_not_cycle() {
        // The team's own flow: wb merges task via a merge commit. The task's
        // tip becomes a strict ancestor of the wb — the wb must still parent
        // to trunk (never to its own task), and the task stays under the wb.
        //   main(boundary) ← w1(wb base) ← t1(task tip); wb tip = merge w2(w1, t1)
        let tips = vec![
            tip("main", "m0"),
            tip("emmett/wb-2026-06-10", "w2"),
            tip("feat/x-1", "t1"),
        ];
        let g = graph(&[("w1", &["m0"]), ("t1", &["w1"]), ("w2", &["w1", "t1"])]);
        let h = build_skeleton(&tips, Some("main"), &g, false);

        let wb = node(&h, "emmett/wb-2026-06-10");
        assert_eq!(
            wb.parent.as_deref(),
            Some("main"),
            "wb must not be parented by its merged task"
        );
        let task = node(&h, "feat/x-1");
        assert_eq!(task.parent.as_deref(), Some("emmett/wb-2026-06-10"));
        assert!(task.merged_into_parent);
        // Both rows must be reachable in depth-first order (no silent drop).
        let names: Vec<&str> = h.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["main", "emmett/wb-2026-06-10", "feat/x-1"]);
    }

    #[test]
    fn break_cycles_reattaches_a_forced_cycle_to_trunk() {
        // Backstop check: hand-build a cycle and confirm it is cut at the
        // wb-named member and everything stays visible.
        let tips = vec![
            tip("main", "m0"),
            tip("emmett/wb-1", "w1"),
            tip("feat/a", "a1"),
        ];
        let g = graph(&[("w1", &["m0"]), ("a1", &["w1"])]);
        let mut h = build_skeleton(&tips, Some("main"), &g, false);
        // Force the cycle the selection rules refuse to produce.
        for n in &mut h.nodes {
            if n.name == "emmett/wb-1" {
                n.parent = Some("feat/a".to_string());
            }
        }
        let reaches: HashMap<&str, HashSet<&str>> = [
            ("main", reach("m0", &g)),
            ("emmett/wb-1", reach("w1", &g)),
            ("feat/a", reach("a1", &g)),
        ]
        .into_iter()
        .collect();
        let mut nodes = std::mem::take(&mut h.nodes);
        break_cycles(&mut nodes, Some("main"), &reaches);
        let wb = nodes.iter().find(|n| n.name == "emmett/wb-1").unwrap();
        assert_eq!(wb.parent.as_deref(), Some("main"), "cycle cut at the wb");
    }

    #[test]
    fn wb_named_matches_last_segment() {
        assert!(wb_named("emmett/wb-2026-06-10"));
        assert!(wb_named("wb-solo"));
        assert!(!wb_named("feat/wb"));
        assert!(!wb_named("feat/csv-export-142"));
        assert!(!wb_named("main"));
    }
}
