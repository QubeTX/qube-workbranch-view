//! Integration tests for the derived branch hierarchy against real temporary
//! Git repositories (with a real bare "origin"), exercising the team workflow:
//! main → `<dev>/wb-<date>` workbranch → task branches in worktrees.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use wb300::git::{BranchLifecycle, BranchRole, RepoIdentity, RepoSnapshot};

fn unique() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

fn temp_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "wb300-hier-{tag}-{}-{}",
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

fn sibling(dir: &Path, suffix: &str) -> PathBuf {
    let name = format!("{}-{suffix}", dir.file_name().unwrap().to_string_lossy());
    let mut path = dir.to_path_buf();
    path.set_file_name(name);
    path
}

fn commit_file(dir: &Path, name: &str, content: &str, msg: &str) {
    std::fs::write(dir.join(name), content).unwrap();
    git(dir, &["add", name]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// Create a sibling bare remote, wire it up as `origin`, push `main` upstream.
fn add_origin(dir: &Path) -> PathBuf {
    let bare = sibling(dir, "origin.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare", "-q"]);
    git(dir, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(dir, &["push", "-q", "-u", "origin", "main"]);
    bare
}

async fn capture(dir: &Path) -> RepoSnapshot {
    let repo = RepoIdentity::discover(dir).await.expect("discover");
    RepoSnapshot::capture(repo).await.expect("capture")
}

#[tokio::test]
async fn derives_the_convention_tree() {
    let dir = temp_dir("tree");
    init_repo(&dir);
    let origin = add_origin(&dir);

    git(&dir, &["checkout", "-q", "-b", "emmett/wb-2026-06-10"]);
    commit_file(&dir, "wb.txt", "wb\n", "wb work");
    git(
        &dir,
        &["push", "-q", "-u", "origin", "emmett/wb-2026-06-10"],
    );

    let task = sibling(&dir, "task");
    git(
        &dir,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat/x-1",
            task.to_str().unwrap(),
            "emmett/wb-2026-06-10",
        ],
    );
    commit_file(&task, "task.txt", "t\n", "task work");
    git(&task, &["push", "-q", "-u", "origin", "feat/x-1"]);
    git(&dir, &["checkout", "-q", "main"]);

    let snap = capture(&dir).await;
    let h = &snap.hierarchy;
    assert_eq!(h.trunk.as_deref(), Some("main"));
    assert!(!h.approximate);

    let main = h.node("main").expect("main node");
    assert_eq!(main.role, BranchRole::Trunk);
    assert_eq!(main.lifecycle, BranchLifecycle::Pushed);
    assert!(main.worktree.is_some(), "main is checked out at the root");

    let wb = h.node("emmett/wb-2026-06-10").expect("workbranch node");
    assert_eq!(wb.role, BranchRole::Workbranch);
    assert_eq!(wb.parent.as_deref(), Some("main"));
    assert_eq!(wb.ahead_of_parent, 1);
    assert_eq!(wb.behind_parent, Some(0));
    assert_eq!(wb.lifecycle, BranchLifecycle::Pushed);
    assert!(wb.worktree.is_none(), "the workbranch has no checkout");

    let task_node = h.node("feat/x-1").expect("task node");
    assert_eq!(task_node.role, BranchRole::Task);
    assert_eq!(task_node.parent.as_deref(), Some("emmett/wb-2026-06-10"));
    assert_eq!(task_node.ahead_of_parent, 1);
    assert_eq!(task_node.behind_parent, Some(0));
    assert_eq!(task_node.lifecycle, BranchLifecycle::Pushed);
    assert!(task_node.worktree.is_some(), "task maps to its worktree");

    // Depth-first: main, then the workbranch, then its task.
    let names: Vec<&str> = h.nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, vec!["main", "emmett/wb-2026-06-10", "feat/x-1"]);

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&task);
    let _ = std::fs::remove_dir_all(&origin);
}

#[tokio::test]
async fn fresh_task_at_workbranch_tip_is_fresh() {
    let dir = temp_dir("fresh");
    init_repo(&dir);
    let origin = add_origin(&dir);

    git(&dir, &["checkout", "-q", "-b", "emmett/wb-2026-06-10"]);
    commit_file(&dir, "wb.txt", "wb\n", "wb work");
    git(&dir, &["branch", "-q", "feat/fresh-2"]); // cut at the wb tip, no work
    git(&dir, &["checkout", "-q", "main"]);

    let snap = capture(&dir).await;
    let fresh = snap.hierarchy.node("feat/fresh-2").expect("fresh node");
    assert_eq!(
        fresh.parent.as_deref(),
        Some("emmett/wb-2026-06-10"),
        "equal-tip tie resolves to the wb-named parent"
    );
    assert_eq!(fresh.lifecycle, BranchLifecycle::Fresh);
    assert!(!fresh.merged_into_parent);

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&origin);
}

