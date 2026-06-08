//! Filesystem watcher (notify). Emits a unit signal on each relevant change;
//! the debouncer coalesces bursts into one refresh.

use std::path::{Path, PathBuf};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::UnboundedSender;

/// Directory names whose contents are too noisy to be worth waking up for.
const IGNORED_DIRS: &[&str] = &[
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
];

/// Watch each path in `paths` recursively, sending `()` on every relevant
/// change. The returned watcher must be kept alive for watching to continue.
pub fn watch(paths: &[PathBuf], tx: UnboundedSender<()>) -> notify::Result<RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(event) = res
            && is_relevant(&event)
        {
            // Unbounded + non-blocking: safe to call from notify's own thread.
            let _ = tx.send(());
        }
    })?;
    for path in paths {
        // Best-effort: a path may not exist yet (e.g. a worktree parent dir).
        let _ = watcher.watch(path, RecursiveMode::Recursive);
    }
    Ok(watcher)
}

fn is_relevant(event: &Event) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    // Wake up unless *every* affected path is in an ignored location.
    !event.paths.iter().all(|p| is_ignored(p))
}

fn is_ignored(path: &Path) -> bool {
    let unified = path.to_string_lossy().replace('\\', "/");
    if unified.contains("/.git/objects") || unified.contains("/.git/lfs") {
        return true;
    }
    path.components().any(|c| {
        let name = c.as_os_str().to_string_lossy();
        IGNORED_DIRS.contains(&name.as_ref())
    })
}
