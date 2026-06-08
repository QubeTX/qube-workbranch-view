//! Rendering: turns [`AppState`] into a Ratatui frame. UI never runs Git or
//! mutates state — it reads [`AppState`] and draws (handoff §8).

pub mod theme;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};

use crate::app::{AppState, Tab};
use crate::git::{WorktreeRecord, WorktreeStatus};

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
    render_footer(frame, footer);

    if app.show_help {
        render_help(frame, frame.area());
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &AppState) {
    let title = Line::from(vec![
        Span::from(" WB-300 ").bold().fg(theme::ACCENT),
        Span::from(format!("· {} ", app.repo_label())).fg(theme::DIM),
    ]);
    let tabs = Tabs::new(Tab::ALL.iter().map(|t| Line::from(t.title())))
        .block(Block::bordered().title(title))
        .select(app.active_tab.index())
        .highlight_style(Style::new().bold().fg(theme::ACCENT));
    frame.render_widget(tabs, area);
}

fn render_body(frame: &mut Frame, area: Rect, app: &AppState) {
    match app.active_tab {
        Tab::Overview => render_overview(frame, area, app),
        Tab::Worktrees => render_worktrees(frame, area, app),
        Tab::Processes => render_placeholder(
            frame,
            area,
            "Processes",
            "process→worktree mapping arrives in Phase 3.",
        ),
        Tab::Collisions => render_placeholder(
            frame,
            area,
            "Collisions",
            "changed-file overlap detection arrives in Phase 6.",
        ),
        Tab::Cleanup => render_placeholder(
            frame,
            area,
            "Cleanup",
            "safe cleanup candidates arrive in Phase 8.",
        ),
        Tab::Help => render_placeholder(
            frame,
            area,
            "Help",
            "press ? to toggle the help overlay anywhere.",
        ),
    }
}

fn render_overview(frame: &mut Frame, area: Rect, app: &AppState) {
    let s = &app.snapshot;
    let dirty = s
        .worktrees
        .iter()
        .filter(|wt| wt.status.as_ref().is_some_and(|st| !st.clean))
        .count();
    let lines = vec![
        Line::from(vec![
            Span::from("repo  ").fg(theme::DIM),
            Span::from(s.repo.root.display().to_string()).bold(),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::from(s.worktrees.len().to_string())
                .bold()
                .fg(theme::ACCENT),
            Span::from(" worktrees    ").fg(theme::DIM),
            Span::from(dirty.to_string()).bold().fg(theme::DIRTY),
            Span::from(" dirty    ").fg(theme::DIM),
            Span::from(s.local_branch_count().to_string()).bold(),
            Span::from(" local branches    ").fg(theme::DIM),
            Span::from(s.remote_branch_count().to_string()).bold(),
            Span::from(" remote-tracking").fg(theme::DIM),
        ]),
        Line::from(""),
        Line::from(
            "Live lanes (active / changing / stale / archive) arrive in later phases."
                .fg(theme::DIM),
        ),
    ];
    let p = Paragraph::new(lines)
        .block(Block::bordered().title(" Overview "))
        .wrap(Wrap { trim: true });
    frame.render_widget(p, area);
}

fn render_worktrees(frame: &mut Frame, area: Rect, app: &AppState) {
    let worktrees = app.worktrees();
    if worktrees.is_empty() {
        render_placeholder(
            frame,
            area,
            "Worktrees",
            "no worktrees found in this repository.",
        );
        return;
    }

    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).areas(area);

    let current = app.snapshot.current_worktree_index();
    let items: Vec<ListItem> = worktrees
        .iter()
        .enumerate()
        .map(|(i, wt)| worktree_item(wt, Some(i) == current))
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected.min(worktrees.len() - 1)));

    let list = List::new(items)
        .block(Block::bordered().title(format!(" Worktrees ({}) ", worktrees.len())))
        .highlight_style(Style::new().bold().fg(theme::ACCENT))
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, list_area, &mut state);

    render_worktree_details(frame, detail_area, app.selected_worktree());
}

fn worktree_item(wt: &WorktreeRecord, is_current: bool) -> ListItem<'_> {
    let mut spans = vec![Span::from(wt.display_name()).bold()];
    if is_current {
        spans.push(Span::from(" (current)").fg(theme::ACCENT));
    }
    if wt.detached {
        spans.push(Span::from(" detached").fg(Color::Magenta));
    }
    if wt.locked.is_some() {
        spans.push(Span::from(" locked").fg(Color::Blue));
    }
    if wt.prunable.is_some() {
        spans.push(Span::from(" prunable").fg(theme::DIM));
    }
    spans.extend(status_badges(wt.status.as_ref()));
    ListItem::new(Line::from(spans))
}

