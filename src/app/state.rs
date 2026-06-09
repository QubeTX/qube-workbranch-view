//! The application state and its reducer.

use std::collections::{HashMap, HashSet, VecDeque};

use crossterm::event::{KeyCode, KeyEvent};

use super::action::Action;
use super::overlay::{Command, Confirm, Overlay, Palette, PendingGit, palette_filtered};
use super::transitions::{TransitionKind, Transitions};
use crate::git::{RepoSnapshot, WorktreeRecord};
use crate::storage::{ArchivedEvent, DirtySummary, EventKind};

/// In-memory cap on archived events (matches the on-disk prune cap).
const MAX_ARCHIVE: usize = 2000;

/// Top-level views. Mirrors the design's tab set; Timeline arrives with the
/// event archive in a later phase (handoff §14.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Worktrees,
    Processes,
    Collisions,
    Cleanup,
    Timeline,
    Help,
}

impl Tab {
    /// All tabs, in display order.
    pub const ALL: [Tab; 7] = [
        Tab::Overview,
        Tab::Worktrees,
        Tab::Processes,
        Tab::Collisions,
        Tab::Cleanup,
        Tab::Timeline,
        Tab::Help,
    ];

    /// The short title shown in the tab bar.
    pub fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Worktrees => "Worktrees",
            Tab::Processes => "Processes",
            Tab::Collisions => "Collisions",
            Tab::Cleanup => "Cleanup",
            Tab::Timeline => "Timeline",
            Tab::Help => "Help",
        }
    }

    /// Position in [`Tab::ALL`].
    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }

    fn next(self) -> Tab {
        let i = self.index();
        Tab::ALL[(i + 1) % Tab::ALL.len()]
    }

    fn prev(self) -> Tab {
        let i = self.index();
        Tab::ALL[(i + Tab::ALL.len() - 1) % Tab::ALL.len()]
    }
}

/// Whether live updating is active (drives the header indicator).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveStatus {
    /// Filesystem watcher active, plus the poll backstop.
    Live,
    /// Watcher unavailable; periodic poll only.
    PollOnly,
    /// `--no-live`: no watcher and no poll (manual refresh only).
    Static,
}

/// The whole UI state. `snapshot` is the Git source of truth; everything the UI
/// shows is derived from it.
#[derive(Debug)]
pub struct AppState {
    /// Set once the user has asked to quit; the event loop checks it each turn.
    pub should_quit: bool,
    /// The currently focused tab.
    pub active_tab: Tab,
    /// Whether the help overlay is shown.
    pub show_help: bool,
    /// The captured Git state.
    pub snapshot: RepoSnapshot,
    /// Selected index within the worktree list.
    pub selected: usize,
    /// Transient highlights for live changes.
    pub transitions: Transitions,
    /// Set by `apply(Refresh)`; consumed by the event loop to trigger a capture.
    pending_refresh: bool,
    /// Whether live updating is active (for the header indicator).
    pub live: LiveStatus,
    /// Archived structural events (newest first, capped) — the Timeline.
    pub archive: VecDeque<ArchivedEvent>,
    /// Set by `apply(Fetch)`; consumed by the event loop.
    pending_fetch: bool,
    /// True while a `git fetch` is running (header indicator).
    pub fetching: bool,
    /// Epoch seconds of the last successful remote check, if any.
    pub remote_checked: Option<u64>,
    /// The active modal overlay (search / confirm / palette).
    pub overlay: Overlay,
    /// Committed worktree filter (from search).
    pub filter: Option<String>,
    /// A pending Git mutation for the event loop to run, if any.
    pending_git: Option<PendingGit>,
    /// True when the most recent snapshot capture failed — the board is showing
    /// stale data. Surfaced in the header so "live" never silently lies.
    pub stale: bool,
}

impl AppState {
    /// Create the initial state from a captured snapshot.
    pub fn new(snapshot: RepoSnapshot) -> Self {
        let selected = snapshot.current_worktree_index().unwrap_or(0);
        Self {
            should_quit: false,
            active_tab: Tab::Overview,
            show_help: false,
            snapshot,
            selected,
            transitions: Transitions::default(),
            pending_refresh: false,
            live: LiveStatus::Static,
            archive: VecDeque::new(),
            pending_fetch: false,
            fetching: false,
            remote_checked: None,
            overlay: Overlay::None,
            filter: None,
            pending_git: None,
            stale: false,
        }
    }

