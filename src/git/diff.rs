//! Committed-change queries for collision detection and per-branch file lists
//! (handoff §11.4–11.5).
//!
//! Working-tree changes (staged/unstaged/untracked) come from the status
//! parser's per-file records; this module only asks git for what status can't
//! see — files committed since the base ref.

use std::path::Path;

use super::commands::run_git;
use super::status::{ChangeKind, FileChange};

/// Files committed on this worktree's branch since `base` (`base...HEAD`), with
/// change kinds from `--name-status`. Best-effort — a failing query contributes
/// nothing, matching the old `touched_files` behavior.
pub async fn committed_files(worktree: &Path, base: &str) -> Vec<FileChange> {
    let range = format!("{base}...HEAD");
    match run_git(Some(worktree), &["diff", "--name-status", "-z", &range]).await {
        Ok(out) if out.success() => parse_name_status(&out.stdout),
        _ => Vec::new(),
    }
}

/// Parse `git diff --name-status -z` output: `STATUS NUL path NUL`, except
/// rename/copy records (`R<score>`/`C<score>`) which carry two paths —
/// `STATUS NUL old NUL new NUL`. The NEW path is reported.
pub fn parse_name_status(bytes: &[u8]) -> Vec<FileChange> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    let mut it = text.split('\0');
    while let Some(status) = it.next() {
        let Some(first) = status.chars().next() else {
            continue; // empty token (trailing NUL)
        };
        match first {
            'R' | 'C' => {
                let _old = it.next();
                let Some(new) = it.next() else { break };
                out.push(FileChange {
                    path: new.to_string(),
                    kind: ChangeKind::Renamed,
                });
            }
            _ => {
                let Some(path) = it.next() else { break };
                let kind = match first {
                    'A' => ChangeKind::Added,
                    'D' => ChangeKind::Deleted,
                    _ => ChangeKind::Modified, // M, T, U, X — treat as modified
                };
                out.push(FileChange {
                    path: path.to_string(),
                    kind,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_status_records() {
        let bytes = b"M\0src/a.rs\0A\0src/new.rs\0D\0gone.rs\0";
        let changes = parse_name_status(bytes);
        assert_eq!(
            changes,
            vec![
                FileChange {
                    path: "src/a.rs".into(),
                    kind: ChangeKind::Modified
                },
                FileChange {
                    path: "src/new.rs".into(),
                    kind: ChangeKind::Added
                },
                FileChange {
                    path: "gone.rs".into(),
                    kind: ChangeKind::Deleted
                },
            ]
        );
    }

    #[test]
    fn rename_consumes_both_paths_and_reports_the_new_one() {
        let bytes = b"R100\0src/old.rs\0src/new.rs\0M\0after.rs\0";
        let changes = parse_name_status(bytes);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].path, "src/new.rs");
        assert_eq!(changes[0].kind, ChangeKind::Renamed);
        // after.rs proves old.rs didn't desynchronize the token stream.
        assert_eq!(changes[1].path, "after.rs");
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(parse_name_status(b"").is_empty());
    }

    #[test]
    fn paths_with_spaces_survive() {
        let bytes = b"M\0src/my file.rs\0";
        let changes = parse_name_status(bytes);
        assert_eq!(changes[0].path, "src/my file.rs");
    }
}
