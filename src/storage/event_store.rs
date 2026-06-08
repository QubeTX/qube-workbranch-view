//! Append-only JSON-lines store for [`ArchivedEvent`]s.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;

use super::events::{ArchivedEvent, epoch_secs};

/// Append one event as a JSON line, creating the directory/file as needed.
pub fn append(path: &Path, event: &ArchivedEvent) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let line = serde_json::to_string(event).map_err(std::io::Error::other)?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")
}

/// Load all events (oldest first), skipping any unparseable lines. A missing
/// file is simply an empty history.
pub fn load(path: &Path) -> Vec<ArchivedEvent> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

/// Load events, drop those older than `keep_secs`, keep at most `max_events`
/// (the newest), and compact the on-disk file to that set so it can't grow
/// without bound. Returns the kept events oldest-first.
pub fn prune(path: &Path, keep_secs: u64, max_events: usize) -> Vec<ArchivedEvent> {
    let cutoff = epoch_secs().saturating_sub(keep_secs);
    let mut events: Vec<ArchivedEvent> = load(path)
        .into_iter()
        .filter(|e| e.at_epoch >= cutoff)
        .collect();
    if events.len() > max_events {
        events.drain(0..events.len() - max_events);
    }
    let _ = rewrite(path, &events); // best-effort compaction
    events
}

/// Atomically-ish replace the file with exactly `events`.
fn rewrite(path: &Path, events: &[ArchivedEvent]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut out = String::new();
    for event in events {
        out.push_str(&serde_json::to_string(event).map_err(std::io::Error::other)?);
        out.push('\n');
    }
    std::fs::write(path, out)
}

/// Spawn one dedicated writer thread that owns the file handle and appends
/// events in arrival order. Returns the sender; dropping it ends the thread.
/// A single writer avoids the interleaved/torn lines that concurrent appends
/// could otherwise produce.
pub fn spawn_writer(path: PathBuf) -> Sender<ArchivedEvent> {
    let (tx, rx) = std::sync::mpsc::channel::<ArchivedEvent>();
    thread::spawn(move || {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut file = match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => file,
            Err(err) => {
                tracing::warn!("event archive open failed: {err}");
                return;
            }
        };
        while let Ok(event) = rx.recv() {
            match serde_json::to_string(&event) {
                Ok(line) => {
                    if let Err(err) = writeln!(file, "{line}") {
                        tracing::warn!("event archive write failed: {err}");
                    }
                }
                Err(err) => tracing::warn!("event serialize failed: {err}"),
            }
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::events::{DirtySummary, EventKind};

    #[test]
    fn round_trips_events() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("wb300-events-{}", std::process::id()));
        let path = dir.join("events.jsonl");
        let _ = std::fs::remove_file(&path);

        let created = ArchivedEvent::new(
            EventKind::WorktreeCreated,
            "/repo-feat".to_string(),
            Some("feature/x".to_string()),
            Some("abcd1234".to_string()),
            None,
        );
        let removed = ArchivedEvent::new(
            EventKind::WorktreeRemoved,
            "/repo-old".to_string(),
            Some("wip".to_string()),
            None,
            Some(DirtySummary {
                unstaged: 3,
                ..DirtySummary::default()
            }),
        );
        append(&path, &created).unwrap();
        append(&path, &removed).unwrap();

        let loaded = load(&path);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0], created);
        assert_eq!(loaded[1], removed);
        assert_eq!(loaded[1].dirty.unwrap().unstaged, 3);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_empty() {
        assert!(load(Path::new("/no/such/wb300/events.jsonl")).is_empty());
    }
}
