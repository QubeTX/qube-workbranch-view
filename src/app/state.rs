//! The application state and its reducer.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};

use super::action::Action;
use super::overlay::{
    Command, Confirm, ConfirmAction, Overlay, Palette, PendingGit, PendingKill, palette_filtered,
};
use super::transitions::{TransitionKind, Transitions};
use crate::git::{RepoSnapshot, WorktreeRecord};
use crate::storage::{ArchivedEvent, DirtySummary, EventKind};

/// In-memory cap on archived events (matches the on-disk prune cap).
const MAX_ARCHIVE: usize = 2000;

/// How long a transient operator status message (e.g. a kill result) stays up.
const STATUS_TTL: Duration = Duration::from_millis(5000);

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
            Tab::Collisions => "Merge Risk",
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
    /// A pending process kill for the event loop to run, if any.
    pending_kill: Option<PendingKill>,
    /// Selected row in the Processes tab.
    pub proc_selected: usize,
    /// True when the most recent snapshot capture failed — the board is showing
    /// stale data. Surfaced in the header so "live" never silently lies.
    pub stale: bool,
    /// Epoch seconds of the last successful snapshot ingest — drives the
    /// Overview "updated Ns ago" freshness line.
    pub last_updated: Option<u64>,
    /// A transient operator-facing result message (e.g. a kill outcome), shown
    /// briefly in the footer: `(text, raised_at, is_error)`.
    status: Option<(String, Instant, bool)>,
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
            pending_kill: None,
            proc_selected: 0,
            stale: false,
            last_updated: None,
            status: None,
        }
    }

    /// Replace the snapshot (after a manual/live refresh), clamping selection.
    pub fn set_snapshot(&mut self, snapshot: RepoSnapshot) {
        self.snapshot = snapshot;
        self.clamp_selection();
        self.clamp_proc_selection();
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

    /// Whether worktree `idx` is protected from removal: the primary checkout
    /// (index 0), the current worktree, a bare repo, or any `main`/`master`
    /// branch worktree (the integration target must never be removable).
    fn is_protected(&self, idx: usize) -> bool {
        idx == 0
            || self.snapshot.current_worktree_index() == Some(idx)
            || self.snapshot.worktrees.get(idx).is_some_and(|w| {
                w.bare || matches!(w.branch_short(), Some("main") | Some("master"))
            })
    }

    /// Consume a pending Git mutation for the event loop to execute.
    pub fn take_pending_git(&mut self) -> Option<PendingGit> {
        self.pending_git.take()
    }

    /// Consume a pending process kill for the event loop to execute.
    pub fn take_pending_kill(&mut self) -> Option<PendingKill> {
        self.pending_kill.take()
    }

    /// Clamp the Processes-tab selection to the mapped-process count.
    pub fn clamp_proc_selection(&mut self) {
        let len = self.snapshot.processes.processes.len();
        if len == 0 {
            self.proc_selected = 0;
        } else if self.proc_selected >= len {
            self.proc_selected = len - 1;
        }
    }

    fn move_proc_selection(&mut self, delta: i32) {
        let len = self.snapshot.processes.processes.len();
        if len > 0 {
            self.proc_selected =
                (self.proc_selected as i32 + delta).clamp(0, len as i32 - 1) as usize;
        }
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
        let mut notes: Vec<(String, Vec<TransitionKind>)> = Vec::new();
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
                        notes.push((wt.path.clone(), vec![TransitionKind::Created]));
                        new_events.push(event_from(EventKind::WorktreeCreated, wt));
                    }
                    Some(old) => {
                        let kinds = change_kind(old, wt);
                        if !kinds.is_empty() {
                            notes.push((wt.path.clone(), kinds));
                        }
                    }
                }
            }
            let incoming: HashSet<&str> = new.worktrees.iter().map(|wt| wt.path.as_str()).collect();
            for old in &self.snapshot.worktrees {
                if !old.bare && !incoming.contains(old.path.as_str()) {
                    notes.push((old.path.clone(), vec![TransitionKind::Deleted]));
                    new_events.push(event_from(EventKind::WorktreeRemoved, old));
                }
            }
            // New merge-conflict-risk pairings (a file changed on a worktree SET
            // not flagged last snapshot) → log once. Keyed by file + the worktree
            // paths involved, so a re-paired conflict (A×B → A×C) re-logs.
            let old_conflicts: HashSet<String> = self
                .snapshot
                .collisions
                .iter()
                .map(|c| conflict_key(c, &self.snapshot.worktrees))
                .collect();
            for c in &new.collisions {
                if !old_conflicts.contains(&conflict_key(c, &new.worktrees)) {
                    let who: Vec<String> = c
                        .worktrees
                        .iter()
                        .filter_map(|&i| new.worktrees.get(i).map(WorktreeRecord::display_name))
                        .collect();
                    new_events.push(ArchivedEvent::new(
                        EventKind::ConflictRisk,
                        c.file.clone(),
                        Some(who.join(" × ")),
                        None,
                        None,
                    ));
                }
            }
            // Branch milestones (commit / push / merge), diffed on the
            // hierarchy. These feed the Timeline and OS notifications.
            new_events.extend(branch_events(&self.snapshot, &new, None));
        }

        for (path, seq) in notes {
            self.transitions.note_seq(path, seq);
        }
        // Prepend so the archive stays newest-first; reverse keeps in-batch order.
        for event in new_events.iter().rev() {
            self.archive.push_front(event.clone());
        }
        self.truncate_archive();
        self.set_snapshot(new);
        self.last_updated = Some(crate::storage::events::epoch_secs());
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

    /// Drop expired transient highlights and status message (animation tick).
    pub fn expire_transitions(&mut self) {
        self.transitions.expire();
        if self
            .status
            .as_ref()
            .is_some_and(|(_, at, _)| at.elapsed() >= STATUS_TTL)
        {
            self.status = None;
        }
    }

    /// Raise a transient operator-facing status message (e.g. a kill result).
    pub fn set_status(&mut self, text: String, is_error: bool) {
        self.status = Some((text, Instant::now(), is_error));
    }

    /// The active status message `(text, is_error)`, if not yet expired.
    pub fn status(&self) -> Option<(&str, bool)> {
        self.status
            .as_ref()
            .filter(|(_, at, _)| at.elapsed() < STATUS_TTL)
            .map(|(text, _, err)| (text.as_str(), *err))
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
            KeyCode::Char('K') => match self.active_tab {
                Tab::Processes => Action::RequestKillProcess,
                _ => Action::RequestKillAgent,
            },
            KeyCode::Char('j') | KeyCode::Down => match self.active_tab {
                Tab::Processes => Action::ProcessDown,
                _ => Action::MoveDown,
            },
            KeyCode::Char('k') | KeyCode::Up => match self.active_tab {
                Tab::Processes => Action::ProcessUp,
                _ => Action::MoveUp,
            },
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
            Action::RequestKillAgent => self.open_kill_agent_confirm(),
            Action::RequestKillProcess => self.open_kill_process_confirm(),
            Action::ProcessDown => self.move_proc_selection(1),
            Action::ProcessUp => self.move_proc_selection(-1),
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
                    match cf.action {
                        ConfirmAction::Git(g) => self.pending_git = Some(g),
                        ConfirmAction::Kill(k) => self.pending_kill = Some(k),
                    }
                } else {
                    self.overlay = Overlay::Confirm(cf); // wrong token — keep open
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
            "UNCOMMITTED — a rescue snapshot will be saved first.".to_string()
        });
        detail.push(format!("Type \"{expected}\" to confirm:"));
        self.overlay = Overlay::Confirm(Confirm {
            title: "Remove worktree?".to_string(),
            detail,
            expected,
            typed: String::new(),
            action: ConfirmAction::Git(PendingGit::RemoveWorktree {
                path,
                force: !clean,
                snapshot_first: !clean,
                label,
            }),
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
            action: ConfirmAction::Git(PendingGit::Prune),
        });
    }

    /// Open a type-the-PID-to-confirm dialog to kill the agent attached to the
    /// selected worktree (for a forgotten / stuck background agent).
    fn open_kill_agent_confirm(&mut self) {
        let Some(idx) = self.selected_original_index() else {
            return;
        };
        let Some(agent) = self.snapshot.processes.agent_for_worktree(idx) else {
            return;
        };
        let pid = agent.pid;
        let name = agent.name.clone();
        let cwd = agent.cwd.as_ref().map(|p| p.display().to_string());
        let wt = self
            .snapshot
            .worktrees
            .get(idx)
            .map_or_else(String::new, WorktreeRecord::display_name);
        self.overlay = Overlay::Confirm(kill_confirm(pid, &name, cwd.as_deref(), &wt));
    }

    /// Open a type-the-PID-to-confirm dialog to kill the selected Processes-tab
    /// process.
    fn open_kill_process_confirm(&mut self) {
        let Some(proc) = self.snapshot.processes.processes.get(self.proc_selected) else {
            return;
        };
        let pid = proc.pid;
        let name = proc.name.clone();
        let cwd = proc.cwd.as_ref().map(|p| p.display().to_string());
        let wt = proc
            .matched_worktree
            .and_then(|i| self.snapshot.worktrees.get(i))
            .map_or_else(String::new, WorktreeRecord::display_name);
        self.overlay = Overlay::Confirm(kill_confirm(pid, &name, cwd.as_deref(), &wt));
    }
}

