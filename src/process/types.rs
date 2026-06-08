//! Process data models.

use std::path::PathBuf;

/// What kind of process this is, for at-a-glance triage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProcessLabel {
    /// A coding agent (Claude, Codex, …) — the headline case.
    Agent,
    /// A build/test/package task (cargo, npm, pnpm, …).
    Task,
    /// A language runtime (node, python, …).
    Runtime,
    /// An interactive shell.
    Shell,
    /// An editor.
    Editor,
    #[default]
    Unknown,
}

impl ProcessLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            ProcessLabel::Agent => "agent",
            ProcessLabel::Task => "task",
            ProcessLabel::Runtime => "runtime",
            ProcessLabel::Shell => "shell",
            ProcessLabel::Editor => "editor",
            ProcessLabel::Unknown => "unknown",
        }
    }

    pub fn is_agent(self) -> bool {
        matches!(self, ProcessLabel::Agent)
    }
}

/// One scanned process, with its worktree mapping resolved.
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent: Option<u32>,
    /// Display name (executable name, e.g. `claude` / `node`).
    pub name: String,
    /// Joined command line, for display.
    pub cmd: String,
    pub cwd: Option<PathBuf>,
    pub label: ProcessLabel,
    pub cpu: f32,
    pub memory_bytes: u64,
    pub run_secs: u64,
    /// Index into the snapshot's worktree list, if the CWD mapped to one.
    pub matched_worktree: Option<usize>,
}

/// All processes mapped into the repository's worktrees.
#[derive(Debug, Clone, Default)]
pub struct ProcessSnapshot {
    pub processes: Vec<ProcessInfo>,
}

impl ProcessSnapshot {
    /// Processes mapped to worktree `idx`.
    pub fn for_worktree(&self, idx: usize) -> impl Iterator<Item = &ProcessInfo> {
        self.processes
            .iter()
            .filter(move |p| p.matched_worktree == Some(idx))
    }

    /// The first agent process running in worktree `idx`, if any.
    pub fn agent_for_worktree(&self, idx: usize) -> Option<&ProcessInfo> {
        self.for_worktree(idx).find(|p| p.label.is_agent())
    }

    /// Whether worktree `idx` has an agent process (i.e. is actively worked on).
    pub fn worktree_is_active(&self, idx: usize) -> bool {
        self.agent_for_worktree(idx).is_some()
    }
}
