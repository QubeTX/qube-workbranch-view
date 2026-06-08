//! Rendering: turns [`AppState`] into a Ratatui frame. UI never runs Git or
//! mutates state — it reads [`AppState`] and draws (handoff §8).

pub mod theme;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Tabs, Wrap},
};

use crate::app::{AppState, Tab};

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
        Span::from(format!("· {} ", app.repo_label)).fg(theme::DIM),
    ]);
    let tabs = Tabs::new(Tab::ALL.iter().map(|t| Line::from(t.title())))
        .block(Block::bordered().title(title))
        .select(app.active_tab.index())
        .highlight_style(Style::new().bold().fg(theme::ACCENT));
    frame.render_widget(tabs, area);
}

fn render_body(frame: &mut Frame, area: Rect, app: &AppState) {
    let placeholder = match app.active_tab {
        Tab::Overview => {
            "Overview — live lanes (active / changing / stale / archive) arrive in later phases."
        }
        Tab::Worktrees => {
            "Worktrees — the branch + worktree tree arrives with Git discovery (Phase 1)."
        }
        Tab::Processes => "Processes — process→worktree mapping arrives in Phase 3.",
        Tab::Collisions => "Collisions — changed-file overlap detection arrives in Phase 6.",
        Tab::Cleanup => "Cleanup — safe cleanup candidates arrive in Phase 8.",
        Tab::Help => "Help — press ? to toggle the help overlay anywhere.",
    };
    let body = Paragraph::new(placeholder)
        .block(Block::bordered().title(format!(" {} ", app.active_tab.title())))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_footer(frame: &mut Frame, area: Rect) {
    let hints = Line::from(vec![
        Span::from(" q ").bold(),
        Span::from("quit  ").fg(theme::DIM),
        Span::from("Tab ").bold(),
        Span::from("next  ").fg(theme::DIM),
        Span::from("1-6 ").bold(),
        Span::from("jump  ").fg(theme::DIM),
        Span::from("? ").bold(),
        Span::from("help").fg(theme::DIM),
    ]);
    frame.render_widget(Paragraph::new(hints), area);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let popup = center(area, Constraint::Percentage(60), Constraint::Length(12));
    let text = vec![
        Line::from("WB-300 — keybindings".bold()),
        Line::from(""),
        Line::from("  q / Esc    quit"),
        Line::from("  ?          toggle this help"),
        Line::from("  Tab        next tab"),
        Line::from("  Shift+Tab  previous tab"),
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