/// Build a type-the-PID-to-confirm dialog for terminating a process.
fn kill_confirm(pid: u32, name: &str, cwd: Option<&str>, worktree: &str) -> Confirm {
    let short = name.trim_end_matches(".exe");
    let mut detail = vec![format!("process: {short}  pid {pid}")];
    if !worktree.is_empty() {
        detail.push(format!("worktree: {worktree}"));
    }
    if let Some(cwd) = cwd {
        detail.push(format!("cwd: {cwd}"));
    }
    detail.push(String::new());
    detail.push("Terminates the process — for forgotten or stuck agents.".to_string());
    detail.push(format!("Type {pid} to confirm:"));
    Confirm {
        title: "Kill process?".to_string(),
        detail,
        expected: pid.to_string(),
        typed: String::new(),
        action: ConfirmAction::Kill(PendingKill {
            pid,
            name: name.to_string(),
        }),
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

/// A stable identity for a collision: its file plus the sorted worktree PATHS
/// involved (paths, not indices — indices shift as worktrees come and go). Lets
/// the Timeline treat a re-paired conflict (A×B → A×C) as a new event.
fn conflict_key(c: &crate::collision::Collision, worktrees: &[WorktreeRecord]) -> String {
    let mut paths: Vec<&str> = c
        .worktrees
        .iter()
        .filter_map(|&i| worktrees.get(i))
        .map(|w| w.path.as_str())
        .collect();
    paths.sort_unstable();
    format!("{}\u{1f}{}", c.file, paths.join("\u{1f}"))
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
pub(crate) fn change_kind(old: &WorktreeRecord, new: &WorktreeRecord) -> Vec<TransitionKind> {
    let old_ahead = old.status.as_ref().and_then(|s| s.ahead).unwrap_or(0);
    let new_ahead = new.status.as_ref().and_then(|s| s.ahead).unwrap_or(0);
    let new_behind = new.status.as_ref().and_then(|s| s.behind).unwrap_or(0);
    let has_live_upstream = new
        .status
        .as_ref()
        .is_some_and(|s| s.upstream.is_some() && !s.upstream_gone);
    // HEAD moved between snapshots → a commit landed. (A reset / rebase /
    // checkout / merge also moves HEAD and will read as Committed — accepted v1
    // heuristic.) Only when both HEADs are known, so we never flash on missing
    // data (a bare worktree has no HEAD oid).
    let head_moved = matches!((&old.head, &new.head), (Some(o), Some(n)) if o != n);
    // In sync with a live upstream now (ahead/behind both zero).
    let now_synced = new_ahead == 0 && new_behind == 0 && has_live_upstream;

    // A commit that also leaves us synced with a live upstream is a back-to-back
    // commit+push: play magenta then green (handoff §13.7). A hard reset to the
    // upstream tip is indistinguishable and also plays this — accepted limit.
    if head_moved && now_synced {
        return vec![TransitionKind::Committed, TransitionKind::Pushed];
    }
    // Ahead drained to 0 against a live upstream with HEAD unchanged → a pure
    // push. behind == 0 rules out a fetch that fast-forwarded the remote past us.
    if old_ahead > 0 && now_synced {
        return vec![TransitionKind::Pushed];
    }
    // HEAD moved but not yet synced → a commit not yet pushed.
    if head_moved {
        return vec![TransitionKind::Committed];
    }
    Vec::new()
}

/// Max touched paths carried with a commit-milestone event.
const EVENT_FILES_CAP: usize = 10;

/// Diff two snapshots' branch hierarchies into milestone events: a commit
/// (tip moved — rebases suppressed), a push (lifecycle reached `Pushed`), a
/// merge (lifecycle reached `Merged`, or the branch vanished after merging).
/// `repo` stamps the events in machine-wide mode.
pub(crate) fn branch_events(
    old: &RepoSnapshot,
    new: &RepoSnapshot,
    repo: Option<&str>,
) -> Vec<ArchivedEvent> {
    use crate::git::BranchLifecycle;
    let old_by_name: HashMap<&str, &crate::git::BranchNode> = old
        .hierarchy
        .nodes
        .iter()
        .map(|n| (n.name.as_str(), n))
        .collect();
    let mut out = Vec::new();

    for n in &new.hierarchy.nodes {
        // Brand-new branches don't fire milestones; their appearance is
        // already covered by the worktree-created event/flash.
        let Some(o) = old_by_name.get(n.name.as_str()) else {
            continue;
        };
        let path = n
            .worktree
            .and_then(|i| new.worktrees.get(i))
            .map(|w| w.path.clone())
            .unwrap_or_default();
        let mk = |kind: EventKind| {
            let mut ev = ArchivedEvent::new(
                kind,
                path.clone(),
                Some(n.name.clone()),
                Some(n.oid.chars().take(8).collect()),
                None,
            );
            ev.parent = n.parent.clone();
            ev.repo = repo.map(str::to_string);
            ev
        };

        // A rebase moves the tip without new work: ahead-of-parent didn't
        // grow and behind-parent drained to zero (the parent moved under us).
        // Heuristic in the same spirit as `change_kind`'s documented limits.
        let rebase_signature = n.ahead_of_parent <= o.ahead_of_parent
            && o.behind_parent.is_some_and(|b| b > 0)
            && n.behind_parent == Some(0);
        if o.oid != n.oid && !rebase_signature {
            let mut ev = mk(EventKind::BranchCommitted);
            ev.files = n.worktree.and_then(|i| new.worktrees.get(i)).map(|w| {
                w.touched
                    .iter()
                    .take(EVENT_FILES_CAP)
                    .map(|f| f.path.clone())
                    .collect()
            });
            out.push(ev);
        }
        if o.lifecycle != BranchLifecycle::Pushed && n.lifecycle == BranchLifecycle::Pushed {
            out.push(mk(EventKind::BranchPushed));
        }
        if o.lifecycle != BranchLifecycle::Merged && n.lifecycle == BranchLifecycle::Merged {
            out.push(mk(EventKind::BranchMerged));
        }
    }

    // A branch that vanished while merged/pushed completed its lifecycle —
    // fold the deletion into a merge milestone.
    let new_names: HashSet<&str> = new
        .hierarchy
        .nodes
        .iter()
        .map(|n| n.name.as_str())
        .collect();
    for o in &old.hierarchy.nodes {
        if !new_names.contains(o.name.as_str())
            && matches!(
                o.lifecycle,
                crate::git::BranchLifecycle::Merged | crate::git::BranchLifecycle::Pushed
            )
        {
            let mut ev = ArchivedEvent::new(
                EventKind::BranchMerged,
                String::new(),
                Some(o.name.clone()),
                Some(o.oid.chars().take(8).collect()),
                None,
            );
            ev.parent = o.parent.clone();
            ev.repo = repo.map(str::to_string);
            out.push(ev);
        }
    }
    out
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
    use crate::process::{ProcessInfo, ProcessLabel, ProcessSnapshot};
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
            hierarchy: Default::default(),
            collisions: Vec::new(),
            processes: ProcessSnapshot::default(),
            captured_at: 0,
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
            hierarchy: Default::default(),
            collisions: Vec::new(),
            processes: ProcessSnapshot::default(),
            captured_at: 0,
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

    fn snapshot_with_conflict(paths: &[&str]) -> RepoSnapshot {
        use crate::collision::{Collision, Severity};
        let mut s = snapshot_paths(paths);
        s.collisions = vec![Collision {
            file: "src/x.rs".into(),
            worktrees: vec![0, 1],
            severity: Severity::Medium,
        }];
        s
    }

    #[test]
    fn ingest_archives_a_new_conflict_risk_event() {
        let mut app = AppState::new(snapshot_paths(&["/repo-a", "/repo-b"]));
        let events = app.ingest_snapshot(snapshot_with_conflict(&["/repo-a", "/repo-b"]));
        assert!(
            events
                .iter()
                .any(|e| e.kind == EventKind::ConflictRisk && e.path == "src/x.rs")
        );
    }

    #[test]
    fn an_existing_conflict_is_not_re_archived() {
        let mut app = AppState::new(snapshot_paths(&["/repo-a", "/repo-b"]));
        app.ingest_snapshot(snapshot_with_conflict(&["/repo-a", "/repo-b"])); // first → logged
        let events = app.ingest_snapshot(snapshot_with_conflict(&["/repo-a", "/repo-b"]));
        assert!(!events.iter().any(|e| e.kind == EventKind::ConflictRisk)); // still present → quiet
    }

    #[test]
    fn a_re_paired_conflict_is_logged_again() {
        use crate::collision::{Collision, Severity};
        let with = |worktrees: Vec<usize>| -> RepoSnapshot {
            let mut s = snapshot_paths(&["/repo-a", "/repo-b", "/repo-c"]);
            s.collisions = vec![Collision {
                file: "src/x.rs".into(),
                worktrees,
                severity: Severity::Medium,
            }];
            s
        };
        let mut app = AppState::new(snapshot_paths(&["/repo-a", "/repo-b", "/repo-c"]));
        app.ingest_snapshot(with(vec![0, 1])); // A×B logged
        let events = app.ingest_snapshot(with(vec![0, 2])); // A×C — new pairing, re-logged
        assert!(events.iter().any(|e| e.kind == EventKind::ConflictRisk));
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
        assert_eq!(change_kind(&old, &new), vec![TransitionKind::Pushed]);
    }

    #[test]
    fn fetch_fast_forward_is_not_a_push_or_commit() {
        let old = wt_with(2, 0, true, false);
        let new = wt_with(0, 3, true, false); // remote moved past us
        // Neither a push (behind != 0) nor a commit (HEAD unchanged): remote
        // drift is a persistent badge, not a flash.
        assert!(change_kind(&old, &new).is_empty());
    }

    #[test]
    fn no_push_without_live_upstream() {
        let old = wt_with(2, 0, true, true); // upstream gone
        let new = wt_with(0, 0, true, true);
        assert!(change_kind(&old, &new).is_empty());
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
            vec![TransitionKind::Committed]
        );
    }

    #[test]
    fn unchanged_head_is_no_flash() {
        assert!(change_kind(&wt_head("aaaaaaaa"), &wt_head("aaaaaaaa")).is_empty());
    }

    #[test]
    fn commit_then_push_plays_committed_then_pushed() {
        // A commit that also leaves us synced with a live upstream → the
        // back-to-back sequence: magenta (committed) then green (pushed).
        let mut old = wt_with(2, 0, true, false);
        old.head = Some("aaaaaaaa".into());
        let mut new = wt_with(0, 0, true, false);
        new.head = Some("bbbbbbbb".into());
        assert_eq!(
            change_kind(&old, &new),
            vec![TransitionKind::Committed, TransitionKind::Pushed]
        );
    }

    fn bnode(
        name: &str,
        oid: &str,
        lifecycle: crate::git::BranchLifecycle,
    ) -> crate::git::BranchNode {
        crate::git::BranchNode {
            name: name.to_string(),
            oid: oid.to_string(),
            role: crate::git::BranchRole::Task,
            parent: Some("emmett/wb-2026-06-10".to_string()),
            ahead_of_parent: 1,
            behind_parent: Some(0),
            merged_into_parent: false,
            upstream: None,
            ahead: None,
            behind: None,
            upstream_gone: false,
            committer_date: None,
            lifecycle,
            worktree: None,
        }
    }

    fn snap_with_nodes(nodes: Vec<crate::git::BranchNode>) -> RepoSnapshot {
        let mut s = snapshot_paths(&[]);
        s.hierarchy = crate::git::BranchHierarchy {
            trunk: Some("main".to_string()),
            nodes,
            approximate: false,
        };
        s
    }

    #[test]
    fn branch_commit_milestone_on_tip_move() {
        use crate::git::BranchLifecycle::Committed;
        let old = snap_with_nodes(vec![bnode("feat/x", "aaaa", Committed)]);
        let new = snap_with_nodes(vec![bnode("feat/x", "bbbb", Committed)]);
        let events = branch_events(&old, &new, None);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::BranchCommitted);
        assert_eq!(events[0].branch.as_deref(), Some("feat/x"));
        assert_eq!(events[0].parent.as_deref(), Some("emmett/wb-2026-06-10"));
    }

    #[test]
    fn rebase_does_not_fire_a_commit_milestone() {
        use crate::git::BranchLifecycle::Committed;
        // Tip moved, but ahead didn't grow and behind-parent drained to 0:
        // the parent moved under us (a rebase), not new work.
        let mut before = bnode("feat/x", "aaaa", Committed);
        before.behind_parent = Some(2);
        let after = bnode("feat/x", "bbbb", Committed);
        let events = branch_events(
            &snap_with_nodes(vec![before]),
            &snap_with_nodes(vec![after]),
            None,
        );
        assert!(events.is_empty(), "a rebase is not a commit milestone");
    }

    #[test]
    fn push_and_merge_milestones_on_lifecycle_transitions() {
        use crate::git::BranchLifecycle::{Committed, Merged, Pushed};
        let old = snap_with_nodes(vec![
            bnode("feat/p", "aaaa", Committed),
            bnode("feat/m", "cccc", Pushed),
        ]);
        let new = snap_with_nodes(vec![
            bnode("feat/p", "aaaa", Pushed),
            bnode("feat/m", "cccc", Merged),
        ]);
        let events = branch_events(&old, &new, None);
        let kinds: Vec<(EventKind, &str)> = events
            .iter()
            .map(|e| (e.kind, e.branch.as_deref().unwrap_or("")))
            .collect();
        assert!(kinds.contains(&(EventKind::BranchPushed, "feat/p")));
        assert!(kinds.contains(&(EventKind::BranchMerged, "feat/m")));
        assert_eq!(events.len(), 2, "steady branches stay quiet");
    }

    #[test]
    fn vanished_merged_branch_folds_into_a_merge_milestone() {
        use crate::git::BranchLifecycle::{Committed, Merged};
        let old = snap_with_nodes(vec![
            bnode("feat/done", "aaaa", Merged),
            bnode("feat/wip", "bbbb", Committed),
        ]);
        // feat/done deleted after merge; feat/wip deleted prematurely.
        let new = snap_with_nodes(vec![]);
        let events = branch_events(&old, &new, Some("myrepo"));
        assert_eq!(
            events.len(),
            1,
            "only the merged branch folds into an event"
        );
        assert_eq!(events[0].kind, EventKind::BranchMerged);
        assert_eq!(events[0].branch.as_deref(), Some("feat/done"));
        assert_eq!(events[0].repo.as_deref(), Some("myrepo"));
    }

    #[test]
    fn brand_new_branch_fires_no_milestone() {
        use crate::git::BranchLifecycle::Committed;
        let old = snap_with_nodes(vec![]);
        let new = snap_with_nodes(vec![bnode("feat/new", "aaaa", Committed)]);
        assert!(branch_events(&old, &new, None).is_empty());
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

    fn agent_proc(pid: u32, wt: usize) -> ProcessInfo {
        ProcessInfo {
            pid,
            parent: None,
            name: "claude.exe".into(),
            cmd: "claude.exe".into(),
            cwd: Some("/repo-0".into()),
            label: ProcessLabel::Agent,
            cpu: 0.0,
            memory_bytes: 0,
            run_secs: 0,
            matched_worktree: Some(wt),
        }
    }

    fn app_with_agent(pid: u32) -> AppState {
        let mut snap = snapshot_with(1);
        snap.processes = ProcessSnapshot {
            processes: vec![agent_proc(pid, 0)],
        };
        let mut app = AppState::new(snap);
        app.active_tab = Tab::Worktrees;
        app
    }

    #[test]
    fn main_and_master_worktrees_are_protected_from_removal() {
        let mut snap = snapshot_paths(&["/repo/wt-a", "/repo/feat", "/repo/wt-c"]);
        snap.worktrees[0].branch = Some("refs/heads/main".into());
        snap.worktrees[1].branch = Some("refs/heads/feat".into());
        snap.worktrees[2].branch = Some("refs/heads/master".into());
        let app = AppState::new(snap);
        assert!(app.is_protected(0)); // primary checkout (also main)
        assert!(!app.is_protected(1)); // a normal task worktree — removable
        assert!(app.is_protected(2)); // master branch — never removable
    }

    #[test]
    fn k_on_worktrees_requests_killing_the_agent() {
        let app = app_with_agent(4242);
        assert_eq!(
            app.resolve_key(key(KeyCode::Char('K'))),
            Action::RequestKillAgent
        );
    }

    #[test]
    fn kill_confirm_with_the_right_pid_sets_pending_kill() {
        let mut app = app_with_agent(4242);
        app.apply(Action::RequestKillAgent);
        assert!(matches!(app.overlay, Overlay::Confirm(_)));
        for c in "4242".chars() {
            app.apply(Action::InputChar(c));
        }
        app.apply(Action::InputSubmit);
        assert_eq!(app.take_pending_kill().map(|k| k.pid), Some(4242));
    }

    #[test]
    fn kill_confirm_rejects_a_wrong_pid() {
        let mut app = app_with_agent(4242);
        app.apply(Action::RequestKillAgent);
        for c in "9999".chars() {
            app.apply(Action::InputChar(c));
        }
        app.apply(Action::InputSubmit);
        assert!(app.take_pending_kill().is_none());
        assert!(matches!(app.overlay, Overlay::Confirm(_))); // stays open
    }
}
