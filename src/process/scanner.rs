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
    let self_pid = std::process::id();
    let mut processes = Vec::new();

    for proc in sys.processes().values() {
        // Never list wb300 itself — it shouldn't be a killable row in its own UI.
        if proc.pid().as_u32() == self_pid {
            continue;
        }
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

/// Best-effort terminate a process by PID, **only** on explicit user request —
/// wb300 never kills a process on its own. Re-confirms the live process still
/// has `expected_name` before signalling, so a PID reused between the confirm
/// dialog and the kill is refused rather than killing the wrong thing. Returns
/// `Ok(())` once the signal is sent. Synchronous (a quick OS call) — invoke via
/// `tokio::task::spawn_blocking` from async code.
pub fn kill(pid: u32, expected_name: &str) -> Result<(), String> {
    // Hard guard: never let the operator terminate wb300 out from under itself.
    if pid == std::process::id() {
        return Err("refusing to terminate wb300 itself".to_string());
    }
    let spid = sysinfo::Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[spid]),
        true,
        ProcessRefreshKind::everything(),
    );
    let Some(proc) = sys.process(spid) else {
        return Err(format!("process {pid} is no longer running"));
    };
    let name = proc.name().to_string_lossy().to_string();
    if name != expected_name {
        return Err(format!(
            "process {pid} is now '{name}', not '{expected_name}' — aborting (PID reused)"
        ));
    }
    if proc.kill() {
        Ok(())
    } else {
        Err(format!(
            "could not terminate process {pid} (permission denied?)"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_of_a_nonexistent_pid_errors() {
        // u32::MAX is not a live PID; kill must refuse, not panic.
        assert!(kill(u32::MAX, "ghost.exe").is_err());
    }

    #[test]
    fn kill_refuses_to_terminate_wb300_itself() {
        assert!(kill(std::process::id(), "whatever").is_err());
    }
}
