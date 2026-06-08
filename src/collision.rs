//! Changed-file collision detection across worktrees (handoff §17.4).
//!
//! Two agents can clobber each other even when both have committed and their
//! worktrees are clean, so the "touched files" set per worktree includes
//! committed-since-base changes. A file touched by ≥2 worktrees is a collision,
//! ranked by how risky the path is.

use serde::Serialize;

use crate::git::WorktreeRecord;

/// How risky a colliding path is. Ordered so `Critical` is greatest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Low => "LOW",
            Severity::Medium => "MEDIUM",
            Severity::High => "HIGH",
            Severity::Critical => "CRITICAL",
        }
    }
}

/// One file touched by more than one worktree.
#[derive(Debug, Clone, Serialize)]
pub struct Collision {
    pub file: String,
    /// Indices into the snapshot's worktree list.
    pub worktrees: Vec<usize>,
    pub severity: Severity,
}

const LOCKFILES: &[&str] = &[
    "cargo.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "poetry.lock",
    "go.sum",
    "composer.lock",
    "gemfile.lock",
];

/// Rank a path by collision risk using built-in defaults (config-overridable
/// later, handoff §12.7).
pub fn classify_severity(path: &str) -> Severity {
    let lower = path.replace('\\', "/").to_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    // Git paths are repo-relative (no leading slash); prepend one so segment
    // probes like "/src/db/" also match top-level paths such as "src/db/x".
    let probe = format!("/{lower}");

    if LOCKFILES.contains(&base)
        || probe.contains("/migrations/")
        || probe.ends_with("/schema.prisma")
        || probe.contains("/.github/workflows/")
    {
        return Severity::Critical;
    }
    if base == "package.json"
        || base == "cargo.toml"
        || base == "go.mod"
        || probe.contains("/src/db/")
        || probe.contains("/src/auth/")
        || probe.contains("/db/migrations")
    {
        return Severity::High;
    }
    if probe.ends_with(".md")
        || probe.contains("/docs/")
        || probe.contains("/tests/")
        || probe.contains("/test/")
        || probe.ends_with(".txt")
    {
        return Severity::Low;
    }
    Severity::Medium
}

/// Compute collisions: invert touched-file → worktrees and keep files touched by
/// two or more. Sorted by severity (highest first), then path.
pub fn compute(worktrees: &[WorktreeRecord]) -> Vec<Collision> {
    use std::collections::BTreeMap;

    let mut by_file: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (idx, wt) in worktrees.iter().enumerate() {
        for file in &wt.touched {
            by_file.entry(file.as_str()).or_default().push(idx);
        }
    }

    let mut collisions: Vec<Collision> = by_file
        .into_iter()
        .filter(|(_, wts)| wts.len() >= 2)
        .map(|(file, worktrees)| Collision {
            severity: classify_severity(file),
            file: file.to_string(),
            worktrees,
        })
        .collect();

    collisions.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.file.cmp(&b.file))
    });
    collisions
}

/// Count of collisions that involve worktree `idx`.
pub fn count_for_worktree(collisions: &[Collision], idx: usize) -> usize {
    collisions
        .iter()
        .filter(|c| c.worktrees.contains(&idx))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wt(path: &str, touched: &[&str]) -> WorktreeRecord {
        WorktreeRecord {
            path: path.to_string(),
            touched: touched.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn detects_only_shared_files() {
        let worktrees = vec![
            wt("/a", &["src/x.rs", "Cargo.lock"]),
            wt("/b", &["src/x.rs", "README.md"]),
        ];
        let collisions = compute(&worktrees);
        assert_eq!(collisions.len(), 1);
        assert_eq!(collisions[0].file, "src/x.rs");
        assert_eq!(collisions[0].worktrees, vec![0, 1]);
    }

    #[test]
    fn ranks_severity() {
        assert_eq!(classify_severity("Cargo.lock"), Severity::Critical);
        assert_eq!(
            classify_severity("db/migrations/0001.sql"),
            Severity::Critical
        );
        assert_eq!(classify_severity("package.json"), Severity::High);
        assert_eq!(classify_severity("src/db/pool.rs"), Severity::High);
        assert_eq!(classify_severity("src/main.rs"), Severity::Medium);
        assert_eq!(classify_severity("docs/guide.md"), Severity::Low);
        assert_eq!(
            classify_severity("prisma/schema.prisma"),
            Severity::Critical
        );
        assert_eq!(
            classify_severity("prisma/app.schema.prisma"),
            Severity::Medium
        );
    }

    #[test]
    fn sorts_critical_first() {
        let worktrees = vec![
            wt("/a", &["src/main.rs", "Cargo.lock"]),
            wt("/b", &["src/main.rs", "Cargo.lock"]),
        ];
        let collisions = compute(&worktrees);
        assert_eq!(collisions[0].file, "Cargo.lock");
        assert_eq!(collisions[0].severity, Severity::Critical);
    }
}
