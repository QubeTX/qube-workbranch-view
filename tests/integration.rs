//! Integration tests against real temporary Git repositories.
//!
//! These shell out to the installed `git` (as wb300 itself does) and assert the
//! snapshot pipeline matches real-world porcelain output — the unit tests cover
//! the parsers with fixtures; these guard against format drift.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use wb300::git::{RepoIdentity, RepoSnapshot};

fn unique() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

fn temp_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "wb300-it-{tag}-{}-{}",
        std::process::id(),
        unique()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .expect("git should be installed");
    assert!(status.success(), "git {args:?} failed");
}

fn init_repo(dir: &Path) {
    git(dir, &["init", "-b", "main", "-q"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "wb300 test"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("README.md"), "hello\n").unwrap();
    git(dir, &["add", "README.md"]);
    git(dir, &["commit", "-q", "-m", "init"]);
}

#[tokio::test]
async fn captures_worktrees_and_dirty_status() {
    let dir = temp_dir("status");
    init_repo(&dir);

    std::fs::write(dir.join("staged.txt"), "a\n").unwrap();
    git(&dir, &["add", "staged.txt"]); // staged add
    std::fs::write(dir.join("README.md"), "hello world\n").unwrap(); // unstaged modify
    std::fs::write(dir.join("untracked.txt"), "u\n").unwrap(); // untracked

    let repo = RepoIdentity::discover(&dir).await.expect("discover");
    let snap = RepoSnapshot::capture(repo).await.expect("capture");

    assert_eq!(snap.worktrees.len(), 1);
    let status = snap.worktrees[0].status.as_ref().expect("status present");
    assert!(!status.clean, "repo should be dirty");
    assert_eq!(status.staged, 1, "one staged add");
    assert_eq!(status.unstaged, 1, "one unstaged modify");
    assert_eq!(status.untracked, 1, "one untracked file");
    assert_eq!(status.branch_head.as_deref(), Some("main"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn discovers_linked_worktree() {
    let dir = temp_dir("wt");
    init_repo(&dir);

    let mut linked = dir.clone();
    let name = format!("{}-feat", dir.file_name().unwrap().to_string_lossy());
    linked.set_file_name(name);
    git(
        &dir,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature/x",
            linked.to_str().unwrap(),
        ],
    );

    let repo = RepoIdentity::discover(&dir).await.expect("discover");
    let snap = RepoSnapshot::capture(repo).await.expect("capture");

    assert_eq!(snap.worktrees.len(), 2);
    let branches: Vec<String> = snap
        .worktrees
        .iter()
        .filter_map(|w| w.branch_short().map(str::to_string))
        .collect();
    assert!(branches.contains(&"main".to_string()));
    assert!(branches.contains(&"feature/x".to_string()));

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&linked);
}

fn sibling(dir: &Path, suffix: &str) -> PathBuf {
    let name = format!("{}-{suffix}", dir.file_name().unwrap().to_string_lossy());
    let mut path = dir.to_path_buf();
    path.set_file_name(name);
    path
}

#[tokio::test]
async fn detects_collision_across_worktrees() {
    let dir = temp_dir("collide");
    init_repo(&dir);

    let wt_a = sibling(&dir, "a");
    let wt_b = sibling(&dir, "b");
    git(
        &dir,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat/a",
            wt_a.to_str().unwrap(),
        ],
    );
    git(
        &dir,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat/b",
            wt_b.to_str().unwrap(),
        ],
    );

    // Both feature worktrees modify the same tracked file.
    std::fs::write(wt_a.join("README.md"), "change from a\n").unwrap();
    std::fs::write(wt_b.join("README.md"), "change from b\n").unwrap();

    let repo = RepoIdentity::discover(&dir).await.expect("discover");
    let snap = RepoSnapshot::capture(repo).await.expect("capture");

    let collision = snap
        .collisions
        .iter()
        .find(|c| c.file == "README.md")
        .expect("README.md should collide across feat/a and feat/b");
    assert!(collision.worktrees.len() >= 2);

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&wt_a);
    let _ = std::fs::remove_dir_all(&wt_b);
}
