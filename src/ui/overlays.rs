//! Overlays drawn on top of the active tab: search bar, type-to-confirm
//! dialog, command palette, and the help popup.

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, Paragraph},
};

use super::theme;
use crate::app::{Confirm, Palette};

pub(crate) fn render_search_bar(frame: &mut Frame, area: Rect, query: &str) {
    let [_, bar] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
    frame.render_widget(Clear, bar);
    let line = Line::from(vec![
        Span::from(" / ").bold().fg(theme::ACCENT),
        Span::from(query.to_string()),
        Span::from("▌").fg(theme::ACCENT),
        Span::from("   (Enter to keep · Esc to clear)").fg(theme::DIM),
    ]);
    frame.render_widget(Paragraph::new(line), bar);
}

pub(crate) fn render_confirm(frame: &mut Frame, area: Rect, confirm: &Confirm) {
    let height = (confirm.detail.len() + 6).min(20) as u16;
    let popup = center(area, Constraint::Percentage(60), Constraint::Length(height));
    let mut lines = vec![
        Line::from(Span::from(confirm.title.clone()).bold().fg(Color::Red)),
        Line::from(""),
    ];
    for detail in &confirm.detail {
        lines.push(Line::from(detail.clone()));
    }
    lines.push(Line::from(vec![
        Span::from("> ").fg(theme::ACCENT),
        Span::from(confirm.typed.clone()).bold(),
        Span::from("▌").fg(theme::ACCENT),
    ]));
    lines.push(Line::from(
        Span::from("Enter to confirm · Esc to cancel").fg(theme::DIM),
    ));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .title(" Confirm ")
                .border_style(Style::new().fg(Color::Red)),
        ),
        popup,
    );
}

pub(crate) fn render_palette(frame: &mut Frame, area: Rect, palette: &Palette) {
    let commands = crate::app::overlay::palette_filtered(&palette.query);
    let height = (commands.len() + 2).clamp(3, 12) as u16;
    let popup = center(area, Constraint::Percentage(50), Constraint::Length(height));
    let items: Vec<ListItem> = commands
        .iter()
        .enumerate()
        .map(|(i, command)| {
            let style = if i == palette.selected {
                Style::new().bold().fg(theme::ACCENT)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(
                format!("  {}", command.label()),
                style,
            )))
        })
        .collect();
    frame.render_widget(Clear, popup);
    frame.render_widget(
        List::new(items).block(Block::bordered().title(format!(" : {} ", palette.query))),
        popup,
    );
}

pub(crate) fn render_help(frame: &mut Frame, area: Rect) {
    let popup = center(area, Constraint::Percentage(62), Constraint::Length(24));
    let text = vec![
        Line::from("WB-300 — keybindings".bold()),
        Line::from(""),
        Line::from("  q / Esc    quit / close overlay"),
        Line::from("  ?          toggle this help"),
        Line::from("  Tab / 1-6  switch tab"),
        Line::from("  j / k      move selection down / up"),
        Line::from("  l / h      expand / collapse a tree node"),
        Line::from("  Enter      toggle expansion"),
        Line::from("  a          show active branches only / all branches"),
        Line::from("  r          refresh the snapshot"),
        Line::from("  f          fetch from remotes"),
        Line::from("  /          filter branches by name"),
        Line::from("  :          command palette"),
        Line::from("  x          remove the selected branch's worktree (confirm)"),
        Line::from("  K          kill attached agent / selected process (confirm)"),
        Line::from("  p          prune stale worktree metadata"),
        Line::from(""),
        Line::from("The tree is your branch hierarchy: repo → main →".fg(theme::DIM)),
        Line::from("workbranch → task branches. ⌂ marks each branch's".fg(theme::DIM)),
        Line::from("worktree; dimmed = no worktree, agent, or unmerged work.".fg(theme::DIM)),
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
