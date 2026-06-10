//! OS toast notifications — the only notification channel.
//!
//! Fires on exactly three things: a branch committed, a branch's work reached
//! its remote, and a new merge-conflict risk. Never on agent exit or idle.
//!
//! Architecture: the reducers stay pure — snapshot ingest yields archived
//! events, the event loops map them to [`NotifyEvent`]s and `try_send` them
//! (dropping on backpressure: a missed toast is acceptable, a blocked UI is
//! not), and one spawned notifier task owns policy (coalescing, cooldown,
//! per-kind gating) and the blocking toast call (via `spawn_blocking`).
//!
//! Windows: WinRT toasts need an AppUserModelID. We self-register
//! `HKCU\Software\Classes\AppUserModelId\QubeTX.WB300` (no admin needed) and
//! tag toasts with it; if that path fails we retry untagged (notify-rust's
//! PowerShell AUMID fallback — the toast then shows as "Windows PowerShell").
//! Any persistent backend failure logs one warning and disables toasts for
//! the session — never crash, never block.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::config::NotifyConfig;
use crate::storage::{ArchivedEvent, EventKind};

/// Collect-then-emit window: events arriving within this of the first are
/// folded into one batch ("3 branches pushed").
const COALESCE_WINDOW: Duration = Duration::from_millis(1500);

/// Max branch names spelled out in a batched toast body.
const BATCH_NAMES: usize = 3;

/// A notification-worthy milestone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyEvent {
    pub kind: NotifyKind,
    /// Repo display name (e.g. `qube-workbranch-view`).
    pub repo: String,
    /// What the event is about: a branch name, or `file (A × B)` for risks.
    pub subject: String,
    /// One-line detail for single-event toasts.
    pub detail: String,
}

/// Exactly the operator-chosen trigger set — no agent-exit, no idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotifyKind {
    Commit,
    Push,
    ConflictRisk,
}

impl NotifyKind {
    fn plural_verb(self) -> &'static str {
        match self {
            NotifyKind::Commit => "committed",
            NotifyKind::Push => "pushed",
            NotifyKind::ConflictRisk => "merge-conflict risks",
        }
    }
}

/// One toast ready to display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub title: String,
    pub body: String,
}

/// Map an archived event to its notification, if it is notification-worthy.
/// `fallback_repo` names the repo when the event wasn't stamped (per-repo mode).
pub fn from_archived(ev: &ArchivedEvent, fallback_repo: &str) -> Option<NotifyEvent> {
    let repo = ev.repo.clone().unwrap_or_else(|| fallback_repo.to_string());
    match ev.kind {
        EventKind::BranchCommitted => {
            let branch = ev.branch.clone()?;
            let detail = match &ev.files {
                Some(files) if !files.is_empty() => {
                    let mut preview = files
                        .iter()
                        .take(BATCH_NAMES)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ");
                    if files.len() > BATCH_NAMES {
                        preview.push_str(", …");
                    }
                    format!("{branch} committed — {preview}")
                }
                _ => format!("{branch} committed"),
            };
            Some(NotifyEvent {
                kind: NotifyKind::Commit,
                repo,
                subject: branch,
                detail,
            })
        }
        EventKind::BranchPushed => {
            let branch = ev.branch.clone()?;
            Some(NotifyEvent {
                kind: NotifyKind::Push,
                repo,
                detail: format!("{branch} pushed"),
                subject: branch,
            })
        }
        EventKind::ConflictRisk => {
            // For conflict events, `path` is the file and `branch` is "A × B".
            let who = ev.branch.clone().unwrap_or_default();
            Some(NotifyEvent {
                kind: NotifyKind::ConflictRisk,
                repo,
                subject: format!("{} ({who})", ev.path),
                detail: format!("merge-conflict risk: {} ({who})", ev.path),
            })
        }
        _ => None,
    }
}

/// Pure notification policy: per-kind gating, per-(repo, subject, kind)
/// cooldown, and same-kind batching. Separated from the OS backend so it is
/// fully unit-testable.
#[derive(Debug)]
pub struct NotifyPolicy {
    cfg: NotifyConfig,
    last: HashMap<String, Instant>,
}

impl NotifyPolicy {
    pub fn new(cfg: NotifyConfig) -> Self {
        Self {
            cfg,
            last: HashMap::new(),
        }
    }

    fn kind_enabled(&self, kind: NotifyKind) -> bool {
        match kind {
            NotifyKind::Commit => self.cfg.commit,
            NotifyKind::Push => self.cfg.push,
            NotifyKind::ConflictRisk => self.cfg.conflict_risk,
        }
    }

