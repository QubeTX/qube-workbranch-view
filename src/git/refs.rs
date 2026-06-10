//! Parser for `git for-each-ref` with NUL-separated fields.

/// The `--format` passed to `git for-each-ref`. Fields are separated by NUL
/// (`%00`); records are newline-separated (refs cannot contain newlines).
pub const FOR_EACH_REF_FORMAT: &str = concat!(
    "%(refname)%00",
    "%(refname:short)%00",
    "%(objectname)%00",
    "%(upstream:short)%00",
    "%(committerdate:iso8601-strict)%00",
    "%(subject)%00",
    "%(worktreepath)%00",
    // Ahead/behind vs the upstream for EVERY branch (not just checked-out
    // ones): "", "gone", "ahead N", "behind M", or "ahead N, behind M".
    "%(upstream:track,nobracket)%00",
    // Set only on symbolic refs — refs/remotes/origin/HEAD yields the remote
    // default branch (e.g. "origin/main"), our trunk hint.
    "%(symref:short)"
);

/// One branch / ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    /// Full ref name, e.g. `refs/heads/main` or `refs/remotes/origin/main`.
    pub full_ref: String,
    /// Short name, e.g. `main` or `origin/main`.
    pub short: String,
    /// Commit object id at the tip.
    pub oid: String,
    /// Upstream short name, if the branch tracks one.
    pub upstream: Option<String>,
    /// Committer date of the tip (ISO-8601 strict), kept as text for now.
    pub committer_date: Option<String>,
    /// Subject line of the tip commit.
    pub subject: Option<String>,
    /// Path of the worktree that has this branch checked out, if any.
    pub worktree_path: Option<String>,
    /// Commits ahead of the upstream (`Some(0)` when in sync; `None` when
    /// there is no upstream or it is gone).
    pub ahead: Option<u32>,
    /// Commits behind the upstream (same conventions as `ahead`).
    pub behind: Option<u32>,
    /// True when the configured upstream no longer exists.
    pub upstream_gone: bool,
    /// For symbolic refs (refs/remotes/origin/HEAD): the ref it points at,
    /// e.g. `origin/main` — identifies the remote default branch.
    pub symref_target: Option<String>,
    /// True for `refs/remotes/*` (remote-tracking) refs.
    pub is_remote: bool,
}

/// Parse `for-each-ref` output produced with [`FOR_EACH_REF_FORMAT`].
pub fn parse_refs(bytes: &[u8]) -> Vec<BranchInfo> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();

    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw); // tolerate CRLF
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split('\0');
        let full_ref = fields.next().unwrap_or("").to_string();
        if full_ref.is_empty() {
            continue;
        }
        let short = fields.next().unwrap_or("").to_string();
        let oid = fields.next().unwrap_or("").to_string();
        let upstream = non_empty(fields.next());
        let committer_date = non_empty(fields.next());
        let subject = non_empty(fields.next());
        let worktree_path = non_empty(fields.next());
        let (ahead, behind, upstream_gone) =
            parse_track(fields.next().unwrap_or(""), upstream.is_some());
        let symref_target = non_empty(fields.next());
        out.push(BranchInfo {
            is_remote: full_ref.starts_with("refs/remotes/"),
            full_ref,
            short,
            oid,
            upstream,
            committer_date,
            subject,
            worktree_path,
            ahead,
            behind,
            upstream_gone,
            symref_target,
        });
    }
    out
}

/// Parse `%(upstream:track,nobracket)`: empty (in sync, when an upstream is
/// configured), `gone`, `ahead N`, `behind M`, or `ahead N, behind M`.
fn parse_track(track: &str, has_upstream: bool) -> (Option<u32>, Option<u32>, bool) {
    let track = track.trim();
    if track == "gone" {
        return (None, None, true);
    }
    if !has_upstream {
        return (None, None, false);
    }
    let (mut ahead, mut behind) = (0u32, 0u32);
    for part in track.split(',') {
        let part = part.trim();
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n.trim().parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n.trim().parse().unwrap_or(0);
        }
    }
    (Some(ahead), Some(behind), false)
}

