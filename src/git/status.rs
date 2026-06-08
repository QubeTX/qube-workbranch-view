//! Parser for `git status --porcelain=v2 --branch -z`.
//!
//! Porcelain v2 emits `# branch.*` headers then one record per changed path.
//! With `-z`, every record is NUL-terminated, and rename/copy (`2`) records are
//! followed by an extra NUL-terminated original-path field that must be consumed.

/// Per-worktree working-tree status.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorktreeStatus {
    pub clean: bool,
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicted: usize,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub upstream: Option<String>,
    /// True when the branch has a configured upstream that no longer exists.
    pub upstream_gone: bool,
    /// Current branch from the status header (`None` when detached).
    pub branch_head: Option<String>,
}

/// Parse `git status --porcelain=v2 --branch -z` output.
pub fn parse_status_v2(bytes: &[u8]) -> WorktreeStatus {
    let text = String::from_utf8_lossy(bytes);
    let mut s = WorktreeStatus::default();
    let mut had_ab = false;

    // We iterate manually so type-`2` (rename) records can consume the extra
    // original-path token that follows them.
    let mut it = text.split('\0');
    while let Some(entry) = it.next() {
        if entry.is_empty() {
            continue;
        }
        if let Some(header) = entry.strip_prefix("# ") {
            parse_header(header, &mut s, &mut had_ab);
            continue;
        }
        match entry.as_bytes()[0] {
            b'1' => count_xy(entry, &mut s),
            b'2' => {
                count_xy(entry, &mut s);
                // The rename/copy original path is its own NUL-terminated field.
                let _ = it.next();
            }
            b'u' => s.conflicted += 1,
            b'?' => s.untracked += 1,
            b'!' => {} // ignored paths
            _ => {}
        }
    }

    // git omits `# branch.ab` when the upstream is configured but missing.
    s.upstream_gone = s.upstream.is_some() && !had_ab;
    s.clean = s.staged == 0 && s.unstaged == 0 && s.untracked == 0 && s.conflicted == 0;
    s
}

fn parse_header(header: &str, s: &mut WorktreeStatus, had_ab: &mut bool) {
    if let Some(head) = header.strip_prefix("branch.head ") {
        s.branch_head = (head != "(detached)").then(|| head.to_string());
    } else if let Some(up) = header.strip_prefix("branch.upstream ") {
        s.upstream = Some(up.to_string());
    } else if let Some(ab) = header.strip_prefix("branch.ab ") {
        *had_ab = true;
        for tok in ab.split_whitespace() {
            if let Some(n) = tok.strip_prefix('+') {
                s.ahead = n.parse().ok();
            } else if let Some(n) = tok.strip_prefix('-') {
                s.behind = n.parse().ok();
            }
        }
    }
}

/// Count staged/unstaged from the two-char XY field of a `1`/`2` record.
/// Layout: `<type> <XY> <sub> ...`, so XY is the second whitespace field.
fn count_xy(entry: &str, s: &mut WorktreeStatus) {
    let Some(xy) = entry.split(' ').nth(1) else {
        return;
    };
    let mut chars = xy.chars();
    let x = chars.next().unwrap_or('.');
    let y = chars.next().unwrap_or('.');
    if x != '.' {
        s.staged += 1;
    }
    if y != '.' {
        s.unstaged += 1;
    }
}

impl WorktreeStatus {
    /// True if there is any ahead/behind divergence from upstream.
    pub fn diverged(&self) -> bool {
        self.ahead.unwrap_or(0) > 0 || self.behind.unwrap_or(0) > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build NUL-joined porcelain v2 output from records.
    fn fixture(records: &[&str]) -> Vec<u8> {
        let mut out = String::new();
        for r in records {
            out.push_str(r);
            out.push('\0');
        }
        out.into_bytes()
    }

    #[test]
    fn clean_branch_with_upstream() {
        let bytes = fixture(&[
            "# branch.oid abc123",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +0 -0",
        ]);
        let s = parse_status_v2(&bytes);
        assert!(s.clean);
        assert_eq!(s.branch_head.as_deref(), Some("main"));
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!(s.ahead, Some(0));
        assert_eq!(s.behind, Some(0));
        assert!(!s.upstream_gone);
    }

    #[test]
    fn counts_staged_unstaged_untracked_conflicted() {
        let bytes = fixture(&[
            "# branch.head feature/x",
            "# branch.ab +2 -1",
            "1 M. N... 100644 100644 100644 aaa bbb src/staged.rs",
            "1 .M N... 100644 100644 100644 aaa bbb src/unstaged.rs",
            "1 MM N... 100644 100644 100644 aaa bbb src/both.rs",
            "u UU N... 1 2 3 4 aaa bbb ccc src/conflict.rs",
            "? new_file.rs",
            "! ignored.rs",
        ]);
        let s = parse_status_v2(&bytes);
        assert!(!s.clean);
        // staged: staged.rs (M.) + both.rs (MM) = 2
        assert_eq!(s.staged, 2);
        // unstaged: unstaged.rs (.M) + both.rs (MM) = 2
        assert_eq!(s.unstaged, 2);
        assert_eq!(s.untracked, 1);
        assert_eq!(s.conflicted, 1);
        assert_eq!(s.ahead, Some(2));
        assert_eq!(s.behind, Some(1));
    }

    #[test]
    fn rename_record_consumes_original_path() {
        // A `2` record is followed by its original path as a separate field.
        let bytes = fixture(&[
            "# branch.head main",
            "# branch.ab +0 -0",
            "2 R. N... 100644 100644 100644 aaa bbb R100 src/new_name.rs",
            "src/old_name.rs",
            "? after.rs",
        ]);
        let s = parse_status_v2(&bytes);
        assert_eq!(s.staged, 1); // the rename (R.)
        assert_eq!(s.untracked, 1); // after.rs — proves old_name.rs wasn't miscounted
    }

    #[test]
    fn upstream_gone_when_ab_absent() {
        let bytes = fixture(&[
            "# branch.head feature/orphan",
            "# branch.upstream origin/feature/orphan",
        ]);
        let s = parse_status_v2(&bytes);
        assert!(s.upstream_gone);
        assert_eq!(s.ahead, None);
    }

    #[test]
    fn detached_head_has_no_branch() {
        let bytes = fixture(&["# branch.oid abc", "# branch.head (detached)"]);
        let s = parse_status_v2(&bytes);
        assert_eq!(s.branch_head, None);
    }
}
