//! `wb300 help` — the full manual, in the terminal.
//!
//! Unlike `--help` (clap's flag/usage summary), this prints the complete
//! documentation: the mental model, every view, every key, every glyph, the
//! notification rules, the agent JSON contract, and troubleshooting. When
//! stdout is a terminal the text is piped through a pager (`$PAGER`, `less`,
//! or `more`); otherwise it prints plainly so it can be grepped or redirected.

use std::io::Write;

/// ANSI styling, switchable off for `--no-color` / non-TTY output.
struct Style {
    on: bool,
}

impl Style {
    /// Section heading.
    fn h(&self, s: &str) -> String {
        if self.on {
            format!("\x1b[1;36m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }

    /// Bold (keys, commands, terms).
    fn b(&self, s: &str) -> String {
        if self.on {
            format!("\x1b[1m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
}

/// Print the manual (paged when appropriate). Returns a process exit code.
pub fn run(use_colors: bool) -> i32 {
    use std::io::IsTerminal;
    let tty = std::io::stdout().is_terminal();
    if tty {
        // Windows pagers (`more.com`) can't be trusted with ANSI on classic
        // conhost — page plain text there; Unix pagers get colors (less -R).
        let paged_text = manual(&Style {
            on: use_colors && !cfg!(windows),
        });
        if page(&paged_text) {
            return 0;
        }
    }
    let text = manual(&Style {
        on: use_colors && tty,
    });
    print!("{text}");
    0
}

/// Try to display `text` through a pager. Returns false to fall back to print.
fn page(text: &str) -> bool {
    let candidates: Vec<(String, Vec<&str>)> = if cfg!(windows) {
        // std's PATH search only appends `.exe`, and Windows' pager is
        // `more.com` — name it explicitly (keep a bare `more` fallback).
        vec![
            ("more.com".to_string(), vec![]),
            ("more".to_string(), vec![]),
        ]
    } else {
        let mut v = Vec::new();
        if let Ok(pager) = std::env::var("PAGER")
            && !pager.trim().is_empty()
        {
            v.push((pager, vec![]));
        }
        v.push(("less".to_string(), vec!["-R"]));
        v.push(("more".to_string(), vec![]));
        v
    };
    for (cmd, args) in candidates {
        let child = std::process::Command::new(&cmd)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .spawn();
        let Ok(mut child) = child else { continue };
        if let Some(stdin) = child.stdin.as_mut() {
            // The pager may exit early (user quits) — a broken pipe is fine.
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
        return true;
    }
    false
}

/// The manual text. One source of truth for what everything means. Built
/// line by line (string-continuation literals would strip the indentation
/// that the tree diagrams and key tables depend on).
fn manual(st: &Style) -> String {
    let mut out = String::new();
    let mut sec = |title: &str, body: Vec<String>| {
        out.push_str(&st.h(title));
        out.push('\n');
        for line in body {
            out.push_str(&line);
            out.push('\n');
        }
        out.push('\n');
    };

    sec(
        "WB-300 — MANUAL",
        vec![
            String::new(),
            "wb300 is a live terminal control tower for parallel coding-agent work in".into(),
            "Git. Run it inside a repository for that repo's view, or anywhere else".into(),
            format!(
                "(or with {}) for the machine-wide view of every active repository.",
                st.b("--home")
            ),
        ],
    );

    sec(
        "THE MODEL",
        vec![
            String::new(),
            "Git's rule: a branch can be checked out in at most ONE worktree. So the".into(),
            "real shape of parallel agent work is a branch tree, and that tree is".into(),
            "exactly what wb300 shows:".into(),
            String::new(),
            "    repo".into(),
            format!(
                "    ├─ {}                     the integration trunk",
                st.b("main")
            ),
            format!(
                "    └─ {}     a daily workbranch (lives ≤ 1 day)",
                st.b("emmett/wb-2026-06-10")
            ),
            format!(
                "       ├─ {}        a task branch — its own worktree, one agent",
                st.b("feat/csv-export-1")
            ),
            format!("       └─ {}", st.b("fix/login-2")),
            String::new(),
            format!(
                "{}. The worktree is just the",
                st.b("One agent = one branch = one worktree")
            ),
            "folder on disk where a branch is checked out; the agent is the process".into(),
            "running inside that folder. Branches with no worktree are plain refs and".into(),
            "appear dimmed (or are hidden in the default active-only view).".into(),
        ],
    );

    sec(
        "VIEWS",
        vec![
            String::new(),
            format!(
                "{}    The main view: the branch hierarchy. Each branch row shows its",
                st.b("Branches")
            ),
            "            agent, lifecycle stage, and ⌂ worktree path, and expands into".into(),
            "            the files currently changed on it. The header strip keeps the".into(),
            "            totals (branches / worktrees / agents / uncommitted / risk)".into(),
            "            and data freshness always visible.".into(),
            format!(
                "{}   Every OS process mapped into a worktree (agents highlighted).",
                st.b("Processes")
            ),
            format!(
                "{}  Files changed on 2+ branches — what will conflict at merge",
                st.b("Merge Risk")
            ),
            "            time, ranked by how risky the file is (lockfiles and".into(),
            "            migrations are critical; docs are low).".into(),
            format!(
                "{}     Which worktrees are safe to remove (merged, pushed, no agent).",
                st.b("Cleanup")
            ),
            format!(
                "{}    The recorded history: branches committed / pushed / merged,",
                st.b("Timeline")
            ),
            "            worktrees created / removed, conflict risks discovered.".into(),
            String::new(),
            "The machine-wide home view is the same tree with one repo node per".into(),
            format!(
                "active repository. Press {} on any row to open that repo's full view",
                st.b("Enter")
            ),
            format!(
                "(the destructive actions live there); {} returns home.",
                st.b("q")
            ),
        ],
    );

    sec(
        "LIFECYCLE STAGES",
        vec![
            String::new(),
            "Each branch sits at one stage of the pipeline:".into(),
            String::new(),
            format!(
                "    {}      its worktree's files are being written right now",
                st.b("editing")
            ),
            format!(
                "    {}  its worktree has uncommitted changes (row holds yellow)",
                st.b("uncommitted")
            ),
            format!(
                "    {}    clean, with work not yet on the remote (↑N badge)",
                st.b("committed")
            ),
            format!(
                "    {}       clean and fully on a live remote (✓)",
                st.b("pushed")
            ),
            format!(
                "    {}       contained in its parent branch — done",
                st.b("merged")
            ),
            format!(
                "    {}        just cut from its parent, no work yet",
                st.b("fresh")
            ),
            String::new(),
            "Note: a squash-merged branch keeps reading \"committed\" until it is".into(),
            "deleted — squash merges are invisible in the commit graph, by design.".into(),
        ],
    );

    sec(
        "INDICATORS",
        vec![
            String::new(),
            "    ◆            a file is being saved right now (blue pulse)".into(),
            "    yellow row   the branch has uncommitted changes".into(),
            "    magenta row  flash: a commit just landed".into(),
            "    green row    flash: work just reached the remote".into(),
            "    ✚ / ⌫        a worktree just appeared / was just removed".into(),
            "    ⌂ path       where the branch's worktree lives on disk".into(),
            "    ● claude     the agent process attached to the branch's worktree".into(),
            "    ↑N / ↓N      commits ahead / behind the upstream".into(),
            "    ⇣N vs parent the parent moved on — the branch needs a rebase".into(),
            "    ⚠ N          merge-conflict risks involving this branch".into(),
            "    ~ + - ? !    per-file change kind: modified, added, deleted,".into(),
            "                 untracked, conflicted".into(),
            "    dimmed       inactive: no worktree, no agent, no unmerged work".into(),
            "    (detached)   a worktree with no branch checked out".into(),
        ],
    );

    sec(
        "KEYS",
        vec![
            String::new(),
            format!(
                "    {}             quit (Esc backs out of overlays/filters first)",
                st.b("q")
            ),
            format!("    {} / {}     switch tab", st.b("Tab"), st.b("1-6")),
            format!("    {}         move selection", st.b("j / k")),
            format!(
                "    {}         expand / collapse the selected node (vim-style)",
                st.b("l / h")
            ),
            format!(
                "    {}         toggle expansion (home view: open the repo)",
                st.b("Enter")
            ),
            format!(
                "    {}             show active branches only ⇄ all branches",
                st.b("a")
            ),
            format!("    {}             refresh now", st.b("r")),
            format!(
                "    {}             fetch from remotes (never runs automatically)",
                st.b("f")
            ),
            format!("    {}             filter branches by name", st.b("/")),
            format!("    {}             command palette", st.b(":")),
            format!(
                "    {}             remove the selected branch's WORKTREE (the branch",
                st.b("x")
            ),
            "                  and its commits are kept; dirty worktrees get a".into(),
            "                  rescue snapshot first; type the branch name to confirm".into(),
            format!(
                "    {}             kill the attached agent / selected process (type",
                st.b("K")
            ),
            "                  the PID to confirm)".into(),
            format!(
                "    {}             prune stale worktree bookkeeping",
                st.b("p")
            ),
            format!("    {}             in-app key overlay", st.b("?")),
        ],
    );

    sec(
        "NOTIFICATIONS",
        vec![
            String::new(),
            "wb300 sends native OS notifications for exactly three things:".into(),
            String::new(),
            "    · a branch got new commits".into(),
            "    · a branch's work reached its remote".into(),
            "    · two branches started changing the same file".into(),
            String::new(),
            "Never for anything else (no agent-exit or idle nagging). Bursts".into(),
            "coalesce (\"3 branches pushed\") and repeats are suppressed for 30s per".into(),
            format!(
                "branch. Disable per run with {}, or permanently in the config:",
                st.b("--no-notify")
            ),
            String::new(),
            format!("    {}", st.b("~/.config/wb300/config.toml")),
            "    (Windows: %LOCALAPPDATA%\\wb300\\config.toml)".into(),
            String::new(),
            "    [notifications]".into(),
            "    enabled = true".into(),
            "    commit = true".into(),
            "    push = true".into(),
            "    conflict_risk = true".into(),
            "    cooldown_secs = 30".into(),
            String::new(),
            "On Windows, toasts identify as WB-300 via a per-user registry entry;".into(),
            "if that registration fails they may display as \"Windows PowerShell\".".into(),
        ],
    );

    sec(
        "COMMANDS",
        vec![
            String::new(),
            format!(
                "    {}                 launch the TUI (repo view inside a repo, else home)",
                st.b("wb300")
            ),
            format!("    {}            this manual", st.b("wb300 help")),
            format!(
                "    {}           the full state as JSON (schema wb300.agent.v2):",
                st.b("wb300 agent")
            ),
            "                    the branch hierarchy with roles, parents, lifecycle,".into(),
            "                    agents, and changed files — built for orchestrating".into(),
            "                    agents and scripts. Honors --repo and --home.".into(),
            format!(
                "    {}          self-update to the latest release",
                st.b("wb300 update")
            ),
            format!(
                "    {}       remove wb300 (detects the install method; --purge",
                st.b("wb300 uninstall")
            ),
            "                    also removes state, config, and registry entries)".into(),
            String::new(),
            format!(
                "Useful flags: {} <path> · {} · {} · {} · {}",
                st.b("--repo"),
                st.b("--home"),
                st.b("--no-live"),
                st.b("--no-color"),
                st.b("--no-notify")
            ),
        ],
    );

    sec(
        "FILES",
        vec![
            String::new(),
            "    <repo>/.git/wb300/        per-repo event log (events.jsonl), rescue".into(),
            "                              snapshots, and the per-repo debug log".into(),
            "    %LOCALAPPDATA%\\wb300\\     machine-wide log + config.toml (Windows)".into(),
            "    ~/.local/state/wb300/     machine-wide log (Linux/macOS)".into(),
            "    ~/.config/wb300/          config.toml (Linux/macOS)".into(),
        ],
    );

    sec(
        "TROUBLESHOOTING",
        vec![
            String::new(),
            format!(
                "    {}     the last Git capture failed; the board shows the previous",
                st.b("⚠ stale")
            ),
            "              good data. Check the log in the state dir.".into(),
            format!(
                "    {}  the filesystem watcher could not start; wb300 falls back",
                st.b("◐ poll-only")
            ),
            "              to polling. Everything works, slightly less instantly.".into(),
            format!(
                "    {}  branch parentage could not be fully derived (very large",
                st.b("~approximate")
            ),
            "              off-trunk history or an old git); parents degrade to trunk.".into(),
            "    No toasts on Windows: check Focus Assist / Do Not Disturb; toasts".into(),
            "    land in the Action Center even when suppressed.".into(),
        ],
    );

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_covers_the_core_topics_without_ansi_when_plain() {
        let text = manual(&Style { on: false });
        for needle in [
            "THE MODEL",
            "one branch",
            "LIFECYCLE",
            "uncommitted",
            "KEYS",
            "NOTIFICATIONS",
            "wb300 agent",
            "wb300 uninstall",
            "TROUBLESHOOTING",
        ] {
            assert!(text.contains(needle), "manual must mention {needle}");
        }
        assert!(!text.contains('\x1b'), "no ANSI when styling is off");
    }

    #[test]
    fn diagram_and_tables_keep_their_indentation() {
        let text = manual(&Style { on: false });
        assert!(text.contains("    repo\n    ├─ main"), "tree indent intact");
        assert!(text.contains("    ◆ "), "legend indent intact");
    }

    #[test]
    fn styled_manual_carries_ansi() {
        let text = manual(&Style { on: true });
        assert!(text.contains("\x1b[1;36m"));
    }
}
