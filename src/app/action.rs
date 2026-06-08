//! Actions: the normalized intents produced from input and applied to state.
//!
//! Input never mutates [`crate::app::AppState`] directly. A key is resolved to
//! an [`Action`] ([`crate::app::AppState::resolve_key`]) and then applied
//! ([`crate::app::AppState::apply`]). Keeping the reducer the single place state
//! changes is what lets the live engine and command palette feed it later.

use super::state::Tab;

/// A normalized user intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Do nothing.
    None,
    /// Quit the application.
    Quit,
    /// Move to the next tab.
    NextTab,
    /// Move to the previous tab.
    PrevTab,
    /// Jump directly to a tab.
    SelectTab(Tab),
    /// Move the selection down within the current list.
    MoveDown,
    /// Move the selection up within the current list.
    MoveUp,
    /// Re-capture the repository snapshot.
    Refresh,
    /// Toggle the help overlay.
    ToggleHelp,
}
