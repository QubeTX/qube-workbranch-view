//! Rendering: turns [`AppState`] into a Ratatui frame. UI never runs Git or
//! mutates state — it reads [`AppState`] and draws (handoff §8).
//!
//! Layout of this module tree: `mod.rs` owns the frame chrome (header with
//! tab bar + summary strip, footer), the per-tab dispatch, and the small
//! colour helpers shared between the per-repo and home views; each tab
//! renders from its own submodule. The Branches tree is the primary view.

mod branches;
mod cleanup;
pub mod home;
mod merge_risk;
mod overlays;
mod processes;
pub mod theme;
mod timeline;
mod tree;
mod util;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Paragraph, Tabs},
};

use crate::app::{AppState, LiveStatus, Overlay, Tab, TransitionKind};
use crate::git::WorktreeStatus;
use util::human_dur;

/// Render the whole UI for the current frame.
pub fn render(frame: &mut Frame, app: &AppState) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    render_header(frame, header, app);
    render_body(frame, body, app);
    render_footer(frame, footer, app);

    if app.show_help {
        overlays::render_help(frame, frame.area());
    }

    match &app.overlay {
        Overlay::Search { query } => overlays::render_search_bar(frame, frame.area(), query),
        Overlay::Confirm(confirm) => overlays::render_confirm(frame, frame.area(), confirm),
        Overlay::Palette(palette) => overlays::render_palette(frame, frame.area(), palette),
        Overlay::None => {}
    }
}

/// Header: bordered block with the tab bar on the first inner line and the
/// counts/freshness summary strip on the second (the old Overview tab's
/// essentials, always on screen).
fn render_header(frame: &mut Frame, area: Rect, app: &AppState) {
    let mut head = vec![
        Span::from(" WB-300 ").bold().fg(theme::ACCENT),
        Span::from(format!("· {} ", app.repo_label())).fg(theme::DIM),
        live_indicator(app.live),
    ];
    if app.stale {
        // A capture failed — say so rather than letting "● live" imply fresh data.
        head.push(Span::from("⚠ stale ").bold().fg(theme::COLLISION));
    }
    head.push(remote_indicator(app));
    let title = Line::from(head);
    let tabs = Tabs::new(Tab::ALL.iter().map(|t| Line::from(t.title())))
        .block(Block::bordered().title(title))
        .select(app.active_tab.index())
        .highlight_style(Style::new().bold().fg(theme::ACCENT));
    frame.render_widget(tabs, area);

    // Second inner line: the summary strip.
    if area.height >= 4 {
        let strip = Rect {
            x: area.x + 1,
            y: area.y + 2,
            width: area.width.saturating_sub(2),
            height: 1,
        };
        frame.render_widget(Paragraph::new(summary_line(app)), strip);
    }
}

/// The always-visible counts + freshness strip.
fn summary_line(app: &AppState) -> Line<'static> {
    let s = &app.snapshot;
    let uncommitted = s
        .worktrees
        .iter()
        .filter(|wt| wt.status.as_ref().is_some_and(|st| !st.clean))
        .count();
    let agents = (0..s.worktrees.len())
        .filter(|&i| s.processes.worktree_is_active(i))
        .count();
    let active = s.hierarchy.active_count();
    let all = s.hierarchy.nodes.len();
    let mut spans = vec![
        Span::from(format!(" {all}")).bold(),
        Span::from(" branches · ").fg(theme::DIM),
        Span::from(s.worktrees.len().to_string()).bold(),
        Span::from(" worktrees · ").fg(theme::DIM),
        Span::from(agents.to_string()).bold().fg(if agents > 0 {
            theme::CLEAN
        } else {
            theme::DIM
        }),
        Span::from(" agents · ").fg(theme::DIM),
        Span::from(uncommitted.to_string())
            .bold()
            .fg(if uncommitted > 0 {
                theme::DIRTY
            } else {
                theme::DIM
            }),
        Span::from(" uncommitted · ").fg(theme::DIM),
        Span::from(s.collisions.len().to_string())
            .bold()
            .fg(if s.collisions.is_empty() {
                theme::DIM
            } else {
                theme::COLLISION
            }),
        Span::from(" risk · ").fg(theme::DIM),
    ];
    if app.tree.show_all {
        spans.push(Span::from(format!("all ({all})")).bold().fg(theme::ACCENT));
        spans.push(Span::from(format!(" / active ({active})")).fg(theme::DIM));
    } else {
        spans.push(
            Span::from(format!("active ({active})"))
                .bold()
                .fg(theme::ACCENT),
        );
        spans.push(Span::from(format!(" / all ({all})")).fg(theme::DIM));
    }
    if s.hierarchy.approximate {
        spans.push(Span::from("  ~hierarchy approximate").fg(theme::DIM));
    }
    spans.push(Span::from("   ").fg(theme::DIM));
    spans.push(freshness(app));
    Line::from(spans)
}

