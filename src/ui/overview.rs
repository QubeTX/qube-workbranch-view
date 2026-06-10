//! Overview tab: repo identity, headline counts, and the data-freshness line.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Stylize},
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};

use super::theme;
use super::util::human_dur;
use crate::app::{AppState, LiveStatus, TransitionKind};

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &AppState) {
    let s = &app.snapshot;
    let uncommitted = s
        .worktrees
        .iter()
        .filter(|wt| wt.status.as_ref().is_some_and(|st| !st.clean))
        .count();
    let active = (0..s.worktrees.len())
        .filter(|&i| s.processes.worktree_is_active(i))
        .count();
    let editing = s
        .worktrees
        .iter()
        .filter(|wt| app.transition_for(&wt.path) == Some(TransitionKind::Activity))
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
            Span::from(active.to_string()).bold().fg(theme::CLEAN),
            Span::from(" with an agent    ").fg(theme::DIM),
            Span::from(uncommitted.to_string()).bold().fg(theme::DIRTY),
            Span::from(" uncommitted    ").fg(theme::DIM),
            Span::from(s.collisions.len().to_string())
                .bold()
                .fg(theme::COLLISION),
            Span::from(" conflict-risk    ").fg(theme::DIM),
            Span::from(s.local_branch_count().to_string()).bold(),
            Span::from(" local    ").fg(theme::DIM),
            Span::from(s.remote_branch_count().to_string()).bold(),
            Span::from(" remote-tracking").fg(theme::DIM),
        ]),
        Line::from(""),
        // Live activity — only as trustworthy as the data is fresh, so it says so.
        Line::from(vec![
            Span::from("◆ ").fg(theme::ACTIVITY),
            Span::from(editing.to_string()).bold().fg(theme::ACTIVITY),
            Span::from(" editing right now    ").fg(theme::DIM),
            overview_freshness(app),
        ]),
    ];
    let p = Paragraph::new(lines)
        .block(Block::bordered().title(" Overview "))
        .wrap(Wrap { trim: true });
    frame.render_widget(p, area);
}

/// The Overview freshness line: how live the data is and how long since the last
/// successful capture — so a "live" summary can never imply fresh data when a
/// capture has been failing.
fn overview_freshness(app: &AppState) -> Span<'static> {
    let ago = app.last_updated.map_or_else(
        || "not yet".to_string(),
        |t| {
            format!(
                "{} ago",
                human_dur(crate::storage::events::epoch_secs().saturating_sub(t))
            )
        },
    );
    if app.stale {
        return Span::from(format!("⚠ stale — last good capture {ago}"))
            .bold()
            .fg(theme::COLLISION);
    }
    match app.live {
        LiveStatus::Live => Span::from(format!("● live · updated {ago}")).fg(theme::CLEAN),
        LiveStatus::PollOnly => {
            Span::from(format!("◐ poll-only · updated {ago}")).fg(Color::Yellow)
        }
        LiveStatus::Static => {
            Span::from(format!("○ static · updated {ago} (r to refresh)")).fg(theme::DIM)
        }
    }
}
