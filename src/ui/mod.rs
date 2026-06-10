//! Rendering: turns [`AppState`] into a Ratatui frame. UI never runs Git or
//! mutates state — it reads [`AppState`] and draws (handoff §8).
//!
//! Layout of this module tree: `mod.rs` owns the frame chrome (header, tabs,
//! footer), the per-tab dispatch, and the small colour helpers shared between
//! the per-repo and home views; each tab renders from its own submodule.

mod cleanup;
pub mod home;
mod merge_risk;
mod overlays;
mod overview;
mod processes;
pub mod theme;
mod timeline;
mod util;
mod worktrees;

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
        Constraint::Length(3),
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
        Tab::Overview => overview::render(frame, area, app),
        Tab::Worktrees => worktrees::render(frame, area, app),
        Tab::Processes => processes::render(frame, area, app),
        Tab::Collisions => merge_risk::render(frame, area, app),
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

/// The leading live-activity dot for a worktree row, in its own colour: blue ◆
/// while editing, ✚ created, ⌫ removed, dim · when idle. Milestones recolor the
/// whole row instead (see [`milestone_color`]), so the dot just dims for those.
pub(crate) fn activity_dot(transition: Option<TransitionKind>) -> Span<'static> {
    match transition {
        Some(TransitionKind::Activity) => Span::from("◆ ").fg(theme::ACTIVITY),
        Some(TransitionKind::Created) => Span::from("✚ ").fg(Color::Cyan),
        Some(TransitionKind::Deleted) => Span::from("⌫ ").fg(theme::COLLISION),
        _ => Span::from("· ").fg(theme::DIM),
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

/// Compact dirty/divergence badges for a worktree row. Shared by the worktree
/// list and (later) any branch-keyed row that needs the same vocabulary.
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
        Span::from("Tab ").bold(),
        Span::from("tab  ").fg(theme::DIM),
        Span::from("j/k ").bold(),
        Span::from("move  ").fg(theme::DIM),
        Span::from("r ").bold(),
        Span::from("refresh  ").fg(theme::DIM),
        Span::from("f ").bold(),
        Span::from("fetch  ").fg(theme::DIM),
        Span::from("/ ").bold(),
        Span::from("find  ").fg(theme::DIM),
        Span::from(": ").bold(),
        Span::from("cmd  ").fg(theme::DIM),
        Span::from("x ").bold(),
        Span::from("remove  ").fg(theme::DIM),
        Span::from("K ").bold(),
        Span::from("kill  ").fg(theme::DIM),
        Span::from("1-7 ").bold(),
        Span::from("jump  ").fg(theme::DIM),
        Span::from("? ").bold(),
        Span::from("help").fg(theme::DIM),
    ]);
    frame.render_widget(Paragraph::new(hints), area);
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::app::TransitionKind;
    use crate::git::{RepoIdentity, RepoSnapshot, WorktreeRecord};
    use crate::process::ProcessSnapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn snapshot(worktrees: Vec<WorktreeRecord>) -> RepoSnapshot {
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
            collisions: Vec::new(),
            processes: ProcessSnapshot::default(),
        }
    }

    /// A worktree on branch `feat`; `clean` toggles a single staged change so a
    /// dirty case has no yellow *badge* (staged is blue) — only the name carries
    /// the persistent-uncommitted yellow.
    fn wt(path: &str, clean: bool) -> WorktreeRecord {
        WorktreeRecord {
            path: path.into(),
            branch: Some("refs/heads/feat".into()),
            status: Some(WorktreeStatus {
                clean,
                staged: if clean { 0 } else { 1 },
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn app_on_worktrees(worktrees: Vec<WorktreeRecord>) -> AppState {
        let mut app = AppState::new(snapshot(worktrees));
        app.active_tab = Tab::Worktrees;
        app
    }

    fn render_buffer(app: &AppState) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(120, 12)).unwrap();
        terminal.draw(|f| render(f, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn has_fg(buf: &Buffer, color: Color) -> bool {
        buf.content().iter().any(|c| c.fg == color)
    }

    fn has_symbol(buf: &Buffer, needle: &str) -> bool {
        buf.content().iter().any(|c| c.symbol() == needle)
    }

    #[test]
    fn dirty_worktree_name_is_held_yellow() {
        let app = app_on_worktrees(vec![wt("/repo", false)]);
        assert!(has_fg(&render_buffer(&app), theme::DIRTY));
    }

    #[test]
    fn clean_worktree_shows_no_uncommitted_yellow() {
        let app = app_on_worktrees(vec![wt("/repo", true)]);
        assert!(!has_fg(&render_buffer(&app), theme::DIRTY));
    }

    #[test]
    fn activity_on_a_clean_worktree_shows_a_blue_dot() {
        // Clean + editing (the brief window before git sees the change) → blue ◆.
        let mut app = app_on_worktrees(vec![wt("/repo", true)]);
        app.note_activity(&[std::path::PathBuf::from("/repo/src/x.rs")]);
        let buf = render_buffer(&app);
        assert!(has_symbol(&buf, "◆"), "expected the editing dot");
        assert!(has_fg(&buf, theme::ACTIVITY), "blue while clean");
    }

    #[test]
    fn commit_flash_recolors_the_row_magenta() {
        let mut app = app_on_worktrees(vec![wt("/repo", true)]);
        app.transitions
            .note("/repo".into(), TransitionKind::Committed);
        assert!(has_fg(&render_buffer(&app), theme::COMMITTED));
    }

    #[test]
    fn push_flash_recolors_the_row_green() {
        // Dirty (so no "clean" green badge) and live=Static (so no green "● live"):
        // the only green in the pane can come from the push recolor.
        let mut app = app_on_worktrees(vec![wt("/repo", false)]);
        app.transitions.note("/repo".into(), TransitionKind::Pushed);
        assert!(has_fg(&render_buffer(&app), theme::CLEAN));
    }

    #[test]
    fn every_tab_renders_without_panicking() {
        let mut app = app_on_worktrees(vec![wt("/repo", false)]);
        app.set_status("✓ terminated foo (pid 1)".into(), false);
        app.transitions
            .note("/repo".into(), TransitionKind::Committed);
        for tab in Tab::ALL {
            app.active_tab = tab;
            let _ = render_buffer(&app); // every tab must render, not panic
        }
    }

    #[test]
    fn editing_an_uncommitted_worktree_is_all_yellow() {
        // Yellow line AND a yellow editing dot — no blue once uncommitted.
        let mut app = app_on_worktrees(vec![wt("/repo", false)]);
        app.note_activity(&[std::path::PathBuf::from("/repo/src/x.rs")]);
        let buf = render_buffer(&app);
        assert!(has_symbol(&buf, "◆"), "editing dot present");
        assert!(has_fg(&buf, theme::DIRTY), "all yellow");
        assert!(
            !has_fg(&buf, theme::ACTIVITY),
            "dot is not blue when uncommitted"
        );
    }
}
