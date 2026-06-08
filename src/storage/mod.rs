//! Local persistence: the event archive (the "black box recorder", handoff §17.10).
//!
//! Events are appended as JSON lines to `<common_git_dir>/wb300/events.jsonl`,
//! so the Timeline survives across sessions.

pub mod event_store;
pub mod events;

pub use events::{ArchivedEvent, DirtySummary, EventKind};
