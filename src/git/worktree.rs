//! Parser for `git worktree list --porcelain -z`.
//!
//! With `-z`, each attribute is NUL-terminated and records are separated by an
//! empty (NUL-only) entry, so paths containing spaces or newlines are safe.

use super::status::{FileChange, WorktreeStatus};

/// One worktree as reported by `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorktreeRecord {
    pub path: String,
    pub head: Option<String>,
    /// Full ref, e.g. `refs/heads/feature/x` (absent when detached/bare).
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    /// `Some(reason)` when locked; the reason may be empty.
    pub locked: Option<String>,
    /// `Some(reason)` when prunable; the reason may be empty.
    pub prunable: Option<String>,
    /// Working-tree status — `None` until filled by snapshot capture (the
    /// porcelain parser leaves it unset).
    pub status: Option<WorktreeStatus>,
    /// Files this worktree has touched (working-tree changes — untracked
    /// included — plus files committed since base), each with its change kind.
    /// Filled by snapshot capture; drives collision detection and per-branch
    /// file lists. Sorted by path, de-duplicated (working-tree kind wins).
    pub touched: Vec<FileChange>,
}

impl WorktreeRecord {
    /// Branch name without the `refs/heads/` prefix.
    pub fn branch_short(&self) -> Option<&str> {
        self.branch
            .as_deref()
            .map(|b| b.strip_prefix("refs/heads/").unwrap_or(b))
    }

    /// First 8 chars of the HEAD oid, if any.
    pub fn short_head(&self) -> Option<&str> {
        self.head.as_deref().map(|h| h.get(..8).unwrap_or(h))
    }

    /// A human label: branch name, `(detached <oid>)`, `(bare)`, or `(unknown)`.
    pub fn display_name(&self) -> String {
        if self.bare {
            return "(bare)".to_string();
        }
        if let Some(branch) = self.branch_short() {
            return branch.to_string();
        }
        if self.detached {
            return format!("(detached {})", self.short_head().unwrap_or("?"));
        }
        "(unknown)".to_string()
    }
}

/// Parse the NUL-delimited porcelain output into worktree records.
pub fn parse_worktrees(bytes: &[u8]) -> Vec<WorktreeRecord> {
    let text = String::from_utf8_lossy(bytes);
    let mut records = Vec::new();
    let mut current: Option<WorktreeRecord> = None;

    for entry in text.split('\0') {
        if entry.is_empty() {
            // Empty entry = record boundary.
            if let Some(rec) = current.take() {
                records.push(rec);
            }
            continue;
        }

        let (key, value) = match entry.split_once(' ') {
            Some((k, v)) => (k, Some(v)),
            None => (entry, None),
        };
        let rec = current.get_or_insert_with(WorktreeRecord::default);
        match key {
            "worktree" => rec.path = value.unwrap_or_default().to_string(),
            "HEAD" => rec.head = value.map(str::to_string),
            "branch" => rec.branch = value.map(str::to_string),
            "detached" => rec.detached = true,
            "bare" => rec.bare = true,
            "locked" => rec.locked = Some(value.unwrap_or_default().to_string()),
            "prunable" => rec.prunable = Some(value.unwrap_or_default().to_string()),
            _ => {}
        }
    }
    // Flush a trailing record if the stream didn't end with a boundary.
    if let Some(rec) = current.take() {
        records.push(rec);
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a NUL-delimited porcelain fixture from `(line)` groups.
    fn fixture(records: &[&[&str]]) -> Vec<u8> {
        let mut out = String::new();
        for rec in records {
            for line in *rec {
                out.push_str(line);
                out.push('\0');
            }
            out.push('\0'); // record boundary
        }
        out.into_bytes()
    }

    #[test]
    fn parses_main_linked_detached_bare_and_locked() {
        let bytes = fixture(&[
            &[
                "worktree /repo",
                "HEAD aaaaaaaaaaaa",
                "branch refs/heads/main",
            ],
            &[
                "worktree /repo-feat",
                "HEAD bbbbbbbbbbbb",
                "branch refs/heads/feature/auth-agent",
            ],
            &["worktree /repo-det", "HEAD cccccccccccc", "detached"],
            &["worktree /repo-bare", "bare"],
            &[
                "worktree /repo-wip",
                "HEAD dddddddddddd",
                "branch refs/heads/wip",
                "locked needs review",
                "prunable gitdir file points to non-existent location",
            ],
        ]);
        let wts = parse_worktrees(&bytes);
        assert_eq!(wts.len(), 5);

        assert_eq!(wts[0].path, "/repo");
        assert_eq!(wts[0].branch_short(), Some("main"));
        assert!(!wts[0].detached && !wts[0].bare);

        assert_eq!(wts[1].branch_short(), Some("feature/auth-agent"));

        assert!(wts[2].detached);
        assert_eq!(wts[2].display_name(), "(detached cccccccc)");

        assert!(wts[3].bare);
        assert_eq!(wts[3].display_name(), "(bare)");

        assert_eq!(wts[4].locked.as_deref(), Some("needs review"));
        assert!(wts[4].prunable.is_some());
    }

    #[test]
    fn handles_paths_with_spaces() {
        let bytes = fixture(&[&[
            "worktree /repos/my project/wt one",
            "HEAD eeeeeeeeeeee",
            "branch refs/heads/feature/x",
        ]]);
        let wts = parse_worktrees(&bytes);
        assert_eq!(wts.len(), 1);
        assert_eq!(wts[0].path, "/repos/my project/wt one");
    }

    #[test]
    fn locked_without_reason_is_some_empty() {
        let bytes = fixture(&[&["worktree /r", "HEAD ffffffffffff", "locked"]]);
        let wts = parse_worktrees(&bytes);
        assert_eq!(wts[0].locked.as_deref(), Some(""));
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(parse_worktrees(b"").is_empty());
    }
}