/// How live the data is and how long since the last successful capture — so
/// "live" can never imply fresh data when a capture has been failing.
fn freshness(app: &AppState) -> Span<'static> {
    let ago = app.last_updated.map_or_else(
        || "not yet".to_string(),
        |t| {
            format!(
                "{} ago",
                human_dur(crate::storage::events::epoch_secs().saturating_sub(t))
            )
        },
    );
    if app.stale {
        return Span::from(format!("⚠ stale — last good capture {ago}"))
            .bold()
            .fg(theme::COLLISION);
    }
    match app.live {
        LiveStatus::Live => Span::from(format!("updated {ago}")).fg(theme::DIM),
        LiveStatus::PollOnly => Span::from(format!("poll-only · updated {ago}")).fg(Color::Yellow),
        LiveStatus::Static => {
            Span::from(format!("static · updated {ago} (r to refresh)")).fg(theme::DIM)
        }
    }
}

fn live_indicator(status: LiveStatus) -> Span<'static> {
    match status {
        LiveStatus::Live => Span::from("● live ").fg(theme::CLEAN),
        LiveStatus::PollOnly => Span::from("◐ poll-only ").fg(Color::Yellow),
        LiveStatus::Static => Span::from("○ static ").fg(theme::DIM),
    }
}

fn remote_indicator(app: &AppState) -> Span<'static> {
    if app.fetching {
        return Span::from("⟳ fetching… ").fg(Color::Yellow);
    }
    match app.remote_checked {
        Some(epoch) => {
            let age = crate::storage::events::epoch_secs().saturating_sub(epoch);
            Span::from(format!("remote {} ago ", human_dur(age))).fg(theme::DIM)
        }
        None => Span::from("remote: not checked ").fg(theme::DIM),
    }
}

fn render_body(frame: &mut Frame, area: Rect, app: &AppState) {
    match app.active_tab {
        Tab::Branches => branches::render(frame, area, app),
        Tab::Processes => processes::render(frame, area, app),
        Tab::MergeRisk => merge_risk::render(frame, area, app),
        Tab::Cleanup => cleanup::render(frame, area, app),
        Tab::Timeline => timeline::render(frame, area, app),
        Tab::Help => util::render_placeholder(
            frame,
            area,
            "Help",
            "press ? to toggle the help overlay anywhere.",
        ),
    }
}

/// The persistent colour the row BODY takes from worktree state: yellow while it
/// has uncommitted work, dim when its status is unknown (a failed `git status`
/// must never read as clean), none (standard) when known-clean.
pub(crate) fn uncommitted_color(status: Option<&WorktreeStatus>) -> Option<Color> {
    match status {
        Some(s) if !s.clean => Some(theme::DIRTY),
        None => Some(theme::DIM),
        Some(_) => None,
    }
}

/// The solid colour a milestone flash recolors the whole row with, if any
/// (magenta = just committed, green = just pushed). Shared with the home view.
pub(crate) fn milestone_color(kind: Option<TransitionKind>) -> Option<Color> {
    match kind {
        Some(TransitionKind::Committed) => Some(theme::COMMITTED),
        Some(TransitionKind::Pushed) => Some(theme::CLEAN),
        _ => None,
    }
}

/// Compact dirty/divergence badges for a worktree row. Shared by the detached
/// rows in the tree and the home view.
pub(crate) fn status_badges(status: Option<&WorktreeStatus>) -> Vec<Span<'static>> {
    let Some(s) = status else {
        return vec![Span::from("  …").fg(theme::DIM)];
    };
    let mut v = Vec::new();
    if s.clean && !s.diverged() {
        v.push(Span::from("  clean").fg(theme::CLEAN));
    }
    if s.unstaged > 0 {
        v.push(Span::from(format!("  {}~", s.unstaged)).fg(theme::DIRTY));
    }
    if s.staged > 0 {
        v.push(Span::from(format!(" {}+", s.staged)).fg(Color::Blue));
    }
    if s.untracked > 0 {
        v.push(Span::from(format!(" {}?", s.untracked)).fg(theme::DIM));
    }
    if s.conflicted > 0 {
        v.push(Span::from(format!(" {}!", s.conflicted)).fg(theme::COLLISION));
    }
    if s.ahead.unwrap_or(0) > 0 {
        v.push(Span::from(format!("  ↑{}", s.ahead.unwrap_or(0))).fg(theme::CLEAN));
    }
    if s.behind.unwrap_or(0) > 0 {
        v.push(Span::from(format!(" ↓{}", s.behind.unwrap_or(0))).fg(Color::Yellow));
    }
    if s.upstream_gone {
        v.push(Span::from("  upstream gone").fg(theme::COLLISION));
    }
    v
}

