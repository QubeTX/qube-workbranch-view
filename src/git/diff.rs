//! Changed-file queries for collision detection (handoff §11.4–11.5).

use std::collections::BTreeSet;
use std::path::Path;

use super::commands::run_git;

/// Parse a NUL-delimited path list (git `-z` output) into owned strings.
pub fn parse_nul_paths(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Files a worktree has touched: unstaged + staged working-tree changes, plus
/// files committed since `base` (so two clean-but-committed worktrees still
/// collide). Untracked files are intentionally excluded. Best-effort — a failing
/// sub-query simply contributes nothing. Returns a sorted, de-duplicated list.
pub async fn touched_files(worktree: &Path, base: Option<&str>) -> Vec<String> {
    let mut files = BTreeSet::new();

    for args in [
        ["diff", "--name-only", "-z"].as_slice(),
        ["diff", "--staged", "--name-only", "-z"].as_slice(),
    ] {
        if let Ok(out) = run_git(Some(worktree), args).await
            && out.success()
        {
            files.extend(parse_nul_paths(&out.stdout));
        }
    }

    if let Some(base) = base {
        let range = format!("{base}...HEAD");
        if let Ok(out) = run_git(Some(worktree), &["diff", "--name-only", "-z", &range]).await
            && out.success()
        {
            files.extend(parse_nul_paths(&out.stdout));
        }
    }

    files.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nul_path_list() {
        assert_eq!(
            parse_nul_paths(b"src/a.rs\0src/b/c.rs\0"),
            vec!["src/a.rs", "src/b/c.rs"]
        );
        assert!(parse_nul_paths(b"").is_empty());
    }
}
