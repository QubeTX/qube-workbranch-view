//! Merge Risk tab: files changed on 2+ worktrees, grouped by severity —
//! a forecast of what will conflict when these branches merge back.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Stylize},
    text::{Line, Span},
    widgets::{Block, List, ListItem},
};

use super::theme;
use super::util::render_placeholder;
use crate::app::AppState;
use crate::collision::Severity;
use crate::git::WorktreeRecord;

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &AppState) {
    let collisions = &app.snapshot.collisions;
    let base = app.snapshot.base.as_deref();

    if collisions.is_empty() {
        let note = match base {
            None => {
                "No merge-conflict risk to forecast: no base branch (origin/main, main, …) \
                     was found, so cross-branch overlap can't be compared."
            }
            Some(_) => {
                "No merge-conflict risk — no file has been changed on two worktrees, so \
                        nothing should conflict when these branches merge back."
            }
        };
        render_placeholder(frame, area, "Merge Conflict Risk", note);
        return;
    }

    let mut items: Vec<ListItem> = vec![ListItem::new(Line::from(
        Span::from(format!(
            "Files changed on 2+ worktrees — likely to conflict when merged{}:",
            base.map_or_else(String::new, |b| format!(" into {b}"))
        ))
        .fg(theme::DIM),
    ))];
    let mut last_severity: Option<Severity> = None;
    for collision in collisions {
        if last_severity != Some(collision.severity) {
            items.push(ListItem::new(Line::from(
                Span::from(collision.severity.label())
                    .bold()
                    .fg(severity_color(collision.severity)),
            )));
            last_severity = Some(collision.severity);
        }
        // Each involved worktree, annotated with its agent (if one is attached),
        // joined by × to read as "these two will collide at merge".
        let who: Vec<String> = collision
            .worktrees
            .iter()
            .map(|&i| {
                let name = app
                    .worktrees()
                    .get(i)
                    .map_or_else(|| format!("#{i}"), WorktreeRecord::display_name);
                match app.snapshot.processes.agent_for_worktree(i) {
                    Some(a) => format!("{name} [{}]", a.name.trim_end_matches(".exe")),
                    None => name,
                }
            })
            .collect();
        items.push(ListItem::new(Line::from(vec![
            Span::from(format!("  {}", collision.file)).bold(),
            Span::from(format!("   {}", who.join("  ×  "))).fg(theme::DIM),
        ])));
    }

    let base_label = base.map_or_else(|| " · no base".to_string(), |b| format!(" vs {b}"));
    let list = List::new(items).block(Block::bordered().title(format!(
        " Merge Conflict Risk ({}){base_label} ",
        collisions.len()
    )));
    frame.render_widget(list, area);
}

fn severity_color(severity: Severity) -> Color {
    match severity {
        Severity::Critical => Color::Red,
        Severity::High => Color::LightRed,
        Severity::Medium => Color::Yellow,
        Severity::Low => theme::DIM,
    }
}
