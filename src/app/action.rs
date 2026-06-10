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
    /// Expand the selected tree node (or step into its first child).
    Expand,
    /// Collapse the selected tree node (or jump to its parent).
    Collapse,
    /// Toggle expansion of the selected tree node.
    ToggleExpand,
    /// Toggle between active-only and all branches in the tree.
    ToggleShowAll,
    /// Re-capture the repository snapshot.
    Refresh,
    /// Fetch from remotes (`git fetch --all --prune`).
    Fetch,
    /// Open the search / filter input.
    OpenSearch,
    /// Open the command palette.
    OpenPalette,
    /// Clear the active worktree filter.
    ClearFilter,
    /// Request removal of the selected worktree (opens a confirm dialog).
    RequestRemove,
    /// Prune stale worktree metadata.
    RequestPrune,
    /// Request killing the agent attached to the selected worktree (confirm).
    RequestKillAgent,
    /// Request killing the selected process in the Processes tab (confirm).
    RequestKillProcess,
    /// Move the selection down within the Processes list.
    ProcessDown,
    /// Move the selection up within the Processes list.
    ProcessUp,
    /// Type a character into the active overlay.
    InputChar(char),
    /// Delete the last character in the active overlay.
    InputBackspace,
    /// Submit the active overlay (run command / confirm / commit filter).
    InputSubmit,
    /// Cancel the active overlay.
    InputCancel,
    /// Toggle the help overlay.
    ToggleHelp,
}
