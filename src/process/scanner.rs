//! Scan OS processes via sysinfo and map them to worktrees. Best-effort: a CWD
//! may be unavailable, and processes can exit between scan and display.

use std::path::PathBuf;

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use super::classifier::classify;
use super::types::{ProcessInfo, ProcessSnapshot};
use crate::util::paths::{longest_prefix_match, normalize};

/// Scan all processes and keep those whose CWD maps into one of
/// `worktree_roots` (passed in worktree-index order). Synchronous and possibly
/// slow — call via `tokio::task::spawn_blocking` from async code.
pub fn scan(worktree_roots: &[String]) -> ProcessSnapshot {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );

    let norm_roots: Vec<String> = worktree_roots.iter().map(|r| normalize(r)).collect();
    let mut processes = Vec::new();

    for proc in sys.processes().values() {
        let cwd = proc.cwd().map(|p| p.to_path_buf());
        let Some(matched) = cwd
            .as_ref()
            .and_then(|c| longest_prefix_match(&normalize(&c.to_string_lossy()), &norm_roots))
        else {
            continue; // only processes living inside one of this repo's worktrees
        };

        let name = proc.name().to_string_lossy().to_string();
        let cmd = proc
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        let label = classify(&name, &cmd);

        processes.push(ProcessInfo {
            pid: proc.pid().as_u32(),
            parent: proc.parent().map(sysinfo::Pid::as_u32),
            name,
            cmd,
            cwd,
            label,
            cpu: proc.cpu_usage(),
            memory_bytes: proc.memory(),
            run_secs: proc.run_time(),
            matched_worktree: Some(matched),
        });
    }

    // Group by worktree, agents first within a worktree, then by pid.
    processes.sort_by(|a, b| {
        a.matched_worktree
            .cmp(&b.matched_worktree)
            .then(b.label.is_agent().cmp(&a.label.is_agent()))
            .then(a.pid.cmp(&b.pid))
    });

    ProcessSnapshot { processes }
}

/// Machine-wide scan for coding-agent processes (Claude / Codex / …) that
/// expose a working directory. Returns each agent's CWD, deduplicated. The home
/// view resolves these to their enclosing Git repositories (handoff §17.16).
///
/// Synchronous and possibly slow (a full process refresh) — call via
/// `tokio::task::spawn_blocking` from async code, like [`scan`].
pub fn scan_agent_cwds() -> Vec<PathBuf> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );

    let mut cwds = Vec::new();
    for proc in sys.processes().values() {
        let name = proc.name().to_string_lossy().to_string();
        let cmd = proc
            .cmd()
            .iter()
            .map(|s| s.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        if !classify(&name, &cmd).is_agent() {
            continue;
        }
        if let Some(cwd) = proc.cwd() {
            cwds.push(cwd.to_path_buf());
        }
    }

    cwds.sort();
    cwds.dedup();
    cwds
}
