//! Application state and the key → action → state reduction.

pub mod action;
pub mod state;
pub mod transitions;

pub use action::Action;
pub use state::{AppState, Tab};
pub use transitions::{TransitionKind, Transitions};
