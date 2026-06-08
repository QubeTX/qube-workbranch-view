//! The application state and its reducer.

use crossterm::event::{KeyCode, KeyEvent};

use super::action::Action;
use crate::git::{RepoSnapshot, WorktreeRecord};

/// Top-level views. Mirrors the design's tab set; Timeline arrives with the
/// event archive in a later phase (handoff §14.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Worktrees,
    Processes,
    Collisions,
    Cleanup,
    Help,
}

impl Tab {
    /// All tabs, in display order.
    pub const ALL: [Tab; 6] = [
        Tab::Overview,
        Tab::Worktrees,
        Tab::Processes,
        Tab::Collisions,
        Tab::Cleanup,
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

/// The whole UI state. `snapshot` is the Git source of truth; everything the UI
/// shows is derived from it. Live refresh swaps the snapshot in later phases.
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
        }
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
            KeyCode::Char('j') | KeyCode::Down => Action::MoveDown,
            KeyCode::Char('k') | KeyCode::Up => Action::MoveUp,
            KeyCode::Char(c @ '1'..='6') => {
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
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
                if self.show_help {
                    self.active_tab = Tab::Help;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{RepoIdentity, WorktreeRecord};
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
}
