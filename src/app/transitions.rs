//! Transient UI highlights ("flashes") for live changes (handoff §12.9).
//!
//! Persistent state (uncommitted, collision) lives on the snapshot; these are
//! the short-lived overlays. Two visual tiers:
//!
//! - `Activity` — a frequent, short-lived *save* pulse: a blue marker that
//!   tracks the file being written and goes dark within ~½s of saves stopping.
//! - `Committed`/`Pushed` — milestone flashes that briefly recolor the whole
//!   row (magenta on commit, green on push). A back-to-back commit+push plays
//!   them as an ordered **sequence**: magenta, then green.
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
    /// stop); milestones flash "a second or two"; structural create/remove
    /// linger so they're not missed.
    fn ttl(self) -> Duration {
        match self {
            TransitionKind::Activity => Duration::from_millis(600),
            TransitionKind::Committed | TransitionKind::Pushed => Duration::from_millis(1800),
            TransitionKind::Created | TransitionKind::Deleted => Duration::from_millis(4000),
        }
    }

    /// Relative signal strength. A lower-priority flash must not clobber a
    /// higher-priority one that's still on screen (the constant `Activity` pulse
    /// must never hide a `Committed`/`Pushed` milestone).
    fn priority(self) -> u8 {
        match self {
            TransitionKind::Activity => 0,
            TransitionKind::Committed | TransitionKind::Pushed => 1,
            TransitionKind::Created | TransitionKind::Deleted => 2,
        }
    }
}

/// One highlight: an ordered sequence of kinds played back-to-back from
/// `started`, each for its own [`TransitionKind::ttl`]. A single change is a
/// one-element sequence; a commit+push is `[Committed, Pushed]`.
#[derive(Debug, Clone)]
struct Transition {
    seq: Vec<TransitionKind>,
    started: Instant,
}

impl Transition {
    /// The kind active `elapsed` into the sequence, if any (pure — testable).
    fn active_at(&self, elapsed: Duration) -> Option<TransitionKind> {
        let mut remaining = elapsed;
        for &kind in &self.seq {
            if remaining < kind.ttl() {
                return Some(kind);
            }
            remaining = remaining.saturating_sub(kind.ttl());
        }
        None
    }

    /// The kind active right now, if the sequence hasn't finished.
    fn active(&self) -> Option<TransitionKind> {
        self.active_at(self.started.elapsed())
    }
}

/// Time-limited highlights keyed by worktree path.
#[derive(Debug, Default)]
pub struct Transitions {
    map: HashMap<String, Transition>,
}

impl Transitions {
    /// Record (or refresh) a single-kind highlight for `key`.
    pub fn note(&mut self, key: String, kind: TransitionKind) {
        self.note_seq(key, vec![kind]);
    }

    /// Record an ordered sequence of highlights for `key` (e.g. commit→push).
    /// A lower-priority sequence is dropped if a higher-priority highlight is
    /// still active for the same key, so the frequent `Activity` pulse never
    /// stomps a live milestone flash.
    pub fn note_seq(&mut self, key: String, seq: Vec<TransitionKind>) {
        let Some(&first) = seq.first() else {
            return;
        };
        if let Some(existing) = self.map.get(&key)
            && let Some(active) = existing.active()
            && active.priority() > first.priority()
        {
            return;
        }
        self.map.insert(
            key,
            Transition {
                seq,
                started: Instant::now(),
            },
        );
    }

    /// The active (un-expired) highlight for `key`, if any.
    pub fn get(&self, key: &str) -> Option<TransitionKind> {
        self.map.get(key).and_then(Transition::active)
    }

    /// Drop finished highlights.
    pub fn expire(&mut self) {
        self.map.retain(|_, t| t.active().is_some());
    }

    /// Whether any highlight is currently active (used to decide if the UI needs
    /// to keep animating).
    pub fn any_active(&self) -> bool {
        self.map.values().any(|t| t.active().is_some())
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
    fn unknown_key_has_no_highlight() {
        let t = Transitions::default();
        assert_eq!(t.get("/nope"), None);
        assert!(!t.any_active());
    }

    #[test]
    fn a_sequence_plays_committed_then_pushed_then_clears() {
        let seq = vec![TransitionKind::Committed, TransitionKind::Pushed];
        let trans = Transition {
            seq: seq.clone(),
            started: Instant::now(),
        };
        // Start of the window → magenta (committed).
        assert_eq!(
            trans.active_at(Duration::ZERO),
            Some(TransitionKind::Committed)
        );
        // Just into the second window → green (pushed).
        let into_push = TransitionKind::Committed.ttl() + Duration::from_millis(1);
        assert_eq!(trans.active_at(into_push), Some(TransitionKind::Pushed));
        // Past both windows → cleared.
        let past_all = TransitionKind::Committed.ttl()
            + TransitionKind::Pushed.ttl()
            + Duration::from_millis(1);
        assert_eq!(trans.active_at(past_all), None);
    }
}