    /// Replace the snapshot (after a manual/live refresh), clamping selection.
    pub fn set_snapshot(&mut self, snapshot: RepoSnapshot) {
        self.snapshot = snapshot;
        self.clamp_selection();
    }

    /// Clamp the selection to the visible (filtered) worktree count.
    fn clamp_selection(&mut self) {
        let len = self.visible_indices().len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    /// Original worktree indices matching the active filter (all if unfiltered).
    pub fn visible_indices(&self) -> Vec<usize> {
        match &self.filter {
            None => (0..self.snapshot.worktrees.len()).collect(),
            Some(query) => {
                let q = query.to_lowercase();
                self.snapshot
                    .worktrees
                    .iter()
                    .enumerate()
                    .filter(|(_, wt)| {
                        wt.display_name().to_lowercase().contains(&q)
                            || wt.path.to_lowercase().contains(&q)
                    })
                    .map(|(i, _)| i)
                    .collect()
            }
        }
    }

    /// The original worktree index currently selected (within the filtered view).
    pub fn selected_original_index(&self) -> Option<usize> {
        self.visible_indices().get(self.selected).copied()
    }

    /// Whether worktree `idx` is protected from removal (main / current / bare).
    fn is_protected(&self, idx: usize) -> bool {
        idx == 0
            || self.snapshot.current_worktree_index() == Some(idx)
            || self.snapshot.worktrees.get(idx).is_some_and(|w| w.bare)
    }

    /// Consume a pending Git mutation for the event loop to execute.
    pub fn take_pending_git(&mut self) -> Option<PendingGit> {
        self.pending_git.take()
    }

    /// Replace the archive (loaded at startup). On disk events are oldest-first;
    /// we store them newest-first for display, capped to [`MAX_ARCHIVE`].
    pub fn set_events(&mut self, events: Vec<ArchivedEvent>) {
        self.archive = events.into_iter().rev().collect();
        self.truncate_archive();
    }

    /// Cap the in-memory archive to the most recent [`MAX_ARCHIVE`] events.
    fn truncate_archive(&mut self) {
        while self.archive.len() > MAX_ARCHIVE {
            self.archive.pop_back();
        }
    }

    /// Diff the new snapshot against the current one: raise transient highlights,
    /// record created/removed worktrees in the archive, swap the snapshot in, and
    /// return the newly-detected events for the caller to persist.
    pub fn ingest_snapshot(&mut self, new: RepoSnapshot) -> Vec<ArchivedEvent> {
        let mut new_events: Vec<ArchivedEvent> = Vec::new();
        let mut notes: Vec<(String, TransitionKind)> = Vec::new();
        {
            let previous: HashMap<&str, &WorktreeRecord> = self
                .snapshot
                .worktrees
                .iter()
                .map(|wt| (wt.path.as_str(), wt))
                .collect();
            for wt in &new.worktrees {
                match previous.get(wt.path.as_str()) {
                    None => {
                        notes.push((wt.path.clone(), TransitionKind::Created));
                        new_events.push(event_from(EventKind::WorktreeCreated, wt));
                    }
                    Some(old) => {
                        if let Some(kind) = change_kind(old, wt) {
                            notes.push((wt.path.clone(), kind));
                        }
                    }
                }
            }
            let incoming: HashSet<&str> = new.worktrees.iter().map(|wt| wt.path.as_str()).collect();
            for old in &self.snapshot.worktrees {
                if !old.bare && !incoming.contains(old.path.as_str()) {
                    notes.push((old.path.clone(), TransitionKind::Deleted));
                    new_events.push(event_from(EventKind::WorktreeRemoved, old));
                }
            }
        }

        for (path, kind) in notes {
            self.transitions.note(path, kind);
        }
        // Prepend so the archive stays newest-first; reverse keeps in-batch order.
        for event in new_events.iter().rev() {
            self.archive.push_front(event.clone());
        }
        self.truncate_archive();
        self.set_snapshot(new);
        new_events
    }

    /// The active transient highlight for a worktree path, if any.
    pub fn transition_for(&self, path: &str) -> Option<TransitionKind> {
        self.transitions.get(path)
    }

    /// Flash live save activity for a batch of changed filesystem paths.
    pub fn note_activity(&mut self, paths: &[std::path::PathBuf]) {
        note_activity_for(&mut self.transitions, &self.snapshot.worktrees, paths);
    }

    /// Drop expired transient highlights (driven by the animation tick).
    pub fn expire_transitions(&mut self) {
        self.transitions.expire();
    }

    /// Consume the pending-refresh flag set by `apply(Action::Refresh)`.
    pub fn take_pending_refresh(&mut self) -> bool {
        std::mem::take(&mut self.pending_refresh)
    }

    /// Record whether live updating is active (set once by the event loop).
    pub fn set_live_status(&mut self, status: LiveStatus) {
        self.live = status;
    }

    /// Record whether the most recent snapshot capture failed (header flag).
    pub fn set_stale(&mut self, stale: bool) {
        self.stale = stale;
    }

    /// Consume the pending-fetch flag set by `apply(Action::Fetch)`.
    pub fn take_pending_fetch(&mut self) -> bool {
        std::mem::take(&mut self.pending_fetch)
    }

    /// Mark a fetch as in-progress (header indicator).
    pub fn set_fetching(&mut self, fetching: bool) {
        self.fetching = fetching;
    }

    /// Record the time of the last remote check.
    pub fn set_remote_checked(&mut self, epoch_secs: u64) {
        self.remote_checked = Some(epoch_secs);
    }

    /// The worktrees from the current snapshot.
    pub fn worktrees(&self) -> &[WorktreeRecord] {
        &self.snapshot.worktrees
    }

    /// The currently selected worktree, if any (within the filtered view).
    pub fn selected_worktree(&self) -> Option<&WorktreeRecord> {
        let idx = self.selected_original_index()?;
        self.snapshot.worktrees.get(idx)
    }

    /// Human-readable label for the repository under inspection.
    pub fn repo_label(&self) -> String {
        self.snapshot.repo.root.display().to_string()
    }

    /// Resolve a key press to an [`Action`] given the current context.
    ///
    /// Bindings match the design defaults (handoff §16); the full configurable
    /// resolver arrives with the config subsystem.
    pub fn resolve_key(&self, key: KeyEvent) -> Action {
        if matches!(self.overlay, Overlay::None) {
            self.resolve_normal(key)
        } else {
            resolve_input(key)
        }
    }

    fn resolve_normal(&self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Esc if self.show_help => Action::ToggleHelp,
            KeyCode::Esc if self.filter.is_some() => Action::ClearFilter,
            KeyCode::Esc => Action::Quit,
            KeyCode::Char('?') => Action::ToggleHelp,
            KeyCode::Tab => Action::NextTab,
            KeyCode::BackTab => Action::PrevTab,
            KeyCode::Char('r') => Action::Refresh,
            KeyCode::Char('f') => Action::Fetch,
            KeyCode::Char('/') => Action::OpenSearch,
            KeyCode::Char(':') => Action::OpenPalette,
            KeyCode::Char('x') => Action::RequestRemove,
            KeyCode::Char('p') => Action::RequestPrune,
            KeyCode::Char('j') | KeyCode::Down => Action::MoveDown,
            KeyCode::Char('k') | KeyCode::Up => Action::MoveUp,
            KeyCode::Char(c @ '1'..='7') => {
                let idx = (c as u8 - b'1') as usize;
                Action::SelectTab(Tab::ALL[idx])
            }
            _ => Action::None,
        }
    }

