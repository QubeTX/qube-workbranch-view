//! Archived event records (serialized to events.jsonl).

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A structural change worth recording in the timeline/archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    WorktreeCreated,
    WorktreeRemoved,
}

impl EventKind {
    pub fn label(self) -> &'static str {
        match self {
            EventKind::WorktreeCreated => "created",
            EventKind::WorktreeRemoved => "removed",
        }
    }
}

/// Last-known dirty counts captured with a tombstone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirtySummary {
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicted: usize,
}

impl DirtySummary {
    pub fn is_clean(&self) -> bool {
        self.staged == 0 && self.unstaged == 0 && self.untracked == 0 && self.conflicted == 0
    }
}

/// One archived event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivedEvent {
    pub kind: EventKind,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty: Option<DirtySummary>,
    /// Unix epoch seconds when the event was observed.
    pub at_epoch: u64,
}

impl ArchivedEvent {
    /// Build an event stamped at the current time.
    pub fn new(
        kind: EventKind,
        path: String,
        branch: Option<String>,
        head: Option<String>,
        dirty: Option<DirtySummary>,
    ) -> Self {
        Self {
            kind,
            path,
            branch,
            head,
            dirty,
            at_epoch: epoch_secs(),
        }
    }

    /// Age in seconds relative to `now` (epoch seconds), saturating.
    pub fn age_secs(&self, now: u64) -> u64 {
        now.saturating_sub(self.at_epoch)
    }
}

/// Current time as Unix epoch seconds (0 if the clock is before the epoch).
pub fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
