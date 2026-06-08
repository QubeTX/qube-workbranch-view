//! The application state and its reducer.

use crossterm::event::{KeyCode, KeyEvent};

use super::action::Action;

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

/// The whole UI state. `RepoSnapshot` (the Git source of truth) is layered in
/// from Phase 1 onward; for now this carries just enough to render the shell.
#[derive(Debug)]
pub struct AppState {
    /// Set once the user has asked to quit; the event loop checks it each turn.
    pub should_quit: bool,
    /// The currently focused tab.
    pub active_tab: Tab,
    /// Whether the help overlay is shown.
    pub show_help: bool,
    /// Human-readable label for the repository under inspection.
    pub repo_label: String,
}

impl AppState {
    /// Create the initial state for a repository.
    pub fn new(repo_label: String) -> Self {
        Self {
            should_quit: false,
            active_tab: Tab::Overview,
            show_help: false,
            repo_label,
        }
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
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn q_quits() {
        let mut app = AppState::new("repo".into());
        let action = app.resolve_key(key(KeyCode::Char('q')));
        assert_eq!(action, Action::Quit);
        app.apply(action);
        assert!(app.should_quit);
    }

    #[test]
    fn tab_cycles_forward_and_wraps() {
        let mut app = AppState::new("repo".into());
        assert_eq!(app.active_tab, Tab::Overview);
        for _ in 0..Tab::ALL.len() {
            let a = app.resolve_key(key(KeyCode::Tab));
            app.apply(a);
        }
        assert_eq!(app.active_tab, Tab::Overview); // wrapped fully around
    }

    #[test]
    fn back_tab_goes_to_last() {
        let mut app = AppState::new("repo".into());
        let a = app.resolve_key(key(KeyCode::BackTab));
        app.apply(a);
        assert_eq!(app.active_tab, Tab::Help);
    }

    #[test]
    fn number_selects_tab() {
        let mut app = AppState::new("repo".into());
        let a = app.resolve_key(key(KeyCode::Char('3')));
        app.apply(a);
        assert_eq!(app.active_tab, Tab::Processes);
    }

    #[test]
    fn help_toggles() {
        let mut app = AppState::new("repo".into());
        let a = app.resolve_key(key(KeyCode::Char('?')));
        app.apply(a);
        assert!(app.show_help);
        let a = app.resolve_key(key(KeyCode::Esc));
        app.apply(a);
        assert!(!app.show_help);
        assert!(!app.should_quit);
    }
}