#[tokio::test]
async fn merge_commit_marks_the_branch_merged() {
    let dir = temp_dir("merge");
    init_repo(&dir);
    let origin = add_origin(&dir);

    git(&dir, &["checkout", "-q", "-b", "emmett/wb-2026-06-09"]);
    commit_file(&dir, "wb.txt", "wb\n", "wb work");
    git(&dir, &["checkout", "-q", "main"]);
    git(
        &dir,
        &[
            "merge",
            "-q",
            "--no-ff",
            "--no-edit",
            "emmett/wb-2026-06-09",
        ],
    );

    let snap = capture(&dir).await;
    let wb = snap
        .hierarchy
        .node("emmett/wb-2026-06-09")
        .expect("wb node");
    assert_eq!(wb.ahead_of_parent, 0);
    assert!(wb.merged_into_parent);
    assert_eq!(wb.lifecycle, BranchLifecycle::Merged);

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&origin);
}

#[tokio::test]
async fn squash_merge_with_deleted_remote_reads_committed_and_gone() {
    // A squash merge is graph-invisible: the lingering local branch keeps its
    // own commits (ahead of parent) with a gone upstream. The documented
    // verdict is Committed + upstream_gone — never silently "Merged".
    let dir = temp_dir("squash");
    init_repo(&dir);
    let origin = add_origin(&dir);

    git(&dir, &["checkout", "-q", "-b", "emmett/wb-2026-06-09"]);
    commit_file(&dir, "wb.txt", "wb\n", "wb work");
    git(
        &dir,
        &["push", "-q", "-u", "origin", "emmett/wb-2026-06-09"],
    );
    git(&dir, &["checkout", "-q", "main"]);
    git(&dir, &["merge", "-q", "--squash", "emmett/wb-2026-06-09"]);
    git(&dir, &["commit", "-q", "-m", "squash: wb work"]);
    git(
        &dir,
        &["push", "-q", "origin", "--delete", "emmett/wb-2026-06-09"],
    );

    let snap = capture(&dir).await;
    let wb = snap
        .hierarchy
        .node("emmett/wb-2026-06-09")
        .expect("wb node");
    assert!(wb.upstream_gone);
    assert!(
        wb.ahead_of_parent > 0,
        "squashed commits stay graph-visible"
    );
    assert_eq!(wb.lifecycle, BranchLifecycle::Committed);

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&origin);
}

#[tokio::test]
async fn hotfix_and_plain_branches_are_standalone() {
    let dir = temp_dir("standalone");
    init_repo(&dir);

    git(&dir, &["checkout", "-q", "-b", "hotfix/crash"]);
    commit_file(&dir, "fix.txt", "f\n", "hotfix");
    git(&dir, &["checkout", "-q", "main"]);
    git(&dir, &["checkout", "-q", "-b", "experiment"]);
    commit_file(&dir, "x.txt", "x\n", "experiment");
    git(&dir, &["checkout", "-q", "main"]);

    let snap = capture(&dir).await;
    let h = &snap.hierarchy;
    for name in ["hotfix/crash", "experiment"] {
        let n = h.node(name).expect("node");
        assert_eq!(n.role, BranchRole::Standalone, "{name}");
        assert_eq!(n.parent.as_deref(), Some("main"), "{name}");
        assert_eq!(n.ahead_of_parent, 1, "{name}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn unconventional_repo_without_main_degrades_to_flat() {
    let dir = temp_dir("notrunk");
    git(&dir, &["init", "-b", "devel", "-q"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    git(&dir, &["config", "user.name", "wb300 test"]);
    git(&dir, &["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    git(&dir, &["add", "a.txt"]);
    git(&dir, &["commit", "-q", "-m", "init"]);

    let snap = capture(&dir).await;
    let h = &snap.hierarchy;
    assert_eq!(h.trunk, None);
    assert!(!h.approximate, "no trunk is a shape, not an error");
    assert!(h.nodes.iter().all(|n| n.parent.is_none()));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn dirty_task_worktree_reads_uncommitted() {
    let dir = temp_dir("dirty");
    init_repo(&dir);

    let task = sibling(&dir, "task");
    git(
        &dir,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat/dirty-9",
            task.to_str().unwrap(),
        ],
    );
    commit_file(&task, "work.txt", "w\n", "task work");
    std::fs::write(task.join("work.txt"), "edited\n").unwrap(); // dirty on top

    let snap = capture(&dir).await;
    let n = snap.hierarchy.node("feat/dirty-9").expect("node");
    assert_eq!(n.lifecycle, BranchLifecycle::Uncommitted);
    assert!(n.worktree.is_some());

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&task);
}