fn non_empty(field: Option<&str>) -> Option<String> {
    match field {
        Some(v) if !v.is_empty() => Some(v.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Join fields with NUL and records with newline, like real `for-each-ref`.
    fn fixture(records: &[&[&str]]) -> Vec<u8> {
        records
            .iter()
            .map(|fields| fields.join("\0"))
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes()
    }

    #[test]
    fn parses_local_and_remote_with_optional_fields() {
        let bytes = fixture(&[
            &[
                "refs/heads/main",
                "main",
                "aaaa1111",
                "origin/main",
                "2026-06-08T10:00:00-04:00",
                "Initial commit",
                "/repo",
                "",
                "",
            ],
            &[
                "refs/remotes/origin/main",
                "origin/main",
                "aaaa1111",
                "",
                "2026-06-08T10:00:00-04:00",
                "Initial commit",
                "",
                "",
                "",
            ],
        ]);
        let refs = parse_refs(&bytes);
        assert_eq!(refs.len(), 2);

        let main = &refs[0];
        assert!(!main.is_remote);
        assert_eq!(main.short, "main");
        assert_eq!(main.upstream.as_deref(), Some("origin/main"));
        assert_eq!(main.subject.as_deref(), Some("Initial commit"));
        assert_eq!(main.worktree_path.as_deref(), Some("/repo"));
        // Upstream configured + empty track field = in sync.
        assert_eq!(main.ahead, Some(0));
        assert_eq!(main.behind, Some(0));
        assert!(!main.upstream_gone);

        let remote = &refs[1];
        assert!(remote.is_remote);
        assert_eq!(remote.upstream, None); // empty field -> None
        assert_eq!(remote.worktree_path, None);
        assert_eq!(remote.ahead, None); // no upstream -> unknown, not 0
    }

    #[test]
    fn parses_upstream_track_states() {
        let bytes = fixture(&[
            &[
                "refs/heads/ahead",
                "ahead",
                "aaaa",
                "origin/ahead",
                "",
                "",
                "",
                "ahead 2",
                "",
            ],
            &[
                "refs/heads/diverged",
                "diverged",
                "bbbb",
                "origin/diverged",
                "",
                "",
                "",
                "ahead 2, behind 3",
                "",
            ],
            &[
                "refs/heads/orphan",
                "orphan",
                "cccc",
                "origin/orphan",
                "",
                "",
                "",
                "gone",
                "",
            ],
        ]);
        let refs = parse_refs(&bytes);
        assert_eq!(refs[0].ahead, Some(2));
        assert_eq!(refs[0].behind, Some(0));
        assert_eq!(refs[1].ahead, Some(2));
        assert_eq!(refs[1].behind, Some(3));
        assert!(refs[2].upstream_gone);
        assert_eq!(refs[2].ahead, None);
    }

    #[test]
    fn parses_remote_head_symref() {
        let bytes = fixture(&[&[
            "refs/remotes/origin/HEAD",
            "origin",
            "aaaa1111",
            "",
            "",
            "",
            "",
            "",
            "origin/main",
        ]]);
        let refs = parse_refs(&bytes);
        assert_eq!(refs[0].symref_target.as_deref(), Some("origin/main"));
    }

    #[test]
    fn subject_with_spaces_survives() {
        let bytes = fixture(&[&[
            "refs/heads/feature/x",
            "feature/x",
            "bbbb2222",
            "",
            "2026-06-08T10:00:00-04:00",
            "feat: add the thing, with commas",
            "",
        ]]);
        let refs = parse_refs(&bytes);
        assert_eq!(
            refs[0].subject.as_deref(),
            Some("feat: add the thing, with commas")
        );
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(parse_refs(b"").is_empty());
        assert!(parse_refs(b"\n").is_empty());
    }
}
