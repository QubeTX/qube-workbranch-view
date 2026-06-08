//! The application state and its reducer.

use std::collections::{HashMap, HashSet, VecDeque};

use crossterm::event::{KeyCode, KeyEvent};

use super::action::Action;
use super::transitions::{TransitionKind, Transitions};
use crate::git::{RepoSnapshot, WorktreeRecord};
use crate::storage::{ArchivedEvent, DirtySummary, EventKind};

/// In-memory cap on archived events (matches the on-disk prune cap).
const MAX_ARCHIVE: usize = 2000;

/// Top-level views. Mirrors the design's tab set; Timeline arrives with the
/// event archive in a later phase (handoff §14.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Worktrees,
    Processes,
    Collisions,
    Cleanup,
    Timeline,
    Help,
}

impl Tab {
    /// All tabs, in display order.
    pub const ALL: [Tab; 7] = [
        Tab::Overview,
        Tab::Worktrees,
        Tab::Processes,
        Tab::Collisions,
        Tab::Cleanup,
        Tab::Timeline,
        Tab::Help,
    ];

    /// The short title shown in the tab bar.
    pub fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Worktrees => "Worktrees",
            Tab::Processes => "Processes",
            Tab::Collisions => "Collisions",
            Tab::Cleanup => "Cleanup",
            Tab::Timeline => "Timeline",
            Tab::Help => "Help",
        }
    }

    /// Position in [`Tab::ALL`].
    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }

    fn next(self) -> Tab {
        let i = self.index();
        Tab::ALL[(i + 1) % Tab::ALL.len()]
    }

    fn prev(self) -> Tab {
        let i = self.index();
        Tab::ALL[(i + Tab::ALL.len() - 1) % Tab::ALL.len()]
    }
}

/// Whether live updating is active (drives the header indicator).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveStatus {
    /// Filesystem watcher active, plus the poll backstop.
    Live,
    /// Watcher unavailable; periodic poll only.
    PollOnly,
    /// `--no-live`: no watcher and no poll (manual refresh only).
    Static,
}

/// The whole UI state. `snapshot` is the Git source of truth; everything the UI
/// shows is derived from it.
#[derive(Debug)]
pub struct AppState {
    /// Set once the user has asked to quit; the event loop checks it each turn.
    pub should_quit: bool,
    /// The currently focused tab.
    pub active_tab: Tab,
    /// Whether the help overlay is shown.
    pub show_help: bool,
    /// The captured Git state.
    pub snapshot: RepoSnapshot,
    /// Selected index within the worktree list.
    pub selected: usize,
    /// Transient highlights for live changes.
    pub transitions: Transitions,
    /// Set by `apply(Refresh)`; consumed by the event loop to trigger a capture.
    pending_refresh: bool,
    /// Whether live updating is active (for the header indicator).
    pub live: LiveStatus,
    /// Archived structural events (newest first, capped) — the Timeline.
    pub archive: VecDeque<ArchivedEvent>,
}

impl AppState {
    /// Create the initial state from a captured snapshot.
    pub fn new(snapshot: RepoSnapshot) -> Self {
        let selected = snapshot.current_worktree_index().unwrap_or(0);
        Self {
            should_quit: false,
            active_tab: Tab::Overview,
            show_help: false,
            snapshot,
            selected,
            transitions: Transitions::default(),
            pending_refresh: false,
            live: LiveStatus::Static,
            archive: VecDeque::new(),
        }
    }

