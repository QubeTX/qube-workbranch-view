//! Transient UI highlights ("flashes") for live changes (handoff §12.9).
//!
//! Persistent state (dirty, collision) lives on the snapshot; these are the
//! short-lived overlays. Two visual tiers:
//!
//! - `Activity` — a frequent, short-lived *save* pulse: a blue marker that
//!   tracks the file being written and goes dark within ~½s of saves stopping.
//! - `Committed`/`Pushed` — milestone flashes that briefly recolor the whole
//!   row (magenta on commit, green on push).
//! - `Created`/`Deleted` — a worktree appearing / disappearing.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The kind of transient change to flash on a worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    Created,
    /// A file was created / modified / deleted inside the worktree (live save).
    Activity,
    /// The worktree's HEAD moved — a commit landed.
    Committed,
    Pushed,
    Deleted,
}

impl TransitionKind {
    /// How long this kind's highlight stays visible. `Activity` is deliberately
    /// short so the blue marker tracks live save state (dark ~½s after saves
    /// stop and "hops" to whichever worktree is being written); milestones flash
    /// "a second or two"; structural create/remove linger so they're not missed.
    fn ttl(self) -> Duration {
        match self {
            TransitionKind::Activity => Duration::from_millis(600),
            TransitionKind::Committed | TransitionKind::Pushed => Duration::from_millis(1800),
            TransitionKind::Created | TransitionKind::Deleted => Duration::from_millis(4000),
        }
    }

    /// Relative signal strength. A lower-priority flash must not clobber a
    /// higher-priority one that's still on screen (a constant `Activity` pulse
    /// must never hide a `Committed`/`Pushed` milestone).
    fn priority(self) -> u8 {
        match self {
            TransitionKind::Activity => 0,
            TransitionKind::Committed | TransitionKind::Pushed => 1,
            TransitionKind::Created | TransitionKind::Deleted => 2,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Transition {
    kind: TransitionKind,
    started: Instant,
    ttl: Duration,
}

/// Time-limited highlights keyed by worktree path. Each entry expires on its
/// own per-kind TTL (see [`TransitionKind::ttl`]).
#[derive(Debug, Default)]
pub struct Transitions {
    map: HashMap<String, Transition>,
}

impl Transitions {
    /// Record (or refresh) a highlight for `key`. A lower-priority kind is
    /// dropped if a higher-priority highlight is still active for the same key,
    /// so the frequent `Activity` pulse never stomps a live milestone flash.
    pub fn note(&mut self, key: String, kind: TransitionKind) {
        if let Some(existing) = self.map.get(&key)
            && existing.started.elapsed() < existing.ttl
            && existing.kind.priority() > kind.priority()
        {
            return;
        }
        self.map.insert(
            key,
            Transition {
                kind,
                started: Instant::now(),
                ttl: kind.ttl(),
            },
        );
    }

    /// The active (un-expired) highlight for `key`, if any.
    pub fn get(&self, key: &str) -> Option<TransitionKind> {
        self.map
            .get(key)
            .filter(|t| t.started.elapsed() < t.ttl)
            .map(|t| t.kind)
    }

    /// Drop expired highlights.
    pub fn expire(&mut self) {
        self.map.retain(|_, t| t.started.elapsed() < t.ttl);
    }

    /// Whether any highlight is currently active (used to decide if the UI needs
    /// to keep animating).
    pub fn any_active(&self) -> bool {
        self.map.values().any(|t| t.started.elapsed() < t.ttl)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_note_is_active() {
        let mut t = Transitions::default();
        t.note("/w".into(), TransitionKind::Activity);
        assert_eq!(t.get("/w"), Some(TransitionKind::Activity));
        assert!(t.any_active());
    }

    #[test]
    fn activity_does_not_clobber_a_live_milestone() {
        let mut t = Transitions::default();
        t.note("/w".into(), TransitionKind::Committed);
        t.note("/w".into(), TransitionKind::Activity); // lower priority, still live
        assert_eq!(t.get("/w"), Some(TransitionKind::Committed));
    }

    #[test]
    fn a_milestone_overrides_an_active_activity_pulse() {
        let mut t = Transitions::default();
        t.note("/w".into(), TransitionKind::Activity);
        t.note("/w".into(), TransitionKind::Pushed); // higher priority wins
        assert_eq!(t.get("/w"), Some(TransitionKind::Pushed));
    }

    #[test]
    fn the_latest_milestone_wins_over_an_equal_one() {
        let mut t = Transitions::default();
        t.note("/w".into(), TransitionKind::Committed);
        t.note("/w".into(), TransitionKind::Pushed); // equal priority, newest shows
        assert_eq!(t.get("/w"), Some(TransitionKind::Pushed));
    }

    #[test]
    fn unknown_key_has_no_highlight() {
        let t = Transitions::default();
        assert_eq!(t.get("/nope"), None);
        assert!(!t.any_active());
    }
}
