//! State + reducer for the machine-wide home view.
//!
//! The home view is the SAME branch tree as the per-repo view, with one repo
//! node at the root per active repository — one window, every repo, every
//! branch, every agent. Navigation and expansion reuse `app::tree`; `Enter`
//! drills into the selected repo's full per-repo view (where mutations live).

use std::collections::{HashMap, HashSet};

use crossterm::event::{KeyCode, KeyEvent};

use super::snapshot::{HomeSnapshot, repo_key};
use crate::app::state::{
    branch_events, change_kind, conflict_events, note_activity_tree, wt_flash_key,
};
use crate::app::tree::{NodeId, TreeRow, TreeState, flatten};
use crate::app::{LiveStatus, TransitionKind, Transitions};
use crate::git::{RepoSnapshot, WorktreeRecord};
use crate::storage::ArchivedEvent;

/// What a key press means in the home view. A small local enum — the per-repo
/// mutation actions (remove / kill / fetch) live in the drilled-in view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HomeAction {
    None,
    Quit,
    ToggleHelp,
    MoveUp,
    MoveDown,
    Expand,
    Collapse,
    ToggleExpand,
    ToggleShowAll,
    Refresh,
    DrillIn,
}

/// The home-view state. `snapshot` is the captured truth; tree state and
/// flashes are derived/transient.
#[derive(Debug)]
pub struct HomeState {
    pub should_quit: bool,
    pub snapshot: HomeSnapshot,
    /// Expansion + selection over the machine-wide tree.
    pub tree: TreeState,
    /// Transient highlights, keyed by `NodeId::flash_key` (globally unique
    /// across repos via the repo key).
    pub transitions: Transitions,
    pub live: LiveStatus,
    pub show_help: bool,
    pending_refresh: bool,
    /// Set by `Enter`: the repo key to drill into; consumed by the event loop.
    drill_in: Option<String>,
}

impl HomeState {
    pub fn new(snapshot: HomeSnapshot) -> Self {
        Self {
            should_quit: false,
            snapshot,
            tree: TreeState::default(),
            transitions: Transitions::default(),
            live: LiveStatus::Static,
            show_help: false,
            pending_refresh: false,
            drill_in: None,
        }
    }

    /// Number of active repos.
    pub fn repo_count(&self) -> usize {
        self.snapshot.repos.len()
    }

