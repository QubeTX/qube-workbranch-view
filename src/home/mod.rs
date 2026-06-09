//! Machine-wide "home" view (handoff §17.16).
//!
//! When `wb300` runs outside a Git repository (or with `--home` / `--multi`),
//! it opens a control tower over every repo being actively worked on across the
//! machine — discovered from running agents (Claude / Codex / …) and from a
//! bounded scan of the usual code-home directories — grouped by workbranch,
//! with the same live colour flashes and agent labels as the per-repo view.
//! Selecting a repo drills into its full per-repo view.

pub mod discovery;
pub mod snapshot;
pub mod state;

pub use discovery::{HomeConfig, home_state_dir};
pub use snapshot::{HomeSnapshot, repo_name, workbranch_label};
pub use state::HomeState;
