//! Rendering for the machine-wide home view: one branch tree across every
//! active repository, with repo nodes at the root. Like `ui::render`, this
//! never runs Git or mutates state — it reads [`HomeState`] and draws.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListState, Paragraph, Wrap},
};

use super::theme;
use crate::app::LiveStatus;
use crate::home::HomeState;

/// Render the whole home view for the current frame.
pub fn render(frame: &mut Frame, home: &HomeState) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    render_header(frame, header, home);
    if home.snapshot.repos.is_empty() {
        render_empty(frame, body);
    } else {
        render_body(frame, body, home);
    }
    render_footer(frame, footer);

    if home.show_help {
        render_help(frame, frame.area());
    }
}

fn render_header(frame: &mut Frame, area: Rect, home: &HomeState) {
    let scanned = if home.snapshot.scanned_at == 0 {
        "scanning… ".to_string()
    } else {
        let age = crate::storage::events::epoch_secs().saturating_sub(home.snapshot.scanned_at);
        format!("scanned {} ago ", human_dur(age))
    };
    let title = Line::from(vec![
        Span::from(" WB-300 ").bold().fg(theme::ACCENT),
        Span::from("· home ").fg(theme::DIM),
        live_indicator(home.live),
        Span::from(scanned).fg(theme::DIM),
    ]);
    let mut summary = vec![
        Span::from(format!("{} ", home.repo_count()))
            .bold()
            .fg(theme::ACCENT),
        Span::from("repos   ").fg(theme::DIM),
        Span::from(format!("{} ", home.snapshot.total_worktrees())).bold(),
        Span::from("worktrees   ").fg(theme::DIM),
        Span::from(format!("{} ", home.snapshot.total_active()))
            .bold()
            .fg(theme::CLEAN),
        Span::from("with a live agent   ").fg(theme::DIM),
    ];
    if home.tree.show_all {
        summary.push(Span::from("showing all branches").fg(theme::ACCENT));
    } else {
        summary.push(Span::from("active branches only (a for all)").fg(theme::DIM));
    }
    let p = Paragraph::new(Line::from(summary)).block(Block::bordered().title(title));
    frame.render_widget(p, area);
}

fn live_indicator(status: LiveStatus) -> Span<'static> {
    match status {
        LiveStatus::Live => Span::from("● live ").fg(theme::CLEAN),
        LiveStatus::PollOnly => Span::from("◐ poll-only ").fg(Color::Yellow),
        LiveStatus::Static => Span::from("○ static ").fg(theme::DIM),
    }
}

fn render_body(frame: &mut Frame, area: Rect, home: &HomeState) {
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).areas(area);

    let rows = home.tree_rows();
    let inner_width = list_area.width.saturating_sub(4); // borders + cursor
    let items = super::tree::tree_items(&rows, &|key| home.transitions.get(key), inner_width);

    let mut state = ListState::default();
    state.select(home.tree.selected_index(&rows));

    let list = List::new(items)
        .block(Block::bordered().title(" Every repo · every branch · every agent "))
        // Bold + the ▸ cursor mark the selection WITHOUT overriding the row's
        // own foreground, so flashes and the uncommitted-yellow stay visible.
        .highlight_style(Style::new().bold())
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, list_area, &mut state);

    let selected = home.tree.selected_index(&rows).and_then(|i| rows.get(i));
    super::branches::render_details(frame, detail_area, &home.snapshot.repos, selected);
}

fn render_empty(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(Span::from("No active repositories found.").bold()),
        Line::from(""),
        Line::from(
            "WB-300's home view shows repositories that are being actively worked on —"
                .fg(theme::DIM),
        ),
        Line::from(
            "those with a running agent (Claude / Codex / …) inside them, plus repos set"
                .fg(theme::DIM),
        ),
        Line::from(
            "up for parallel work (≥ 2 worktrees) under ~/git, ~/code, ~/src, ….".fg(theme::DIM),
        ),
        Line::from(""),
        Line::from(Span::from("Press r to rescan · q to quit.").fg(theme::DIM)),
    ];
    let p = Paragraph::new(lines)
        .block(Block::bordered().title(" Home "))
        .alignment(Alignment::Center);
    frame.render_widget(p, area);
}

