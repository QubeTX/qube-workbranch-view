//! Timeline tab: the archived event history (created/removed worktrees,
//! merge-conflict risks), newest first.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Stylize},
    text::{Line, Span},
    widgets::{Block, List, ListItem},
};

use super::theme;
use super::util::{human_dur, render_placeholder};
use crate::app::AppState;
use crate::storage::EventKind;

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &AppState) {
    if app.archive.is_empty() {
        render_placeholder(
            frame,
            area,
            "Timeline",
            "no recorded events yet — created/removed worktrees and merge-conflict risks appear here.",
        );
        return;
    }
    let now = crate::storage::events::epoch_secs();
    let items: Vec<ListItem> = app
        .archive
        .iter()
        .take(300)
        .map(|ev| {
            let kind = match ev.kind {
                EventKind::WorktreeCreated => Span::from("created").fg(Color::Cyan),
                EventKind::WorktreeRemoved => Span::from("removed").fg(theme::COLLISION),
                EventKind::ConflictRisk => Span::from("conflict-risk").fg(theme::DIRTY),
                EventKind::BranchCommitted => Span::from("committed").fg(theme::COMMITTED),
                EventKind::BranchPushed => Span::from("pushed").fg(theme::CLEAN),
                EventKind::BranchMerged => Span::from("merged").fg(theme::DIM),
            };
            let name = ev.branch.clone().unwrap_or_else(|| ev.path.clone());
            let mut spans = vec![
                Span::from(format!("{:>5} ago  ", human_dur(ev.age_secs(now)))).fg(theme::DIM),
                kind,
                Span::from(format!("  {name}")).bold(),
                Span::from(format!("   {}", ev.path)).fg(theme::DIM),
            ];
            if let Some(dirty) = ev.dirty
                && !dirty.is_clean()
            {
                spans.push(
                    Span::from(format!(
                        "  ({} unstaged, {} staged)",
                        dirty.unstaged, dirty.staged
                    ))
                    .fg(theme::DIRTY),
                );
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let list = List::new(items)
        .block(Block::bordered().title(format!(" Timeline ({}) ", app.archive.len())));
    frame.render_widget(list, area);
}