    /// Filter a batch through gating + cooldown, then fold what survives into
    /// toasts (one per kind: single events verbatim, multiples summarized).
    pub fn coalesce(&mut self, batch: Vec<NotifyEvent>, now: Instant) -> Vec<Toast> {
        let cooldown = Duration::from_secs(self.cfg.cooldown_secs);
        let mut admitted: Vec<NotifyEvent> = Vec::new();
        for ev in batch {
            if !self.kind_enabled(ev.kind) {
                continue;
            }
            let key = format!("{}\u{1f}{}\u{1f}{:?}", ev.repo, ev.subject, ev.kind);
            match self.last.get(&key) {
                Some(&at) if now.duration_since(at) < cooldown => continue,
                _ => {
                    self.last.insert(key, now);
                    admitted.push(ev);
                }
            }
        }
        // Bound the cooldown map (long sessions, many branches).
        if self.last.len() > 512 {
            self.last
                .retain(|_, &mut at| now.duration_since(at) < cooldown);
        }

        let mut toasts = Vec::new();
        for kind in [
            NotifyKind::Commit,
            NotifyKind::Push,
            NotifyKind::ConflictRisk,
        ] {
            let group: Vec<&NotifyEvent> = admitted.iter().filter(|e| e.kind == kind).collect();
            match group.len() {
                0 => {}
                1 => {
                    let ev = group[0];
                    toasts.push(Toast {
                        title: format!("WB-300 · {}", ev.repo),
                        body: ev.detail.clone(),
                    });
                }
                n => {
                    let mut names: Vec<&str> = group
                        .iter()
                        .take(BATCH_NAMES)
                        .map(|e| e.subject.as_str())
                        .collect();
                    if n > BATCH_NAMES {
                        names.push("…");
                    }
                    let noun = if kind == NotifyKind::ConflictRisk {
                        format!("{n} new {}", kind.plural_verb())
                    } else {
                        format!("{n} branches {}", kind.plural_verb())
                    };
                    toasts.push(Toast {
                        title: "WB-300".to_string(),
                        body: format!("{noun} ({})", names.join(", ")),
                    });
                }
            }
        }
        toasts
    }
}

/// Spawn the notifier task: receives events, batches them for
/// [`COALESCE_WINDOW`], applies policy, and shows toasts off the async
/// executor. Returns immediately; drop the sender to stop it.
pub fn spawn_notifier(
    cfg: NotifyConfig,
    mut rx: mpsc::Receiver<NotifyEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if !cfg.enabled {
            // Drain so senders never error; emit nothing.
            while rx.recv().await.is_some() {}
            return;
        }
        let mut policy = NotifyPolicy::new(cfg);
        let mut backend_alive = true;
        #[cfg(windows)]
        if let Err(err) = register_aumid() {
            tracing::warn!(
                "toast AUMID registration failed (toasts may show as PowerShell): {err}"
            );
        }

        while let Some(first) = rx.recv().await {
            let mut batch = vec![first];
            let deadline = tokio::time::sleep(COALESCE_WINDOW);
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    _ = &mut deadline => break,
                    more = rx.recv() => match more {
                        Some(ev) => batch.push(ev),
                        None => break,
                    },
                }
            }
            let toasts = policy.coalesce(batch, Instant::now());
            if !backend_alive {
                continue; // keep draining + tracking cooldowns, emit nothing
            }
            for toast in toasts {
                let shown = tokio::task::spawn_blocking(move || show_toast(&toast)).await;
                match shown {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => {
                        tracing::warn!("toast backend failed — disabling for this session: {err}");
                        backend_alive = false;
                        break;
                    }
                    Err(err) => {
                        tracing::warn!("toast task panicked — disabling: {err}");
                        backend_alive = false;
                        break;
                    }
                }
            }
        }
    })
}

/// Our Windows AppUserModelID (registered under HKCU at startup).
#[cfg(windows)]
const AUMID: &str = "QubeTX.WB300";

/// Register the AUMID so toasts display as "WB-300" instead of PowerShell.
/// HKCU only — no elevation. (The installers may own this later; runtime
/// registration keeps cargo/shell installs covered.)
#[cfg(windows)]
fn register_aumid() -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(r"Software\Classes\AppUserModelId\QubeTX.WB300")?;
    key.set_value("DisplayName", &"WB-300")?;
    Ok(())
}