/// Compact dirty/divergence badges for the worktree list.
fn status_badges(status: Option<&WorktreeStatus>) -> Vec<Span<'static>> {
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

fn render_worktree_details(frame: &mut Frame, area: Rect, wt: Option<&WorktreeRecord>) {
    let block = Block::bordered().title(" Details ");
    let Some(wt) = wt else {
        frame.render_widget(Paragraph::new("Nothing selected.").block(block), area);
        return;
    };

    let mut lines = vec![
        field("name", wt.display_name()),
        field("path", wt.path.clone()),
    ];
    if let Some(head) = wt.short_head() {
        lines.push(field("head", head.to_string()));
    }

    if let Some(s) = &wt.status {
        if let Some(up) = &s.upstream {
            let mut spans = label("upstream");
            spans.push(Span::from(up.clone()));
            if s.upstream_gone {
                spans.push(Span::from("  (gone)").fg(theme::COLLISION));
            }
            lines.push(Line::from(spans));
        }
        lines.push(field(
            "status",
            if s.clean {
                "clean".to_string()
            } else {
                format!(
                    "{} staged · {} unstaged · {} untracked · {} conflicts",
                    s.staged, s.unstaged, s.untracked, s.conflicted
                )
            },
        ));
        lines.push(field(
            "vs up",
            format!(
                "ahead {} · behind {}",
                s.ahead.unwrap_or(0),
                s.behind.unwrap_or(0)
            ),
        ));
    }

    if let Some(reason) = &wt.locked {
        lines.push(field("locked", reason_or_yes(reason)));
    }
    if let Some(reason) = &wt.prunable {
        lines.push(field("prunable", reason_or_yes(reason)));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn label(name: &str) -> Vec<Span<'static>> {
    vec![Span::from(format!("{name:<9}")).fg(theme::DIM)]
}

fn field(name: &str, value: String) -> Line<'static> {
    let mut spans = label(name);
    spans.push(Span::from(value));
    Line::from(spans)
}

fn reason_or_yes(reason: &str) -> String {
    if reason.is_empty() {
        "yes".to_string()
    } else {
        reason.to_string()
    }
}

fn render_placeholder(frame: &mut Frame, area: Rect, title: &str, note: &str) {
    let body = Paragraph::new(format!("{title} — {note}"))
        .block(Block::bordered().title(format!(" {title} ")))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_footer(frame: &mut Frame, area: Rect) {
    let hints = Line::from(vec![
        Span::from(" q ").bold(),
        Span::from("quit  ").fg(theme::DIM),
        Span::from("Tab ").bold(),
        Span::from("tab  ").fg(theme::DIM),
        Span::from("j/k ").bold(),
        Span::from("move  ").fg(theme::DIM),
        Span::from("r ").bold(),
        Span::from("refresh  ").fg(theme::DIM),
        Span::from("1-6 ").bold(),
        Span::from("jump  ").fg(theme::DIM),
        Span::from("? ").bold(),
        Span::from("help").fg(theme::DIM),
    ]);
    frame.render_widget(Paragraph::new(hints), area);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let popup = center(area, Constraint::Percentage(60), Constraint::Length(14));
    let text = vec![
        Line::from("WB-300 — keybindings".bold()),
        Line::from(""),
        Line::from("  q / Esc    quit / close overlay"),
        Line::from("  ?          toggle this help"),
        Line::from("  Tab        next tab"),
        Line::from("  Shift+Tab  previous tab"),
        Line::from("  j / k      move selection down / up"),
        Line::from("  r          refresh the snapshot"),
        Line::from("  1 – 6      jump to a tab"),
        Line::from(""),
        Line::from("Live worktree intelligence arrives phase by phase.".fg(theme::DIM)),
    ];
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(text).block(Block::bordered().title(" Help ")),
        popup,
    );
}

/// Center a region of the given size within `area`.
fn center(area: Rect, horizontal: Constraint, vertical: Constraint) -> Rect {
    let [area] = Layout::horizontal([horizontal])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([vertical]).flex(Flex::Center).areas(area);
    area
}