    /// Replace the snapshot (after a manual/live refresh), clamping selection.
    pub fn set_snapshot(&mut self, snapshot: RepoSnapshot) {
        self.snapshot = snapshot;
        let len = self.snapshot.worktrees.len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    /// Replace the archive (loaded at startup). On disk events are oldest-first;
    /// we store them newest-first for display, capped to [`MAX_ARCHIVE`].
    pub fn set_events(&mut self, events: Vec<ArchivedEvent>) {
        self.archive = events.into_iter().rev().collect();
        self.truncate_archive();
    }

    /// Cap the in-memory archive to the most recent [`MAX_ARCHIVE`] events.
    fn truncate_archive(&mut self) {
        while self.archive.len() > MAX_ARCHIVE {
            self.archive.pop_back();
        }
    }

    /// Diff the new snapshot against the current one: raise transient highlights,
    /// record created/removed worktrees in the archive, swap the snapshot in, and
    /// return the newly-detected events for the caller to persist.
    pub fn ingest_snapshot(&mut self, new: RepoSnapshot) -> Vec<ArchivedEvent> {
        let mut new_events: Vec<ArchivedEvent> = Vec::new();
        let mut notes: Vec<(String, TransitionKind)> = Vec::new();
        {
            let previous: HashMap<&str, &WorktreeRecord> = self
                .snapshot
                .worktrees
                .iter()
                .map(|wt| (wt.path.as_str(), wt))
                .collect();
            for wt in &new.worktrees {
                match previous.get(wt.path.as_str()) {
                    None => {
                        notes.push((wt.path.clone(), TransitionKind::Created));
                        new_events.push(event_from(EventKind::WorktreeCreated, wt));
                    }
                    Some(old) if dirty_summary(old) != dirty_summary(wt) => {
                        notes.push((wt.path.clone(), TransitionKind::Modified));
                    }
                    Some(_) => {}
                }
            }
            let incoming: HashSet<&str> = new.worktrees.iter().map(|wt| wt.path.as_str()).collect();
            for old in &self.snapshot.worktrees {
                if !old.bare && !incoming.contains(old.path.as_str()) {
                    notes.push((old.path.clone(), TransitionKind::Deleted));
                    new_events.push(event_from(EventKind::WorktreeRemoved, old));
                }
            }
        }

        for (path, kind) in notes {
            self.transitions.note(path, kind);
        }
        // Prepend so the archive stays newest-first; reverse keeps in-batch order.
        for event in new_events.iter().rev() {
            self.archive.push_front(event.clone());
        }
        self.truncate_archive();
        self.set_snapshot(new);
        new_events
    }

    /// The active transient highlight for a worktree path, if any.
    pub fn transition_for(&self, path: &str) -> Option<TransitionKind> {
        self.transitions.get(path)
    }

    /// Drop expired transient highlights (driven by the animation tick).
    pub fn expire_transitions(&mut self) {
        self.transitions.expire();
    }

    /// Consume the pending-refresh flag set by `apply(Action::Refresh)`.
    pub fn take_pending_refresh(&mut self) -> bool {
        std::mem::take(&mut self.pending_refresh)
    }

    /// Record whether live updating is active (set once by the event loop).
    pub fn set_live_status(&mut self, status: LiveStatus) {
        self.live = status;
    }

    /// The worktrees from the current snapshot.
    pub fn worktrees(&self) -> &[WorktreeRecord] {
        &self.snapshot.worktrees
    }

    /// The currently selected worktree, if any.
    pub fn selected_worktree(&self) -> Option<&WorktreeRecord> {
        self.worktrees().get(self.selected)
    }

    /// Human-readable label for the repository under inspection.
    pub fn repo_label(&self) -> String {
        self.snapshot.repo.root.display().to_string()
    }

    /// Resolve a key press to an [`Action`] given the current context.
    ///
    /// Bindings match the design defaults (handoff §16); the full configurable
    /// resolver arrives with the config subsystem.
    pub fn resolve_key(&self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Esc if self.show_help => Action::ToggleHelp,
            KeyCode::Esc => Action::Quit,
            KeyCode::Char('?') => Action::ToggleHelp,
            KeyCode::Tab => Action::NextTab,
            KeyCode::BackTab => Action::PrevTab,
            KeyCode::Char('r') => Action::Refresh,
            KeyCode::Char('j') | KeyCode::Down => Action::MoveDown,
            KeyCode::Char('k') | KeyCode::Up => Action::MoveUp,
            KeyCode::Char(c @ '1'..='7') => {
                let idx = (c as u8 - b'1') as usize;
                Action::SelectTab(Tab::ALL[idx])
            }
            _ => Action::None,
        }
    }

    /// Apply an [`Action`], mutating state. The single place state changes.
    pub fn apply(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::Quit => self.should_quit = true,
            Action::NextTab => self.active_tab = self.active_tab.next(),
            Action::PrevTab => self.active_tab = self.active_tab.prev(),
            Action::SelectTab(tab) => self.active_tab = tab,
            Action::MoveDown => {
                let len = self.worktrees().len();
                if len > 0 {
                    self.selected = (self.selected + 1).min(len - 1);
                }
            }
            Action::MoveUp => self.selected = self.selected.saturating_sub(1),
            // The async re-capture runs in the event loop; the reducer just
            // records the intent (keeping mutation in one place).
            Action::Refresh => self.pending_refresh = true,
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
                if self.show_help {
                    self.active_tab = Tab::Help;
                }
            }
        }
    }
}

/// Build an [`ArchivedEvent`] from a worktree's last-known state.
fn event_from(kind: EventKind, wt: &WorktreeRecord) -> ArchivedEvent {
    let dirty = wt.status.as_ref().map(|s| DirtySummary {
        staged: s.staged,
        unstaged: s.unstaged,
        untracked: s.untracked,
        conflicted: s.conflicted,
    });
    ArchivedEvent::new(
        kind,
        wt.path.clone(),
        wt.branch_short().map(str::to_string),
        wt.short_head().map(str::to_string),
        dirty,
    )
}

