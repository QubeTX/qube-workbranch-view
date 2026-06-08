//! Transient UI highlights ("flashes") for live changes (handoff §12.9).
//!
//! Persistent state (dirty, collision) lives on the snapshot; these are the
//! short-lived overlays — a worktree flashes when it's just been created,
//! modified, pushed, or deleted, then settles back to its persistent look.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The kind of transient change to flash. (`Pushed`/`Deleted` are produced by
/// later phases; defined here so the rendering vocabulary is complete.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    Created,
    Modified,
    Pushed,
    Deleted,
}

#[derive(Debug, Clone, Copy)]
struct Transition {
    kind: TransitionKind,
    started: Instant,
}

/// Time-limited highlights keyed by worktree path.
#[derive(Debug)]
pub struct Transitions {
    map: HashMap<String, Transition>,
    ttl: Duration,
}

impl Default for Transitions {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            ttl: Duration::from_millis(4000),
        }
    }
}

impl Transitions {
    /// Record (or refresh) a highlight for `key`.
    pub fn note(&mut self, key: String, kind: TransitionKind) {
        self.map.insert(
            key,
            Transition {
                kind,
                started: Instant::now(),
            },
        );
    }

    /// The active (un-expired) highlight for `key`, if any.
    pub fn get(&self, key: &str) -> Option<TransitionKind> {
        self.map
            .get(key)
            .filter(|t| t.started.elapsed() < self.ttl)
            .map(|t| t.kind)
    }

    /// Drop expired highlights.
    pub fn expire(&mut self) {
        let ttl = self.ttl;
        self.map.retain(|_, t| t.started.elapsed() < ttl);
    }

    /// Whether any highlight is currently active (used to decide if the UI needs
    /// to keep animating).
    pub fn any_active(&self) -> bool {
        self.map.values().any(|t| t.started.elapsed() < self.ttl)
    }
}
