//! Path normalization and worktree prefix matching.
//!
//! Process CWDs and worktree roots come from different sources (sysinfo vs git)
//! and may differ in separators and case, so we normalize both before matching.

/// Normalize a path for comparison: unify separators, drop a trailing slash,
/// and lowercase on Windows (case-insensitive filesystem).
pub fn normalize(path: &str) -> String {
    let unified = path.replace('\\', "/");
    let trimmed = unified.trim_end_matches('/');
    if cfg!(windows) {
        trimmed.to_lowercase()
    } else {
        trimmed.to_string()
    }
}

/// Index of the longest `root` in `roots` that is a path-prefix of `cwd`.
/// All inputs must already be [`normalize`]d. Matching is path-boundary aware,
/// so `/a/repository` does not match the root `/a/repo`.
pub fn longest_prefix_match(cwd: &str, roots: &[String]) -> Option<usize> {
    roots
        .iter()
        .enumerate()
        .filter(|(_, root)| !root.is_empty() && is_path_prefix(cwd, root))
        .max_by_key(|(_, root)| root.len())
        .map(|(i, _)| i)
}

fn is_path_prefix(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_longest_matching_root() {
        let roots = vec![normalize("/a/repo"), normalize("/a/repo/sub")];
        assert_eq!(
            longest_prefix_match(&normalize("/a/repo/sub/x"), &roots),
            Some(1)
        );
        assert_eq!(
            longest_prefix_match(&normalize("/a/repo/y"), &roots),
            Some(0)
        );
        assert_eq!(longest_prefix_match(&normalize("/a/repo"), &roots), Some(0));
    }

    #[test]
    fn respects_path_boundaries() {
        let roots = vec![normalize("/a/repo")];
        assert_eq!(
            longest_prefix_match(&normalize("/a/repository"), &roots),
            None
        );
        assert_eq!(longest_prefix_match(&normalize("/elsewhere"), &roots), None);
    }

    #[test]
    fn windows_is_case_insensitive() {
        if cfg!(windows) {
            let roots = vec![normalize("C:\\A\\Repo")];
            assert_eq!(
                longest_prefix_match(&normalize("c:/a/repo/sub"), &roots),
                Some(0)
            );
        }
    }
}