/// The persistent dirty/divergence fingerprint used to detect changes between
/// snapshots. A `None` status compares as all-zero.
fn dirty_summary(wt: &WorktreeRecord) -> (usize, usize, usize, usize, u32, u32) {
    match &wt.status {
        Some(s) => (
            s.staged,
            s.unstaged,
            s.untracked,
            s.conflicted,
            s.ahead.unwrap_or(0),
            s.behind.unwrap_or(0),
        ),
        None => (0, 0, 0, 0, 0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{RepoIdentity, WorktreeRecord};
    use crate::process::ProcessSnapshot;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn snapshot_with(n: usize) -> RepoSnapshot {
        let worktrees = (0..n)
            .map(|i| WorktreeRecord {
                path: format!("/repo-{i}"),
                ..Default::default()
            })
            .collect();
        RepoSnapshot {
            repo: RepoIdentity {
                start_dir: "/repo".into(),
                root: "/repo".into(),
                git_dir: "/repo/.git".into(),
                common_git_dir: "/repo/.git".into(),
                is_worktree: false,
            },
            worktrees,
            branches: Vec::new(),
            base: None,
            collisions: Vec::new(),
            processes: ProcessSnapshot::default(),
        }
    }

    fn app(n: usize) -> AppState {
        AppState::new(snapshot_with(n))
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn q_quits() {
        let mut a = app(0);
        let action = a.resolve_key(key(KeyCode::Char('q')));
        assert_eq!(action, Action::Quit);
        a.apply(action);
        assert!(a.should_quit);
    }

    #[test]
    fn tab_cycles_forward_and_wraps() {
        let mut a = app(0);
        assert_eq!(a.active_tab, Tab::Overview);
        for _ in 0..Tab::ALL.len() {
            let action = a.resolve_key(key(KeyCode::Tab));
            a.apply(action);
        }
        assert_eq!(a.active_tab, Tab::Overview); // wrapped fully around
    }

    #[test]
    fn back_tab_goes_to_last() {
        let mut a = app(0);
        let action = a.resolve_key(key(KeyCode::BackTab));
        a.apply(action);
        assert_eq!(a.active_tab, Tab::Help);
    }

    #[test]
    fn number_selects_tab() {
        let mut a = app(0);
        let action = a.resolve_key(key(KeyCode::Char('3')));
        a.apply(action);
        assert_eq!(a.active_tab, Tab::Processes);
    }

    #[test]
    fn help_toggles() {
        let mut a = app(0);
        a.apply(a.resolve_key(key(KeyCode::Char('?'))));
        assert!(a.show_help);
        a.apply(a.resolve_key(key(KeyCode::Esc)));
        assert!(!a.show_help);
        assert!(!a.should_quit);
    }

    #[test]
    fn worktree_navigation_clamps() {
        let mut a = app(3);
        assert_eq!(a.selected, 0);
        a.apply(Action::MoveUp); // already at top
        assert_eq!(a.selected, 0);
        a.apply(Action::MoveDown);
        a.apply(Action::MoveDown);
        assert_eq!(a.selected, 2);
        a.apply(Action::MoveDown); // clamp at bottom
        assert_eq!(a.selected, 2);
    }

    #[test]
    fn navigation_noop_when_empty() {
        let mut a = app(0);
        a.apply(Action::MoveDown);
        assert_eq!(a.selected, 0);
    }

    fn snapshot_paths(paths: &[&str]) -> RepoSnapshot {
        let worktrees = paths
            .iter()
            .map(|p| WorktreeRecord {
                path: (*p).to_string(),
                ..Default::default()
            })
            .collect();
        RepoSnapshot {
            repo: RepoIdentity {
                start_dir: "/repo".into(),
                root: "/repo".into(),
                git_dir: "/repo/.git".into(),
                common_git_dir: "/repo/.git".into(),
                is_worktree: false,
            },
            worktrees,
            branches: Vec::new(),
            base: None,
            collisions: Vec::new(),
            processes: ProcessSnapshot::default(),
        }
    }

    #[test]
    fn ingest_detects_created_and_removed() {
        let mut app = AppState::new(snapshot_paths(&["/repo-a", "/repo-b"]));
        let events = app.ingest_snapshot(snapshot_paths(&["/repo-a", "/repo-c"]));
        assert_eq!(events.len(), 2);
        let kinds: Vec<(EventKind, &str)> =
            events.iter().map(|e| (e.kind, e.path.as_str())).collect();
        assert!(kinds.contains(&(EventKind::WorktreeCreated, "/repo-c")));
        assert!(kinds.contains(&(EventKind::WorktreeRemoved, "/repo-b")));
        assert_eq!(app.archive.len(), 2);
    }
}