    /// Apply an [`Action`], mutating state. The single place state changes.
    pub fn apply(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::Quit => self.should_quit = true,
            Action::NextTab => self.active_tab = self.active_tab.next(),
            Action::PrevTab => self.active_tab = self.active_tab.prev(),
            Action::SelectTab(tab) => self.active_tab = tab,
            Action::MoveDown => self.move_selection(1),
            Action::MoveUp => self.move_selection(-1),
            // The async work runs in the event loop; the reducer records intent.
            Action::Refresh => self.pending_refresh = true,
            Action::Fetch => self.pending_fetch = true,
            Action::OpenSearch => {
                self.overlay = Overlay::Search {
                    query: self.filter.clone().unwrap_or_default(),
                };
            }
            Action::OpenPalette => self.overlay = Overlay::Palette(Palette::default()),
            Action::ClearFilter => {
                self.filter = None;
                self.clamp_selection();
            }
            Action::RequestRemove => self.open_remove_confirm(),
            Action::RequestPrune => self.open_prune_confirm(),
            Action::InputChar(c) => self.input_char(c),
            Action::InputBackspace => self.input_backspace(),
            Action::InputSubmit => self.input_submit(),
            Action::InputCancel => self.overlay = Overlay::None,
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
                if self.show_help {
                    self.active_tab = Tab::Help;
                }
            }
        }
    }

    /// Move the selection within the active list (palette or worktrees).
    fn move_selection(&mut self, delta: i32) {
        if let Overlay::Palette(p) = &mut self.overlay {
            let len = palette_filtered(&p.query).len();
            if len > 0 {
                p.selected = (p.selected as i32 + delta).clamp(0, len as i32 - 1) as usize;
            }
            return;
        }
        let len = self.visible_indices().len();
        if len > 0 {
            self.selected = (self.selected as i32 + delta).clamp(0, len as i32 - 1) as usize;
        }
    }

    fn input_char(&mut self, c: char) {
        match &mut self.overlay {
            Overlay::Search { query } => query.push(c),
            Overlay::Palette(p) => {
                p.query.push(c);
                p.selected = 0;
            }
            Overlay::Confirm(cf) => cf.typed.push(c),
            Overlay::None => {}
        }
        self.sync_search_filter();
    }

    fn input_backspace(&mut self) {
        match &mut self.overlay {
            Overlay::Search { query } => {
                query.pop();
            }
            Overlay::Palette(p) => {
                p.query.pop();
                p.selected = 0;
            }
            Overlay::Confirm(cf) => {
                cf.typed.pop();
            }
            Overlay::None => {}
        }
        self.sync_search_filter();
    }

    /// Keep the committed filter in sync with the search buffer being typed.
    fn sync_search_filter(&mut self) {
        let query = match &self.overlay {
            Overlay::Search { query } => Some(query.clone()),
            _ => None,
        };
        if let Some(query) = query {
            self.filter = (!query.is_empty()).then_some(query);
            self.clamp_selection();
        }
    }

    fn input_submit(&mut self) {
        match std::mem::take(&mut self.overlay) {
            Overlay::Search { query } => {
                self.filter = (!query.is_empty()).then_some(query);
                self.clamp_selection();
            }
            Overlay::Palette(p) => {
                if let Some(&command) = palette_filtered(&p.query).get(p.selected) {
                    self.run_command(command);
                }
            }
            Overlay::Confirm(cf) => {
                if cf.typed.trim() == cf.expected {
                    self.pending_git = Some(cf.action);
                } else {
                    self.overlay = Overlay::Confirm(cf); // wrong name — keep open
                }
            }
            Overlay::None => {}
        }
    }

    fn run_command(&mut self, command: Command) {
        match command {
            Command::Refresh => self.pending_refresh = true,
            Command::Fetch => self.pending_fetch = true,
            Command::Prune => self.open_prune_confirm(),
            Command::RemoveSelected => self.open_remove_confirm(),
            Command::Search => {
                self.overlay = Overlay::Search {
                    query: self.filter.clone().unwrap_or_default(),
                };
            }
        }
    }

    /// Open a type-to-confirm dialog to remove the selected worktree, unless it
    /// is protected or has a running process.
    fn open_remove_confirm(&mut self) {
        let Some(idx) = self.selected_original_index() else {
            return;
        };
        if self.is_protected(idx) || self.snapshot.processes.worktree_is_active(idx) {
            return;
        }
        let (path, label, expected, clean, status_line) = {
            let Some(wt) = self.snapshot.worktrees.get(idx) else {
                return;
            };
            let clean = wt.status.as_ref().is_some_and(|s| s.clean);
            let branch = wt.branch_short().map(str::to_string);
            // A typeable confirmation token: branch name, else short HEAD oid,
            // else a fixed word (detached / unknown worktrees).
            let expected = branch
                .clone()
                .or_else(|| wt.short_head().map(str::to_string))
                .unwrap_or_else(|| "REMOVE".to_string());
            let label = branch.unwrap_or_else(|| wt.display_name());
            let status_line = wt.status.as_ref().map(|s| {
                format!(
                    "status: {} staged · {} unstaged · {} untracked",
                    s.staged, s.unstaged, s.untracked
                )
            });
            (wt.path.clone(), label, expected, clean, status_line)
        };
        let mut detail = vec![format!("path: {path}"), format!("branch: {label}")];
        if let Some(line) = status_line {
            detail.push(line);
        }
        detail.push(String::new());
        detail.push(if clean {
            "Clean — safe to remove.".to_string()
        } else {
            "DIRTY — a rescue snapshot will be saved first.".to_string()
        });
        detail.push(format!("Type \"{expected}\" to confirm:"));
        self.overlay = Overlay::Confirm(Confirm {
            title: "Remove worktree?".to_string(),
            detail,
            expected,
            typed: String::new(),
            action: PendingGit::RemoveWorktree {
                path,
                force: !clean,
                snapshot_first: !clean,
                label,
            },
        });
    }

    /// Open a type-to-confirm dialog to prune stale worktree metadata.
    fn open_prune_confirm(&mut self) {
        self.overlay = Overlay::Confirm(Confirm {
            title: "Prune worktree metadata?".to_string(),
            detail: vec![
                "Removes git's bookkeeping for worktrees whose directory is gone.".to_string(),
                "Working trees on disk are not touched.".to_string(),
                String::new(),
                "Type \"prune\" to confirm:".to_string(),
            ],
            expected: "prune".to_string(),
            typed: String::new(),
            action: PendingGit::Prune,
        });
    }
}