fn render_footer(frame: &mut Frame, area: Rect) {
    let hint = Line::from(vec![
        Span::from(" j/k ").fg(theme::ACCENT),
        Span::from("move  ").fg(theme::DIM),
        Span::from("h/l ").fg(theme::ACCENT),
        Span::from("fold  ").fg(theme::DIM),
        Span::from("a ").fg(theme::ACCENT),
        Span::from("active/all  ").fg(theme::DIM),
        Span::from("Enter ").fg(theme::ACCENT),
        Span::from("open repo  ").fg(theme::DIM),
        Span::from("r ").fg(theme::ACCENT),
        Span::from("rescan  ").fg(theme::DIM),
        Span::from("? ").fg(theme::ACCENT),
        Span::from("help  ").fg(theme::DIM),
        Span::from("q ").fg(theme::ACCENT),
        Span::from("quit").fg(theme::DIM),
    ]);
    frame.render_widget(Paragraph::new(hint), area);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::from("WB-300 — Home view").bold().fg(theme::ACCENT)),
        Line::from(""),
        Line::from("One tree across every repository being actively worked on: each"),
        Line::from("repo's branch hierarchy (main → workbranch → task branches), the"),
        Line::from("agent on each branch, and the files being changed."),
        Line::from(""),
        Line::from(vec![
            Span::from("  j / k        ").fg(theme::ACCENT),
            Span::from("move through the tree").fg(theme::DIM),
        ]),
        Line::from(vec![
            Span::from("  l / h        ").fg(theme::ACCENT),
            Span::from("expand / collapse a node").fg(theme::DIM),
        ]),
        Line::from(vec![
            Span::from("  Space        ").fg(theme::ACCENT),
            Span::from("toggle expansion").fg(theme::DIM),
        ]),
        Line::from(vec![
            Span::from("  a            ").fg(theme::ACCENT),
            Span::from("active branches only / all branches").fg(theme::DIM),
        ]),
        Line::from(vec![
            Span::from("  Enter        ").fg(theme::ACCENT),
            Span::from("open the selected repo's full view (actions live there)").fg(theme::DIM),
        ]),
        Line::from(vec![
            Span::from("  r            ").fg(theme::ACCENT),
            Span::from("rescan now").fg(theme::DIM),
        ]),
        Line::from(vec![
            Span::from("  ? / Esc      ").fg(theme::ACCENT),
            Span::from("toggle this help").fg(theme::DIM),
        ]),
        Line::from(vec![
            Span::from("  q            ").fg(theme::ACCENT),
            Span::from("quit").fg(theme::DIM),
        ]),
        Line::from(""),
        Line::from(
            Span::from("Live: ◆ a file is being saved · the whole line is yellow while uncommitted.")
                .fg(theme::DIM),
        ),
        Line::from(
            Span::from("Milestones flash the row: magenta committed · green pushed · ✚ created · ⌫ removed.")
                .fg(theme::DIM),
        ),
    ];
    let popup = centered_rect(72, 75, area);
    frame.render_widget(Clear, popup);
    let p = Paragraph::new(lines)
        .block(Block::bordered().title(" Help "))
        .wrap(Wrap { trim: true });
    frame.render_widget(p, popup);
}

/// A coarse human-readable duration (matches the per-repo header style).
fn human_dur(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

/// A centered rectangle `pct_x` × `pct_y` percent of `area` (for the help popup).
fn centered_rect(pct_x: u16, pct_y: u16, area: Rect) -> Rect {
    let [_, mid, _] = Layout::vertical([
        Constraint::Percentage((100 - pct_y) / 2),
        Constraint::Percentage(pct_y),
        Constraint::Percentage((100 - pct_y) / 2),
    ])
    .flex(Flex::Center)
    .areas(area);
    let [_, inner, _] = Layout::horizontal([
        Constraint::Percentage((100 - pct_x) / 2),
        Constraint::Percentage(pct_x),
        Constraint::Percentage((100 - pct_x) / 2),
    ])
    .flex(Flex::Center)
    .areas(mid);
    inner
}
