//! Filesystem watcher (notify).
//!
//! We watch the *source* directories of each worktree — pruning heavy/noisy
//! dirs (`.git`, `node_modules`, `target`, …) and capping the total — rather
//! than recursively watching the whole tree. This keeps us a good citizen of
//! the OS watch budget: on Linux, `inotify` has a per-user `max_user_watches`
//! limit shared with the user's editor and other tools, and a recursive watch
//! over `node_modules`/`target` can exhaust it. Anything beyond the cap is
//! covered by the event loop's periodic poll.

use std::path::PathBuf;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::UnboundedSender;

/// Directory names we never descend into or watch.
const IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".turbo",
    ".cache",
    "coverage",
    "venv",
    ".venv",
    "__pycache__",
    "vendor",
    ".idea",
    ".vscode",
];

/// Hard cap on watched directories. Protects the shared OS watch budget even on
/// pathological trees; anything past this is caught by the periodic poll.
const MAX_WATCH_DIRS: usize = 2048;

/// Watch the pruned source tree under each root, sending `()` on every relevant
/// change. The returned watcher must be kept alive for watching to continue;
/// the `usize` is how many directories ended up watched (for diagnostics).
pub fn watch(
    roots: &[PathBuf],
    tx: UnboundedSender<()>,
) -> notify::Result<(RecommendedWatcher, usize)> {
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(event) = res
            && is_relevant(&event)
        {
            // Unbounded + non-blocking: safe to call from notify's own thread.
            let _ = tx.send(());
        }
    })?;

    let mut watched = 0;
    for dir in collect_watch_dirs(roots) {
        // Non-recursive per directory bounds the watch count predictably (a
        // recursive watch on the root can't be told to skip heavy dirs).
        if watcher.watch(&dir, RecursiveMode::NonRecursive).is_ok() {
            watched += 1;
        }
    }
    Ok((watcher, watched))
}

/// Depth-first collection of source directories under `roots`, pruning ignored
/// directories and symlinks (which avoids cycles) and stopping at the cap.
fn collect_watch_dirs(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut stack: Vec<PathBuf> = roots.to_vec();
    while let Some(dir) = stack.pop() {
        if dirs.len() >= MAX_WATCH_DIRS {
            break;
        }
        dirs.push(dir.clone());
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            // `file_type()` does not follow symlinks, so symlinked dirs are
            // skipped — no cycles, no escaping into ignored trees.
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            if IGNORED_DIRS.contains(&entry.file_name().to_string_lossy().as_ref()) {
                continue;
            }
            stack.push(entry.path());
        }
    }
    dirs
}

fn is_relevant(event: &Event) -> bool {
    // Saves/creates/removes/renames matter; pure reads do not.
    !matches!(event.kind, EventKind::Access(_))
}
