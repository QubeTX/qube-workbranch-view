//! Machine-wide "home" view.
//!
//! When `wb300` runs outside a Git repository (or with `--home` / `--multi`),
//! it opens a control tower over every repo being actively worked on across the
//! machine — discovered from running agents (Claude / Codex / …) and from a
//! bounded scan of the usual code-home directories. The view is one branch
//! tree with a repo node per repository, sharing the per-repo view's hierarchy,
//! live flashes, and agent labels. `Enter` drills into a repo's full view.

pub mod discovery;
pub mod snapshot;
pub mod state;

pub use discovery::{HomeConfig, home_state_dir};
pub use snapshot::{HomeSnapshot, repo_name};
pub use state::HomeState;
