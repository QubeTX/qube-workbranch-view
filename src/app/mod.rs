//! Application state and the key → action → state reduction.

pub mod action;
pub mod state;

pub use action::Action;
pub use state::{AppState, Tab};