/// Show one toast, blocking. On Windows, try the registered AUMID first and
/// fall back to an untagged toast (PowerShell identity) before failing.
fn show_toast(toast: &Toast) -> Result<(), String> {
    #[cfg(windows)]
    {
        let mut tagged = notify_rust::Notification::new();
        tagged
            .summary(&toast.title)
            .body(&toast.body)
            .appname("WB-300")
            .app_id(AUMID);
        if tagged.show().is_ok() {
            return Ok(());
        }
    }
    let mut plain = notify_rust::Notification::new();
    plain
        .summary(&toast.title)
        .body(&toast.body)
        .appname("WB-300");
    plain.show().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> NotifyConfig {
        NotifyConfig::default()
    }

    fn ev(kind: NotifyKind, repo: &str, subject: &str) -> NotifyEvent {
        NotifyEvent {
            kind,
            repo: repo.to_string(),
            subject: subject.to_string(),
            detail: format!("{subject} {}", kind.plural_verb()),
        }
    }

    #[test]
    fn single_event_becomes_a_repo_titled_toast() {
        let mut p = NotifyPolicy::new(cfg());
        let toasts = p.coalesce(vec![ev(NotifyKind::Push, "qwv", "feat/x")], Instant::now());
        assert_eq!(toasts.len(), 1);
        assert_eq!(toasts[0].title, "WB-300 · qwv");
        assert!(toasts[0].body.contains("feat/x"));
    }

    #[test]
    fn same_kind_events_batch_into_one_toast() {
        let mut p = NotifyPolicy::new(cfg());
        let now = Instant::now();
        let toasts = p.coalesce(
            vec![
                ev(NotifyKind::Push, "qwv", "feat/a"),
                ev(NotifyKind::Push, "tr300", "feat/b"),
                ev(NotifyKind::Push, "qwv", "feat/c"),
                ev(NotifyKind::Push, "qwv", "feat/d"),
            ],
            now,
        );
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0].body.starts_with("4 branches pushed"));
        assert!(toasts[0].body.contains("…"), "overflow names elided");
    }

    #[test]
    fn mixed_kinds_become_one_toast_per_kind() {
        let mut p = NotifyPolicy::new(cfg());
        let toasts = p.coalesce(
            vec![
                ev(NotifyKind::Commit, "qwv", "feat/a"),
                ev(NotifyKind::Push, "qwv", "feat/a"),
            ],
            Instant::now(),
        );
        assert_eq!(toasts.len(), 2);
    }

    #[test]
    fn cooldown_suppresses_repeats_within_the_window() {
        let mut p = NotifyPolicy::new(cfg());
        let now = Instant::now();
        assert_eq!(
            p.coalesce(vec![ev(NotifyKind::Commit, "qwv", "feat/a")], now)
                .len(),
            1
        );
        // Same branch, same kind, still inside the 30s window → silent.
        assert!(
            p.coalesce(vec![ev(NotifyKind::Commit, "qwv", "feat/a")], now)
                .is_empty()
        );
        // After the window it fires again.
        let later = now + Duration::from_secs(31);
        assert_eq!(
            p.coalesce(vec![ev(NotifyKind::Commit, "qwv", "feat/a")], later)
                .len(),
            1
        );
    }

    #[test]
    fn per_kind_gating_silences_disabled_kinds() {
        let mut config = cfg();
        config.push = false;
        let mut p = NotifyPolicy::new(config);
        let toasts = p.coalesce(
            vec![
                ev(NotifyKind::Push, "qwv", "feat/a"),
                ev(NotifyKind::Commit, "qwv", "feat/b"),
            ],
            Instant::now(),
        );
        assert_eq!(toasts.len(), 1);
        assert!(toasts[0].body.contains("committed"));
    }

    #[test]
    fn maps_archived_branch_and_conflict_events() {
        let mut commit = ArchivedEvent::new(
            EventKind::BranchCommitted,
            "/wt".into(),
            Some("feat/x".into()),
            Some("abcd1234".into()),
            None,
        );
        commit.files = Some(vec!["src/a.rs".into(), "src/b.rs".into()]);
        let n = from_archived(&commit, "qwv").expect("commit maps");
        assert_eq!(n.kind, NotifyKind::Commit);
        assert_eq!(n.repo, "qwv");
        assert!(n.detail.contains("src/a.rs"));

        let conflict = ArchivedEvent::new(
            EventKind::ConflictRisk,
            "src/db/pool.rs".into(),
            Some("feat/a × feat/b".into()),
            None,
            None,
        );
        let n = from_archived(&conflict, "qwv").expect("conflict maps");
        assert_eq!(n.kind, NotifyKind::ConflictRisk);
        assert!(n.detail.contains("src/db/pool.rs"));
        assert!(n.subject.contains("feat/a × feat/b"));

        let created =
            ArchivedEvent::new(EventKind::WorktreeCreated, "/wt".into(), None, None, None);
        assert!(from_archived(&created, "qwv").is_none(), "not a milestone");
    }

    #[test]
    fn stamped_repo_wins_over_the_fallback() {
        let mut pushed = ArchivedEvent::new(
            EventKind::BranchPushed,
            String::new(),
            Some("feat/x".into()),
            None,
            None,
        );
        pushed.repo = Some("other-repo".into());
        let n = from_archived(&pushed, "qwv").unwrap();
        assert_eq!(n.repo, "other-repo");
    }
}
