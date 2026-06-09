//! State + reducer for the machine-wide home view.
//!
//! Mirrors the per-repo `AppState` shape at a coarser grain: the snapshot is a
//! `HomeSnapshot` (one captured repo per card), selection moves over repo
//! cards, and `Enter` drills into the selected repo's full per-repo view. Live
//! flashes are keyed by worktree path (globally unique across repos), reusing
//! the same `change_kind` heuristic as the per-repo reducer.

use std::collections::{HashMap, HashSet};

use crossterm::event::{KeyCode, KeyEvent};

use super::snapshot::{HomeSnapshot, repo_key};
use crate::app::state::change_kind;
use crate::app::{LiveStatus, TransitionKind, Transitions};
use crate::git::{RepoSnapshot, WorktreeRecord};

/// What a key press means in the home view. A small local enum — the per-repo
/// `Action` set (overlays, fetch, remove, …) doesn't apply here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HomeAction {
    None,
    Quit,
    ToggleHelp,
    MoveUp,
    MoveDown,
    Refresh,
    DrillIn,
}

/// The home-view state. `snapshot` is the captured truth; selection and flashes
/// are derived/transient.
#[derive(Debug)]
pub struct HomeState {
    pub should_quit: bool,
    pub snapshot: HomeSnapshot,
    /// Selected repo-card index.
    pub selected: usize,
    /// Transient per-worktree highlights (keyed by path).
    pub transitions: Transitions,
    pub live: LiveStatus,
    pub show_help: bool,
    pending_refresh: bool,
    /// Set by `Enter`; consumed by the event loop to drill into the selection.
    drill_in: bool,
}

impl HomeState {
    pub fn new(snapshot: HomeSnapshot) -> Self {
        Self {
            should_quit: false,
            snapshot,
            selected: 0,
            transitions: Transitions::default(),
            live: LiveStatus::Static,
            show_help: false,
            pending_refresh: false,
            drill_in: false,
        }
    }

    /// Number of repo cards.
    pub fn repo_count(&self) -> usize {
        self.snapshot.repos.len()
    }

    /// The selected repo snapshot, if any.
    pub fn selected_repo(&self) -> Option<&RepoSnapshot> {
        self.snapshot.repos.get(self.selected)
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
            KeyCode::Char('j') | KeyCode::Down => HomeAction::MoveDown,
            KeyCode::Char('k') | KeyCode::Up => HomeAction::MoveUp,
            _ => HomeAction::None,
        }
    }

    /// Apply a key press (resolve + reduce). The single place home state changes.
    pub fn handle_key(&mut self, key: KeyEvent) {
        match self.resolve_key(key) {
            HomeAction::None => {}
            HomeAction::Quit => self.should_quit = true,
            HomeAction::ToggleHelp => self.show_help = !self.show_help,
            HomeAction::MoveUp => self.move_selection(-1),
            HomeAction::MoveDown => self.move_selection(1),
            HomeAction::Refresh => self.pending_refresh = true,
            HomeAction::DrillIn => {
                if self.selected_repo().is_some() {
                    self.drill_in = true;
                }
            }
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let len = self.repo_count();
        if len > 0 {
            self.selected = (self.selected as i32 + delta).clamp(0, len as i32 - 1) as usize;
        }
    }

    /// Consume the pending-refresh flag (set by `r`).
    pub fn take_pending_refresh(&mut self) -> bool {
        std::mem::take(&mut self.pending_refresh)
    }

    /// Consume a pending drill-in, returning a clone of the selected repo
    /// snapshot for the per-repo view to take over.
    pub fn take_drill_in(&mut self) -> Option<RepoSnapshot> {
        if std::mem::take(&mut self.drill_in) {
            self.selected_repo().cloned()
        } else {
            None
        }
    }

    pub fn set_live_status(&mut self, status: LiveStatus) {
        self.live = status;
    }

    pub fn expire_transitions(&mut self) {
        self.transitions.expire();
    }

    /// The active transient highlight for a worktree path, if any.
    pub fn transition_for(&self, path: &str) -> Option<TransitionKind> {
        self.transitions.get(path)
    }

    /// Flash live save activity for a batch of changed filesystem paths, mapping
    /// each across every repo's worktrees (a path belongs to exactly one).
    pub fn note_activity(&mut self, paths: &[std::path::PathBuf]) {
        let Self {
            snapshot,
            transitions,
            ..
        } = self;
        for repo in &snapshot.repos {
            crate::app::state::note_activity_for(transitions, &repo.worktrees, paths);
        }
    }

    /// Swap in a freshly captured machine-wide snapshot, raising per-worktree
    /// flashes for repos seen in the previous scan (created / modified / pushed
    /// / deleted). New repos don't flash every worktree on first sight.
    pub fn ingest_snapshot(&mut self, new: HomeSnapshot) {
        let old_by_key: HashMap<String, &RepoSnapshot> = self
            .snapshot
            .repos
            .iter()
            .map(|r| (repo_key(r), r))
            .collect();

        let mut notes: Vec<(String, Vec<TransitionKind>)> = Vec::new();
        for new_repo in &new.repos {
            if let Some(old_repo) = old_by_key.get(&repo_key(new_repo)) {
                collect_repo_transitions(old_repo, new_repo, &mut notes);
            }
        }
        for (path, seq) in notes {
            self.transitions.note_seq(path, seq);
        }

        self.snapshot = new;
        self.clamp_selection();
    }

    fn clamp_selection(&mut self) {
        let len = self.repo_count();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }
}