    /// The flattened machine-wide tree rows.
    pub fn tree_rows(&self) -> Vec<TreeRow<'_>> {
        flatten(&self.snapshot.repos, &self.tree, None)
    }

    /// Resolve a key press to a [`HomeAction`].
    fn resolve_key(&self, key: KeyEvent) -> HomeAction {
        match key.code {
            KeyCode::Char('q') => HomeAction::Quit,
            KeyCode::Esc if self.show_help => HomeAction::ToggleHelp,
            KeyCode::Esc => HomeAction::Quit,
            KeyCode::Char('?') => HomeAction::ToggleHelp,
            KeyCode::Char('r') => HomeAction::Refresh,
            KeyCode::Enter => HomeAction::DrillIn,
            KeyCode::Char(' ') => HomeAction::ToggleExpand,
            KeyCode::Char('a') => HomeAction::ToggleShowAll,
            KeyCode::Char('j') | KeyCode::Down => HomeAction::MoveDown,
            KeyCode::Char('k') | KeyCode::Up => HomeAction::MoveUp,
            KeyCode::Char('l') | KeyCode::Right => HomeAction::Expand,
            KeyCode::Char('h') | KeyCode::Left => HomeAction::Collapse,
            _ => HomeAction::None,
        }
    }

    /// Apply a key press (resolve + reduce). The single place home state changes.
    pub fn handle_key(&mut self, key: KeyEvent) {
        match self.resolve_key(key) {
            HomeAction::None => {}
            HomeAction::Quit => self.should_quit = true,
            HomeAction::ToggleHelp => self.show_help = !self.show_help,
            HomeAction::MoveUp => self.with_rows(|tree, rows| tree.move_selection(rows, -1)),
            HomeAction::MoveDown => self.with_rows(|tree, rows| tree.move_selection(rows, 1)),
            HomeAction::Expand => self.with_rows(|tree, rows| tree.expand_selected(rows)),
            HomeAction::Collapse => self.with_rows(|tree, rows| tree.collapse_selected(rows)),
            HomeAction::ToggleExpand => self.with_rows(|tree, rows| tree.toggle_selected(rows)),
            HomeAction::ToggleShowAll => {
                self.tree.show_all = !self.tree.show_all;
                self.refresh_tree_selection();
            }
            HomeAction::Refresh => self.pending_refresh = true,
            HomeAction::DrillIn => {
                let rows = flatten(&self.snapshot.repos, &self.tree, None);
                if let Some(i) = self.tree.selected_index(&rows) {
                    let key = match &rows[i].id {
                        NodeId::Repo { repo }
                        | NodeId::Branch { repo, .. }
                        | NodeId::Detached { repo, .. }
                        | NodeId::File { repo, .. } => repo.clone(),
                    };
                    self.drill_in = Some(key);
                }
            }
        }
    }

    /// Run a tree-state mutation with rows computed from disjoint field
    /// borrows (`snapshot` immutably, `tree` mutably).
    fn with_rows(&mut self, f: impl FnOnce(&mut TreeState, &[TreeRow])) {
        let rows = flatten(&self.snapshot.repos, &self.tree, None);
        f(&mut self.tree, &rows);
    }

    /// Consume the pending-refresh flag (set by `r`).
    pub fn take_pending_refresh(&mut self) -> bool {
        std::mem::take(&mut self.pending_refresh)
    }

    /// Consume a pending drill-in, returning a clone of the selected repo
    /// snapshot for the per-repo view to take over.
    pub fn take_drill_in(&mut self) -> Option<RepoSnapshot> {
        let key = self.drill_in.take()?;
        self.snapshot
            .repos
            .iter()
            .find(|r| repo_key(r) == key)
            .cloned()
    }

    pub fn set_live_status(&mut self, status: LiveStatus) {
        self.live = status;
    }

    pub fn expire_transitions(&mut self) {
        self.transitions.expire();
    }

    /// The active transient highlight for a flash key, if any.
    pub fn transition_for(&self, key: &str) -> Option<TransitionKind> {
        self.transitions.get(key)
    }

    /// Flash live save activity for a batch of changed filesystem paths,
    /// mapping each across every repo's worktrees (a path belongs to one).
    pub fn note_activity(&mut self, paths: &[std::path::PathBuf]) {
        let Self {
            snapshot,
            transitions,
            ..
        } = self;
        for repo in &snapshot.repos {
            let rkey = repo_key(repo);
            note_activity_tree(transitions, &rkey, &repo.worktrees, paths);
        }
    }

    /// Swap in a freshly captured machine-wide snapshot, raising branch-keyed
    /// flashes for repos seen in the previous scan. New repos don't flash
    /// every row on first sight. Returns branch milestone events across all
    /// matched repos, each stamped with its repo name — the machine-wide feed
    /// for notifications.
    pub fn ingest_snapshot(&mut self, new: HomeSnapshot) -> Vec<ArchivedEvent> {
        let old_by_key: HashMap<String, &RepoSnapshot> = self
            .snapshot
            .repos
            .iter()
            .map(|r| (repo_key(r), r))
            .collect();

        let mut notes: Vec<(String, Vec<TransitionKind>)> = Vec::new();
        let mut events: Vec<ArchivedEvent> = Vec::new();
        for new_repo in &new.repos {
            let rkey = repo_key(new_repo);
            if let Some(old_repo) = old_by_key.get(&rkey) {
                collect_repo_transitions(&rkey, old_repo, new_repo, &mut notes);
                let name = super::snapshot::repo_name(new_repo);
                events.extend(branch_events(old_repo, new_repo, Some(&name)));
                for mut ev in conflict_events(old_repo, new_repo) {
                    ev.repo = Some(name.clone());
                    events.push(ev);
                }
            }
        }
        for (key, seq) in notes {
            self.transitions.note_seq(key, seq);
        }

        self.snapshot = new;
        self.refresh_tree_selection();
        events
    }

    /// Re-resolve the selection and prune expansion state after the snapshot
    /// (or the active-only scope) changed.
    fn refresh_tree_selection(&mut self) {
        let rows = flatten(&self.snapshot.repos, &self.tree, None);
        let live: HashSet<NodeId> = rows.iter().map(|r| r.id.clone()).collect();
        self.tree.retain_ids(&live);
        if let Some(i) = self.tree.selected_index(&rows) {
            self.tree.select_index(&rows, i);
        } else {
            self.tree.selected = None;
        }
    }
}