fn render_footer(frame: &mut Frame, area: Rect, app: &AppState) {
    // A transient action result (e.g. a kill outcome) takes over the footer so a
    // requested destructive action is always acknowledged on screen.
    if let Some((text, is_error)) = app.status() {
        let color = if is_error {
            theme::COLLISION
        } else {
            theme::CLEAN
        };
        let line = Line::from(Span::from(format!(" {text}")).bold().fg(color));
        frame.render_widget(Paragraph::new(line), area);
        return;
    }
    let hints = Line::from(vec![
        Span::from(" q ").bold(),
        Span::from("quit  ").fg(theme::DIM),
        Span::from("j/k ").bold(),
        Span::from("move  ").fg(theme::DIM),
        Span::from("h/l ").bold(),
        Span::from("fold  ").fg(theme::DIM),
        Span::from("a ").bold(),
        Span::from("active/all  ").fg(theme::DIM),
        Span::from("r ").bold(),
        Span::from("refresh  ").fg(theme::DIM),
        Span::from("f ").bold(),
        Span::from("fetch  ").fg(theme::DIM),
        Span::from("/ ").bold(),
        Span::from("find  ").fg(theme::DIM),
        Span::from(": ").bold(),
        Span::from("cmd  ").fg(theme::DIM),
        Span::from("x ").bold(),
        Span::from("rm worktree  ").fg(theme::DIM),
        Span::from("K ").bold(),
        Span::from("kill  ").fg(theme::DIM),
        Span::from("1-6 ").bold(),
        Span::from("jump  ").fg(theme::DIM),
        Span::from("? ").bold(),
        Span::from("help").fg(theme::DIM),
    ]);
    frame.render_widget(Paragraph::new(hints), area);
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::app::state::wt_flash_key;
    use crate::git::{
        BranchHierarchy, BranchLifecycle, BranchNode, BranchRole, ChangeKind, FileChange,
        RepoIdentity, RepoSnapshot, WorktreeRecord,
    };
    use crate::process::ProcessSnapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn snapshot(worktrees: Vec<WorktreeRecord>, nodes: Vec<BranchNode>) -> RepoSnapshot {
        RepoSnapshot {
            repo: RepoIdentity {
                start_dir: "/repo".into(),
                root: "/repo".into(),
                git_dir: "/repo/.git".into(),
                common_git_dir: "/repo/.git".into(),
                is_worktree: false,
            },
            base: None,
            worktrees,
            branches: Vec::new(),
            hierarchy: BranchHierarchy {
                trunk: Some("main".to_string()),
                nodes,
                approximate: false,
            },
            collisions: Vec::new(),
            processes: ProcessSnapshot::default(),
            captured_at: 0,
        }
    }

    fn node(name: &str, lifecycle: BranchLifecycle, worktree: Option<usize>) -> BranchNode {
        BranchNode {
            name: name.to_string(),
            oid: format!("oid-{name}"),
            role: BranchRole::Task,
            parent: Some("main".to_string()),
            ahead_of_parent: 1,
            behind_parent: Some(0),
            merged_into_parent: false,
            upstream: None,
            ahead: None,
            behind: None,
            upstream_gone: false,
            committer_date: None,
            lifecycle,
            worktree,
        }
    }

    /// One task branch checked out in a worktree; `clean` toggles dirt. The
    /// clean case lists an Added file (green chip) so any DIRTY yellow in the
    /// buffer can only come from an uncommitted row.
    fn app_with_branch(clean: bool) -> AppState {
        let wt = WorktreeRecord {
            path: "/repo-feat".into(),
            branch: Some("refs/heads/feat/x".into()),
            status: Some(WorktreeStatus {
                clean,
                staged: if clean { 0 } else { 1 },
                ..Default::default()
            }),
            touched: vec![FileChange {
                path: "src/lib.rs".into(),
                kind: if clean {
                    ChangeKind::Added
                } else {
                    ChangeKind::Modified
                },
            }],
            ..Default::default()
        };
        let lifecycle = if clean {
            BranchLifecycle::Committed
        } else {
            BranchLifecycle::Uncommitted
        };
        let snap = snapshot(vec![wt], vec![node("feat/x", lifecycle, Some(0))]);
        AppState::new(snap)
    }

    fn render_buffer(app: &AppState) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(120, 14)).unwrap();
        terminal.draw(|f| render(f, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn has_fg(buf: &Buffer, color: Color) -> bool {
        buf.content().iter().any(|c| c.fg == color)
    }

    fn has_symbol(buf: &Buffer, needle: &str) -> bool {
        buf.content().iter().any(|c| c.symbol() == needle)
    }

    fn buffer_text(buf: &Buffer) -> String {
        buf.content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn dirty_branch_row_is_held_yellow() {
        let app = app_with_branch(false);
        assert!(has_fg(&render_buffer(&app), theme::DIRTY));
    }

    #[test]
    fn clean_branch_shows_no_uncommitted_yellow() {
        let app = app_with_branch(true);
        assert!(!has_fg(&render_buffer(&app), theme::DIRTY));
    }

    #[test]
    fn tree_draws_guides_worktree_path_and_files() {
        let app = app_with_branch(true);
        let buf = render_buffer(&app);
        let text = buffer_text(&buf);
        assert!(has_symbol(&buf, "▾"), "expanded repo row arrow");
        assert!(text.contains("└─"), "connector drawn");
        assert!(has_symbol(&buf, "⌂"), "worktree path marker");
        assert!(text.contains("src/lib.rs"), "file row visible");
        assert!(text.contains("added"), "change kind visible");
    }

    #[test]
    fn commit_flash_recolors_the_branch_row_magenta() {
        let mut app = app_with_branch(true);
        let rkey = crate::home::snapshot::repo_key(&app.snapshot);
        let key = wt_flash_key(&rkey, &app.snapshot.worktrees[0]);
        app.transitions.note(key, TransitionKind::Committed);
        assert!(has_fg(&render_buffer(&app), theme::COMMITTED));
    }

    #[test]
    fn push_flash_recolors_the_branch_row_green() {
        // Dirty (no green "clean"/pushed text) and Static (no green "● live"):
        // green can only come from the push recolor.
        let mut app = app_with_branch(false);
        let rkey = crate::home::snapshot::repo_key(&app.snapshot);
        let key = wt_flash_key(&rkey, &app.snapshot.worktrees[0]);
        app.transitions.note(key, TransitionKind::Pushed);
        assert!(has_fg(&render_buffer(&app), theme::CLEAN));
    }

    #[test]
    fn file_activity_pulses_a_blue_marker() {
        let mut app = app_with_branch(false);
        app.note_activity(&[std::path::PathBuf::from("/repo-feat/src/lib.rs")]);
        let buf = render_buffer(&app);
        assert!(has_symbol(&buf, "◆"), "editing marker present");
        assert!(has_fg(&buf, theme::ACTIVITY), "blue pulse");
    }

    #[test]
    fn dimmed_inactive_branch_under_show_all() {
        let mut merged = node("feat/old", BranchLifecycle::Merged, None);
        merged.ahead_of_parent = 0;
        merged.merged_into_parent = true;
        let snap = snapshot(Vec::new(), vec![merged]);
        let mut app = AppState::new(snap);
        // Hidden by default…
        assert!(!buffer_text(&render_buffer(&app)).contains("feat/old"));
        // …visible (dim) with show_all.
        app.apply(crate::app::Action::ToggleShowAll);
        assert!(buffer_text(&render_buffer(&app)).contains("feat/old"));
    }

    #[test]
    fn every_tab_renders_without_panicking() {
        let mut app = app_with_branch(false);
        app.set_status("✓ terminated foo (pid 1)".into(), false);
        for tab in Tab::ALL {
            app.active_tab = tab;
            let _ = render_buffer(&app); // every tab must render, not panic
        }
    }

    #[test]
    fn narrow_terminal_renders_without_panicking() {
        let app = app_with_branch(false);
        let mut terminal = Terminal::new(TestBackend::new(48, 10)).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[test]
    fn header_summary_shows_counts_and_scope() {
        let app = app_with_branch(false);
        let text = buffer_text(&render_buffer(&app));
        assert!(text.contains("branches"));
        assert!(text.contains("uncommitted"));
        assert!(text.contains("active ("));
    }
}