/// Map a key to an overlay-editing action while a modal is open.
fn resolve_input(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Esc => Action::InputCancel,
        KeyCode::Enter => Action::InputSubmit,
        KeyCode::Backspace => Action::InputBackspace,
        KeyCode::Up => Action::MoveUp,
        KeyCode::Down => Action::MoveDown,
        KeyCode::Char(c) => Action::InputChar(c),
        _ => Action::None,
    }
}

/// Build an [`ArchivedEvent`] from a worktree's last-known state.
fn event_from(kind: EventKind, wt: &WorktreeRecord) -> ArchivedEvent {
    let dirty = wt.status.as_ref().map(|s| DirtySummary {
        staged: s.staged,
        unstaged: s.unstaged,
        untracked: s.untracked,
        conflicted: s.conflicted,
    });
    ArchivedEvent::new(
        kind,
        wt.path.clone(),
        wt.branch_short().map(str::to_string),
        wt.short_head().map(str::to_string),
        dirty,
    )
}

/// Classify how a worktree changed between snapshots into a *milestone* flash:
/// a push (ahead drained to zero against a live upstream) takes precedence over
/// a commit (HEAD moved). Returns `None` for lesser changes — live save activity
/// is flashed separately by the filesystem watcher, and "has uncommitted work"
/// is a persistent highlight, not a flash. Shared with the home view's diffing.
pub(crate) fn change_kind(old: &WorktreeRecord, new: &WorktreeRecord) -> Option<TransitionKind> {
    let old_ahead = old.status.as_ref().and_then(|s| s.ahead).unwrap_or(0);
    let new_ahead = new.status.as_ref().and_then(|s| s.ahead).unwrap_or(0);
    let new_behind = new.status.as_ref().and_then(|s| s.behind).unwrap_or(0);
    let has_live_upstream = new
        .status
        .as_ref()
        .is_some_and(|s| s.upstream.is_some() && !s.upstream_gone);
    // Heuristic (handoff §13.7): ahead dropping to 0 with a live upstream looks
    // like a push. Requiring behind == 0 rules out a fetch that fast-forwarded
    // the remote past us. A hard reset to the exact upstream tip is
    // indistinguishable and also flashes Pushed — an accepted v1 limitation
    // (true push detection needs upstream-OID tracking).
    if old_ahead > 0 && new_ahead == 0 && new_behind == 0 && has_live_upstream {
        return Some(TransitionKind::Pushed);
    }
    // A moved HEAD is a commit. Heuristic: a reset / rebase / checkout / merge
    // also moves HEAD and will flash Committed — accepted for v1 (true commit
    // detection needs reflog tracking). Only when both HEADs are known, so we
    // never flash on missing data (a bare worktree has no HEAD oid).
    if let (Some(old_head), Some(new_head)) = (&old.head, &new.head)
        && old_head != new_head
    {
        return Some(TransitionKind::Committed);
    }
    None
}

