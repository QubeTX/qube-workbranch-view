//! Application state and the key → action → state reduction.

pub mod action;
pub mod overlay;
pub mod state;
pub mod transitions;
pub mod tree;

pub use action::Action;
pub use overlay::{Command, Confirm, ConfirmAction, Overlay, Palette, PendingGit, PendingKill};
pub use state::{AppState, LiveStatus, Tab};
pub use transitions::{TransitionKind, Transitions};
