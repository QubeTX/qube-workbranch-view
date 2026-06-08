//! Git access layer.
//!
//! Everything here drives the installed `git` CLI through async subprocesses
//! (no `git2`/`gix` in v1) and parses NUL-safe porcelain output. `RepoSnapshot`
//! is the Git source of truth; the rest of the app derives its view from it.

pub mod commands;
pub mod diff;
pub mod refs;
pub mod repo;
pub mod snapshot;
pub mod status;
pub mod worktree;

pub use refs::BranchInfo;
pub use repo::RepoIdentity;
pub use snapshot::RepoSnapshot;
pub use status::WorktreeStatus;
pub use worktree::WorktreeRecord;