/// Diff one repo's worktrees between scans, pushing flash notes (created /
/// modified / pushed / deleted) keyed by the worktree's tree-row identity.
fn collect_repo_transitions(
    rkey: &str,
    old: &RepoSnapshot,
    new: &RepoSnapshot,
    notes: &mut Vec<(String, Vec<TransitionKind>)>,
) {
    let previous: HashMap<&str, &WorktreeRecord> = old
        .worktrees
        .iter()
        .map(|wt| (wt.path.as_str(), wt))
        .collect();

    for wt in &new.worktrees {
        match previous.get(wt.path.as_str()) {
            None => notes.push((wt_flash_key(rkey, wt), vec![TransitionKind::Created])),
            Some(old_wt) => {
                let kinds = change_kind(old_wt, wt);
                if !kinds.is_empty() {
                    notes.push((wt_flash_key(rkey, wt), kinds));
                }
            }
        }
    }

    let incoming: HashSet<&str> = new.worktrees.iter().map(|wt| wt.path.as_str()).collect();
    for old_wt in &old.worktrees {
        if !old_wt.bare && !incoming.contains(old_wt.path.as_str()) {
            notes.push((wt_flash_key(rkey, old_wt), vec![TransitionKind::Deleted]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{RepoIdentity, RepoSnapshot, WorktreeRecord, WorktreeStatus};
    use crate::process::ProcessSnapshot;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn repo(common: &str, worktrees: Vec<WorktreeRecord>) -> RepoSnapshot {
        RepoSnapshot {
            repo: RepoIdentity {
                start_dir: common.into(),
                root: format!("{common}/root").into(),
                git_dir: format!("{common}/.git").into(),
                common_git_dir: format!("{common}/.git").into(),
                is_worktree: false,
            },
            base: None,
            worktrees,
            branches: Vec::new(),
            hierarchy: Default::default(),
            collisions: Vec::new(),
            processes: ProcessSnapshot::default(),
            captured_at: 0,
        }
    }

    fn wt(path: &str) -> WorktreeRecord {
        WorktreeRecord {
            path: path.to_string(),
            ..Default::default()
        }
    }

    fn home(repos: Vec<RepoSnapshot>) -> HomeSnapshot {
        HomeSnapshot {
            repos,
            scanned_at: 0,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn selected_row(s: &HomeState) -> Option<usize> {
        let rows = s.tree_rows();
        s.tree.selected_index(&rows)
    }

    #[test]
    fn tree_navigation_clamps_across_repos() {
        // Two repos, each with one branchless worktree → 4 rows total
        // (repo, detached, repo, detached).
        let mut s = HomeState::new(home(vec![
            repo("/a", vec![wt("/a/root")]),
            repo("/b", vec![wt("/b/root")]),
        ]));
        assert_eq!(s.tree_rows().len(), 4);
        s.handle_key(key(KeyCode::Char('k'))); // already at top
        assert_eq!(selected_row(&s), Some(0));
        for _ in 0..5 {
            s.handle_key(key(KeyCode::Char('j')));
        }
        assert_eq!(selected_row(&s), Some(3), "clamped at the last row");
    }

    #[test]
    fn enter_requests_drill_in_only_when_a_repo_is_selected() {
        let mut empty = HomeState::new(home(vec![]));
        empty.handle_key(key(KeyCode::Enter));
        assert!(empty.take_drill_in().is_none());

        let mut s = HomeState::new(home(vec![repo("/a", vec![wt("/a/root")])]));
        s.handle_key(key(KeyCode::Enter));
        let drilled = s.take_drill_in().expect("repo resolved");
        assert_eq!(
            drilled.repo.common_git_dir,
            std::path::PathBuf::from("/a/.git")
        );
        // Consumed — a second take yields nothing.
        assert!(s.take_drill_in().is_none());
    }

    #[test]
    fn enter_on_a_child_row_drills_into_its_repo() {
        let mut s = HomeState::new(home(vec![
            repo("/a", vec![wt("/a/root")]),
            repo("/b", vec![wt("/b/root")]),
        ]));
        // Move onto repo B's detached row (row 3) and drill in.
        for _ in 0..3 {
            s.handle_key(key(KeyCode::Char('j')));
        }
        s.handle_key(key(KeyCode::Enter));
        let drilled = s.take_drill_in().expect("repo resolved from child row");
        assert_eq!(
            drilled.repo.common_git_dir,
            std::path::PathBuf::from("/b/.git")
        );
    }

    #[test]
    fn q_quits_and_question_toggles_help() {
        let mut s = HomeState::new(home(vec![]));
        s.handle_key(key(KeyCode::Char('?')));
        assert!(s.show_help);
        s.handle_key(key(KeyCode::Esc)); // Esc closes help, doesn't quit
        assert!(!s.show_help);
        assert!(!s.should_quit);
        s.handle_key(key(KeyCode::Char('q')));
        assert!(s.should_quit);
    }

    #[test]
    fn ingest_flashes_created_and_deleted_within_a_known_repo() {
        let mut s = HomeState::new(home(vec![repo("/a", vec![wt("/a/root"), wt("/a/feat-x")])]));
        // Same repo (same common dir), feat-x removed and feat-y added.
        let _ = s.ingest_snapshot(home(vec![repo("/a", vec![wt("/a/root"), wt("/a/feat-y")])]));
        let rkey = repo_key(&s.snapshot.repos[0]);
        let created = wt_flash_key(&rkey, &wt("/a/feat-y"));
        let deleted = wt_flash_key(&rkey, &wt("/a/feat-x"));
        assert_eq!(s.transition_for(&created), Some(TransitionKind::Created));
        assert_eq!(s.transition_for(&deleted), Some(TransitionKind::Deleted));
    }

    #[test]
    fn ingest_flashes_pushed_when_ahead_clears() {
        let ahead = WorktreeRecord {
            path: "/a/feat".into(),
            status: Some(WorktreeStatus {
                ahead: Some(2),
                behind: Some(0),
                upstream: Some("origin/main".into()),
                upstream_gone: false,
                ..Default::default()
            }),
            ..Default::default()
        };
        let pushed = WorktreeRecord {
            path: "/a/feat".into(),
            status: Some(WorktreeStatus {
                ahead: Some(0),
                behind: Some(0),
                upstream: Some("origin/main".into()),
                upstream_gone: false,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut s = HomeState::new(home(vec![repo("/a", vec![ahead])]));
        let _ = s.ingest_snapshot(home(vec![repo("/a", vec![pushed.clone()])]));
        let rkey = repo_key(&s.snapshot.repos[0]);
        let pushed_key = wt_flash_key(&rkey, &pushed);
        assert_eq!(s.transition_for(&pushed_key), Some(TransitionKind::Pushed));
    }

    #[test]
    fn note_activity_flashes_the_owning_worktree_across_repos() {
        let mut s = HomeState::new(home(vec![
            repo("/a", vec![wt("/a/root"), wt("/a/feat")]),
            repo("/b", vec![wt("/b/root")]),
        ]));
        s.note_activity(&[std::path::PathBuf::from("/a/feat/src/x.rs")]);
        let rkey_a = repo_key(&s.snapshot.repos[0]);
        let rkey_b = repo_key(&s.snapshot.repos[1]);
        assert_eq!(
            s.transition_for(&wt_flash_key(&rkey_a, &wt("/a/feat"))),
            Some(TransitionKind::Activity)
        );
        assert_eq!(
            s.transition_for(&wt_flash_key(&rkey_a, &wt("/a/root"))),
            None
        );
        assert_eq!(
            s.transition_for(&wt_flash_key(&rkey_b, &wt("/b/root"))),
            None
        );
    }
}
