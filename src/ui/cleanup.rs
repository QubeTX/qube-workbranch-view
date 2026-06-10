//! Cleanup tab: every worktree scored for how safely it can be removed.

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
use crate::cleanup::CleanupState;

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &AppState) {
    let mut rows: Vec<(usize, CleanupState, String)> = app
        .snapshot
        .worktrees
        .iter()
        .enumerate()
        .filter(|(_, wt)| !wt.bare)
        .map(|(i, wt)| {
            let protected = i == 0 || app.snapshot.current_worktree_index() == Some(i);
            let has_agent = app.snapshot.processes.worktree_is_active(i);
            let (state, reason) = crate::cleanup::assess(wt, protected, has_agent);
            (i, state, reason)
        })
        .collect();
    if rows.is_empty() {
        render_placeholder(frame, area, "Cleanup", "no worktrees to assess.");
        return;
    }
    rows.sort_by_key(|(_, state, _)| state.rank());

    let items: Vec<ListItem> = rows
        .iter()
        .map(|(i, state, reason)| {
            let wt = &app.snapshot.worktrees[*i];
            ListItem::new(Line::from(vec![
                Span::from(format!("{} ", state.glyph()))
                    .bold()
                    .fg(cleanup_color(*state)),
                Span::from(wt.display_name()).bold(),
                Span::from(format!("   {reason}")).fg(theme::DIM),
                Span::from(format!("   {}", wt.path)).fg(theme::DIM),
            ]))
        })
        .collect();

    let list = List::new(items).block(Block::bordered().title(
        " Cleanup — ✓ safe · ! caution/uncommitted · ✗ active   (select in Branches, then x) ",
    ));
    frame.render_widget(list, area);
}

fn cleanup_color(state: CleanupState) -> Color {
    match state {
        CleanupState::Safe => theme::CLEAN,
        CleanupState::Caution | CleanupState::Dirty => theme::DIRTY,
        CleanupState::Active => theme::COLLISION,
        CleanupState::Protected => theme::DIM,
    }
}
