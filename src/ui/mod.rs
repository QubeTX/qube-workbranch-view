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
use crate::git::WorktreeRecord;

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
    frame.render_stateful_widget(list, area, &mut state);
}

fn worktree_item(wt: &WorktreeRecord, is_current: bool) -> ListItem<'_> {
    let mut spans = vec![Span::from(wt.display_name()).bold()];
    if is_current {
        spans.push(Span::from(" (current)").fg(theme::ACCENT));
    }
    spans.push(Span::from(format!("   {}", wt.path)).fg(theme::DIM));
    if wt.detached {
        spans.push(Span::from(" detached").fg(Color::Magenta));
    }
    if wt.locked.is_some() {
        spans.push(Span::from(" locked").fg(Color::Blue));
    }
    if wt.prunable.is_some() {
        spans.push(Span::from(" prunable").fg(theme::DIM));
    }
    ListItem::new(Line::from(spans))
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
        Span::from("1-6 ").bold(),
        Span::from("jump  ").fg(theme::DIM),
        Span::from("? ").bold(),
        Span::from("help").fg(theme::DIM),
    ]);
    frame.render_widget(Paragraph::new(hints), area);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let popup = center(area, Constraint::Percentage(60), Constraint::Length(13));
    let text = vec![
        Line::from("WB-300 — keybindings".bold()),
        Line::from(""),
        Line::from("  q / Esc    quit / close overlay"),
        Line::from("  ?          toggle this help"),
        Line::from("  Tab        next tab"),
        Line::from("  Shift+Tab  previous tab"),
        Line::from("  j / k      move selection down / up"),
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
