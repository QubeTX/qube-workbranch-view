//! Modal input overlays (search, confirm, command palette) and the pending
//! Git side-effects they produce.

/// A Git mutation the event loop must run off the UI task (it can't `await`
/// inside the reducer).
#[derive(Debug, Clone)]
pub enum PendingGit {
    /// Remove a worktree; when `snapshot_first`, save a rescue patch first.
    RemoveWorktree {
        path: String,
        force: bool,
        snapshot_first: bool,
        label: String,
    },
    /// Prune stale worktree metadata.
    Prune,
}

/// A modal overlay captured above the normal key bindings.
#[derive(Debug, Clone, Default)]
pub enum Overlay {
    #[default]
    None,
    /// Live worktree filter being typed.
    Search { query: String },
    /// A type-the-name-to-confirm dialog for a destructive action.
    Confirm(Confirm),
    /// The command palette.
    Palette(Palette),
}

/// A destructive-action confirmation. The user must type `expected` exactly.
#[derive(Debug, Clone)]
pub struct Confirm {
    pub title: String,
    pub detail: Vec<String>,
    pub expected: String,
    pub typed: String,
    pub action: PendingGit,
}

/// Command-palette state.
#[derive(Debug, Clone, Default)]
pub struct Palette {
    pub query: String,
    pub selected: usize,
}

/// A command-palette entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Refresh,
    Fetch,
    Prune,
    RemoveSelected,
    Search,
}

impl Command {
    pub const ALL: &'static [Command] = &[
        Command::Refresh,
        Command::Fetch,
        Command::Prune,
        Command::RemoveSelected,
        Command::Search,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Command::Refresh => "refresh snapshot",
            Command::Fetch => "fetch from remotes",
            Command::Prune => "prune stale worktree metadata",
            Command::RemoveSelected => "remove selected worktree",
            Command::Search => "search / filter worktrees",
        }
    }
}

/// Palette commands whose label contains `query` (case-insensitive).
pub fn palette_filtered(query: &str) -> Vec<Command> {
    let q = query.to_lowercase();
    Command::ALL
        .iter()
        .copied()
        .filter(|c| c.label().to_lowercase().contains(&q))
        .collect()
}