/// Diff one repo's worktrees between scans, pushing flash notes (created /
/// modified / pushed / deleted) keyed by worktree path.
fn collect_repo_transitions(
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
            None => notes.push((wt.path.clone(), vec![TransitionKind::Created])),
            Some(old_wt) => {
                let kinds = change_kind(old_wt, wt);
                if !kinds.is_empty() {
                    notes.push((wt.path.clone(), kinds));
                }
            }
        }
    }

    let incoming: HashSet<&str> = new.worktrees.iter().map(|wt| wt.path.as_str()).collect();
    for old_wt in &old.worktrees {
        if !old_wt.bare && !incoming.contains(old_wt.path.as_str()) {
            notes.push((old_wt.path.clone(), vec![TransitionKind::Deleted]));
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
            collisions: Vec::new(),
            processes: ProcessSnapshot::default(),
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

    #[test]
    fn selection_clamps_to_repo_count() {
        let mut s = HomeState::new(home(vec![
            repo("/a", vec![wt("/a/root")]),
            repo("/b", vec![wt("/b/root")]),
        ]));
        assert_eq!(s.selected, 0);
        s.handle_key(key(KeyCode::Char('k'))); // already at top
        assert_eq!(s.selected, 0);
        s.handle_key(key(KeyCode::Char('j')));
        assert_eq!(s.selected, 1);
        s.handle_key(key(KeyCode::Char('j'))); // clamp at bottom
        assert_eq!(s.selected, 1);
    }

    #[test]
    fn enter_requests_drill_in_only_when_a_repo_is_selected() {
        let mut empty = HomeState::new(home(vec![]));
        empty.handle_key(key(KeyCode::Enter));
        assert!(empty.take_drill_in().is_none());

        let mut s = HomeState::new(home(vec![repo("/a", vec![wt("/a/root")])]));
        s.handle_key(key(KeyCode::Enter));
        assert!(s.take_drill_in().is_some());
        // Consumed — a second take yields nothing.
        assert!(s.take_drill_in().is_none());
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
        s.ingest_snapshot(home(vec![repo("/a", vec![wt("/a/root"), wt("/a/feat-y")])]));
        assert_eq!(s.transition_for("/a/feat-y"), Some(TransitionKind::Created));
        assert_eq!(s.transition_for("/a/feat-x"), Some(TransitionKind::Deleted));
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
        s.ingest_snapshot(home(vec![repo("/a", vec![pushed])]));
        assert_eq!(s.transition_for("/a/feat"), Some(TransitionKind::Pushed));
    }

    #[test]
    fn note_activity_flashes_the_owning_worktree_across_repos() {
        let mut s = HomeState::new(home(vec![
            repo("/a", vec![wt("/a/root"), wt("/a/feat")]),
            repo("/b", vec![wt("/b/root")]),
        ]));
        s.note_activity(&[std::path::PathBuf::from("/a/feat/src/x.rs")]);
        assert_eq!(s.transition_for("/a/feat"), Some(TransitionKind::Activity));
        assert_eq!(s.transition_for("/a/root"), None);
        assert_eq!(s.transition_for("/b/root"), None);
    }
}