/// Flash a blue `Activity` marker on the worktree containing each changed path.
/// Driven by the (un-debounced) filesystem watcher so the marker tracks live
/// save state. Paths that fall outside every known worktree are ignored. Shared
/// by the per-repo and home reducers.
pub(crate) fn note_activity_for(
    transitions: &mut Transitions,
    worktrees: &[WorktreeRecord],
    paths: &[std::path::PathBuf],
) {
    use crate::util::paths::{longest_prefix_match, normalize};
    let roots: Vec<String> = worktrees.iter().map(|w| normalize(&w.path)).collect();
    for path in paths {
        let probe = normalize(&path.to_string_lossy());
        if let Some(idx) = longest_prefix_match(&probe, &roots) {
            transitions.note(worktrees[idx].path.clone(), TransitionKind::Activity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{RepoIdentity, WorktreeRecord};
    use crate::process::ProcessSnapshot;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn snapshot_with(n: usize) -> RepoSnapshot {
        let worktrees = (0..n)
            .map(|i| WorktreeRecord {
                path: format!("/repo-{i}"),
                ..Default::default()
            })
            .collect();
        RepoSnapshot {
            repo: RepoIdentity {
                start_dir: "/repo".into(),
                root: "/repo".into(),
                git_dir: "/repo/.git".into(),
                common_git_dir: "/repo/.git".into(),
                is_worktree: false,
            },
            worktrees,
            branches: Vec::new(),
            base: None,
            collisions: Vec::new(),
            processes: ProcessSnapshot::default(),
        }
    }

    fn app(n: usize) -> AppState {
        AppState::new(snapshot_with(n))
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn q_quits() {
        let mut a = app(0);
        let action = a.resolve_key(key(KeyCode::Char('q')));
        assert_eq!(action, Action::Quit);
        a.apply(action);
        assert!(a.should_quit);
    }

    #[test]
    fn tab_cycles_forward_and_wraps() {
        let mut a = app(0);
        assert_eq!(a.active_tab, Tab::Overview);
        for _ in 0..Tab::ALL.len() {
            let action = a.resolve_key(key(KeyCode::Tab));
            a.apply(action);
        }
        assert_eq!(a.active_tab, Tab::Overview); // wrapped fully around
    }

    #[test]
    fn back_tab_goes_to_last() {
        let mut a = app(0);
        let action = a.resolve_key(key(KeyCode::BackTab));
        a.apply(action);
        assert_eq!(a.active_tab, Tab::Help);
    }

    #[test]
    fn number_selects_tab() {
        let mut a = app(0);
        let action = a.resolve_key(key(KeyCode::Char('3')));
        a.apply(action);
        assert_eq!(a.active_tab, Tab::Processes);
    }

    #[test]
    fn help_toggles() {
        let mut a = app(0);
        a.apply(a.resolve_key(key(KeyCode::Char('?'))));
        assert!(a.show_help);
        a.apply(a.resolve_key(key(KeyCode::Esc)));
        assert!(!a.show_help);
        assert!(!a.should_quit);
    }

    #[test]
    fn worktree_navigation_clamps() {
        let mut a = app(3);
        assert_eq!(a.selected, 0);
        a.apply(Action::MoveUp); // already at top
        assert_eq!(a.selected, 0);
        a.apply(Action::MoveDown);
        a.apply(Action::MoveDown);
        assert_eq!(a.selected, 2);
        a.apply(Action::MoveDown); // clamp at bottom
        assert_eq!(a.selected, 2);
    }

    #[test]
    fn navigation_noop_when_empty() {
        let mut a = app(0);
        a.apply(Action::MoveDown);
        assert_eq!(a.selected, 0);
    }

    fn snapshot_paths(paths: &[&str]) -> RepoSnapshot {
        let worktrees = paths
            .iter()
            .map(|p| WorktreeRecord {
                path: (*p).to_string(),
                ..Default::default()
            })
            .collect();
        RepoSnapshot {
            repo: RepoIdentity {
                start_dir: "/repo".into(),
                root: "/repo".into(),
                git_dir: "/repo/.git".into(),
                common_git_dir: "/repo/.git".into(),
                is_worktree: false,
            },
            worktrees,
            branches: Vec::new(),
            base: None,
            collisions: Vec::new(),
            processes: ProcessSnapshot::default(),
        }
    }

    #[test]
    fn ingest_detects_created_and_removed() {
        let mut app = AppState::new(snapshot_paths(&["/repo-a", "/repo-b"]));
        let events = app.ingest_snapshot(snapshot_paths(&["/repo-a", "/repo-c"]));
        assert_eq!(events.len(), 2);
        let kinds: Vec<(EventKind, &str)> =
            events.iter().map(|e| (e.kind, e.path.as_str())).collect();
        assert!(kinds.contains(&(EventKind::WorktreeCreated, "/repo-c")));
        assert!(kinds.contains(&(EventKind::WorktreeRemoved, "/repo-b")));
        assert_eq!(app.archive.len(), 2);
    }

    fn wt_with(ahead: u32, behind: u32, upstream: bool, gone: bool) -> WorktreeRecord {
        WorktreeRecord {
            path: "/r".into(),
            status: Some(crate::git::WorktreeStatus {
                ahead: Some(ahead),
                behind: Some(behind),
                upstream: upstream.then(|| "origin/main".to_string()),
                upstream_gone: gone,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn push_flashes_when_ahead_clears_evenly() {
        let old = wt_with(2, 0, true, false);
        let new = wt_with(0, 0, true, false);
        assert_eq!(change_kind(&old, &new), Some(TransitionKind::Pushed));
    }

    #[test]
    fn fetch_fast_forward_is_not_a_push_or_commit() {
        let old = wt_with(2, 0, true, false);
        let new = wt_with(0, 3, true, false); // remote moved past us
        // Neither a push (behind != 0) nor a commit (HEAD unchanged): remote
        // drift is a persistent badge, not a flash.
        assert_eq!(change_kind(&old, &new), None);
    }

    #[test]
    fn no_push_without_live_upstream() {
        let old = wt_with(2, 0, true, true); // upstream gone
        let new = wt_with(0, 0, true, true);
        assert_ne!(change_kind(&old, &new), Some(TransitionKind::Pushed));
    }

    fn wt_head(head: &str) -> WorktreeRecord {
        WorktreeRecord {
            path: "/r".into(),
            head: Some(head.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn moved_head_flashes_committed() {
        assert_eq!(
            change_kind(&wt_head("aaaaaaaa"), &wt_head("bbbbbbbb")),
            Some(TransitionKind::Committed)
        );
    }

    #[test]
    fn unchanged_head_is_no_flash() {
        assert_eq!(
            change_kind(&wt_head("aaaaaaaa"), &wt_head("aaaaaaaa")),
            None
        );
    }

    #[test]
    fn push_takes_precedence_over_a_moved_head() {
        // A commit that is simultaneously pushed reads as the louder Pushed.
        let mut old = wt_with(2, 0, true, false);
        old.head = Some("aaaaaaaa".into());
        let mut new = wt_with(0, 0, true, false);
        new.head = Some("bbbbbbbb".into());
        assert_eq!(change_kind(&old, &new), Some(TransitionKind::Pushed));
    }

    #[test]
    fn note_activity_flashes_the_containing_worktree() {
        let mut app = AppState::new(snapshot_paths(&["/repo/main", "/repo/feat"]));
        app.note_activity(&[std::path::PathBuf::from("/repo/feat/src/lib.rs")]);
        assert_eq!(
            app.transition_for("/repo/feat"),
            Some(TransitionKind::Activity)
        );
        assert_eq!(app.transition_for("/repo/main"), None);
    }

    #[test]
    fn note_activity_ignores_paths_outside_any_worktree() {
        let mut app = AppState::new(snapshot_paths(&["/repo/main"]));
        app.note_activity(&[std::path::PathBuf::from("/elsewhere/x.rs")]);
        assert_eq!(app.transition_for("/repo/main"), None);
    }
}
