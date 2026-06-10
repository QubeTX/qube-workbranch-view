//! Worktrees tab: the filtered worktree list (left) and details pane (right).

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
};

use super::theme;
use super::util::{field, label, reason_or_yes, render_placeholder};
use super::{activity_dot, milestone_color, status_badges, uncommitted_color};
use crate::app::{AppState, TransitionKind};
use crate::git::WorktreeRecord;
use crate::process::ProcessInfo;

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &AppState) {
    let visible = app.visible_indices();
    if visible.is_empty() {
        let note = if app.filter.is_some() {
            "no worktrees match the filter (Esc to clear)."
        } else {
            "no worktrees found in this repository."
        };
        render_placeholder(frame, area, "Worktrees", note);
        return;
    }

    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).areas(area);

    let current = app.snapshot.current_worktree_index();
    let items: Vec<ListItem> = visible
        .iter()
        .map(|&i| {
            let wt = &app.snapshot.worktrees[i];
            worktree_item(
                wt,
                Some(i) == current,
                app.snapshot.processes.agent_for_worktree(i),
                app.transition_for(&wt.path),
                crate::collision::count_for_worktree(&app.snapshot.collisions, i),
            )
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected.min(visible.len() - 1)));

    let title = match &app.filter {
        Some(f) => format!(
            " Worktrees ({}/{}) /{f} ",
            visible.len(),
            app.snapshot.worktrees.len()
        ),
        None => format!(" Worktrees ({}) ", visible.len()),
    };
    let list = List::new(items)
        .block(Block::bordered().title(title))
        // Bold + the ▸ cursor mark the selection WITHOUT overriding the row's own
        // foreground, so live flashes (activity / commit / push) and the
        // uncommitted-yellow name stay visible on the selected row too.
        .highlight_style(Style::new().bold())
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, list_area, &mut state);

    render_worktree_details(frame, detail_area, app.selected_worktree());
}

fn worktree_item<'a>(
    wt: &'a WorktreeRecord,
    is_current: bool,
    agent: Option<&ProcessInfo>,
    transition: Option<TransitionKind>,
    collisions: usize,
) -> ListItem<'a> {
    // Body spans: everything after the leading live-activity dot.
    let mut body = vec![Span::from(wt.display_name()).bold()];
    if is_current {
        body.push(Span::from(" (current)").fg(theme::ACCENT));
    }
    if let Some(agent) = agent {
        let name = agent.name.trim_end_matches(".exe");
        body.push(Span::from(format!("  ● {name} pid {}", agent.pid)).fg(theme::CLEAN));
    }
    if wt.detached {
        body.push(Span::from(" detached").fg(Color::Magenta));
    }
    if wt.locked.is_some() {
        body.push(Span::from(" locked").fg(Color::Blue));
    }
    if wt.prunable.is_some() {
        body.push(Span::from(" prunable").fg(theme::DIM));
    }
    if collisions > 0 {
        body.push(Span::from(format!("  ⚠ {collisions}")).fg(theme::COLLISION));
    }
    body.extend(status_badges(wt.status.as_ref()));

    let dot = activity_dot(transition);

    // A commit/push milestone, OR uncommitted state, recolors the ENTIRE row —
    // dot included — so an actively-edited uncommitted worktree is all yellow
    // (yellow line, yellow editing dot). Milestone flashes take precedence.
    let line_color = milestone_color(transition).or_else(|| uncommitted_color(wt.status.as_ref()));
    if let Some(color) = line_color {
        let recolored: Vec<Span> = std::iter::once(dot)
            .chain(body)
            .map(|s| s.fg(color))
            .collect();
        return ListItem::new(Line::from(recolored));
    }
    // Clean: the dot keeps its own live colour (blue ◆ while editing, dim · idle).
    let mut spans = vec![dot];
    spans.extend(body);
    ListItem::new(Line::from(spans))
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
