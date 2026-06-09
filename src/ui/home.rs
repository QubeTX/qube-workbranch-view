//! Rendering for the machine-wide home view. Like `ui::render`, this never runs
//! Git or mutates state — it reads [`HomeState`] and draws.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use super::theme;
use crate::app::{LiveStatus, TransitionKind};
use crate::git::{RepoSnapshot, WorktreeRecord, WorktreeStatus};
use crate::home::{HomeState, repo_name, workbranch_label};

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
    let summary = Line::from(vec![
        Span::from(format!("{} ", home.repo_count()))
            .bold()
            .fg(theme::ACCENT),
        Span::from("repos   ").fg(theme::DIM),
        Span::from(format!("{} ", home.snapshot.total_worktrees())).bold(),
        Span::from("worktrees   ").fg(theme::DIM),
        Span::from(format!("{} ", home.snapshot.total_active()))
            .bold()
            .fg(theme::CLEAN),
        Span::from("with a live agent").fg(theme::DIM),
    ]);
    let p = Paragraph::new(summary).block(Block::bordered().title(title));
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
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(area);

    render_repo_list(frame, list_area, home);
    render_repo_detail(frame, detail_area, home);
}

fn render_repo_list(frame: &mut Frame, area: Rect, home: &HomeState) {
    let items: Vec<ListItem> = home
        .snapshot
        .repos
        .iter()
        .map(|repo| {
            let active = active_count(repo);
            let dirty = dirty_count(repo);
            let mut spans = vec![Span::from(repo_name(repo)).bold()];
            spans.push(Span::from(format!("  {} wt", repo.worktrees.len())).fg(theme::DIM));
            if active > 0 {
                spans.push(Span::from(format!("  ● {active}")).fg(theme::CLEAN));
            }
            if dirty > 0 {
                spans.push(Span::from(format!("  ✎ {dirty}")).fg(theme::DIRTY));
            }
            if !repo.collisions.is_empty() {
                spans.push(
                    Span::from(format!("  ⚠ {}", repo.collisions.len())).fg(theme::COLLISION),
                );
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items)
        .block(Block::bordered().title(" Repos "))
        .highlight_style(Style::new().bold().fg(theme::ACCENT))
        .highlight_symbol("▌ ");
    let mut state = ListState::default();
    state.select(Some(home.selected));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_repo_detail(frame: &mut Frame, area: Rect, home: &HomeState) {
    let Some(repo) = home.selected_repo() else {
        return;
    };

    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::from("repo  ").fg(theme::DIM),
            Span::from(repo_name(repo)).bold(),
        ]),
        Line::from(Span::from(repo.repo.root.display().to_string()).fg(theme::DIM)),
        Line::from(""),
    ];

    // Group worktrees by their (heuristic) workbranch, preserving first-seen
    // group order. Each entry keeps the worktree's original index so agent /
    // collision lookups against the snapshot stay correct.
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (idx, wt) in repo.worktrees.iter().enumerate() {
        if wt.bare {
            continue;
        }
        let label = workbranch_label(wt.branch_short());
        match groups.iter_mut().find(|(g, _)| *g == label) {
            Some((_, v)) => v.push(idx),
            None => groups.push((label, vec![idx])),
        }
    }

    if groups.is_empty() {
        lines.push(Line::from(Span::from("(no worktrees)").fg(theme::DIM)));
    }

    for (label, indices) in groups {
        lines.push(Line::from(
            Span::from(format!("▸ {label}")).bold().fg(theme::ACCENT),
        ));
        for idx in indices {
            lines.push(worktree_line(home, repo, idx));
        }
        lines.push(Line::from(""));
    }

    let p = Paragraph::new(lines)
        .block(Block::bordered().title(" Worktrees by workbranch "))
        .wrap(Wrap { trim: true });
    frame.render_widget(p, area);
}

/// One worktree row: a flash marker, branch, agent badge, dirty/divergence
/// badges, and a collision marker.
fn worktree_line(home: &HomeState, repo: &RepoSnapshot, idx: usize) -> Line<'static> {
    let wt = &repo.worktrees[idx];
    let mut spans = vec![flash_marker(home.transition_for(&wt.path)), Span::from(" ")];
    spans.push(Span::from(wt.display_name()));

    if let Some(agent) = repo.processes.agent_for_worktree(idx) {
        spans.push(Span::from(format!("  ● {} pid {}", agent.name, agent.pid)).fg(theme::CLEAN));
    }

    spans.extend(status_badges(wt.status.as_ref()));

    let collisions = repo
        .collisions
        .iter()
        .filter(|c| c.worktrees.contains(&idx))
        .count();
    if collisions > 0 {
        spans.push(Span::from(format!("  ⚠ {collisions}")).fg(theme::COLLISION));
    }
    Line::from(spans)
}

/// A coloured dot for the worktree's active transition flash (dim dot if none).
fn flash_marker(kind: Option<TransitionKind>) -> Span<'static> {
    match kind {
        Some(TransitionKind::Created) => Span::from("●").fg(theme::CLEAN),
        Some(TransitionKind::Modified) => Span::from("●").fg(theme::DIRTY),
        Some(TransitionKind::Pushed) => Span::from("●").fg(theme::ACCENT),
        Some(TransitionKind::Deleted) => Span::from("●").fg(theme::COLLISION),
        None => Span::from("·").fg(theme::DIM),
    }
}

fn status_badges(status: Option<&WorktreeStatus>) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let Some(s) = status else {
        return spans;
    };
    if s.clean {
        spans.push(Span::from("  clean").fg(theme::DIM));
    } else {
        let dirty = s.staged + s.unstaged + s.untracked + s.conflicted;
        spans.push(Span::from(format!("  ✎ {dirty}")).fg(theme::DIRTY));
    }
    if let Some(ahead) = s.ahead.filter(|&a| a > 0) {
        spans.push(Span::from(format!("  ↑{ahead}")).fg(theme::CLEAN));
    }
    if let Some(behind) = s.behind.filter(|&b| b > 0) {
        spans.push(Span::from(format!("  ↓{behind}")).fg(theme::DIRTY));
    }
    if s.upstream_gone {
        spans.push(Span::from("  upstream-gone").fg(theme::COLLISION));
    }
    spans
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
        Span::from("select  ").fg(theme::DIM),
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
        Line::from("A machine-wide control tower over every repository being actively"),
        Line::from("worked on: those with a running agent, plus parallel-work repos under"),
        Line::from("your code-home directories. Each card breaks a repo down by workbranch."),
        Line::from(""),
        Line::from(vec![
            Span::from("  j / k        ").fg(theme::ACCENT),
            Span::from("move between repositories").fg(theme::DIM),
        ]),
        Line::from(vec![
            Span::from("  Enter        ").fg(theme::ACCENT),
            Span::from("open the selected repo's full view").fg(theme::DIM),
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
            Span::from("Dots flash as worktrees change: green created · yellow modified")
                .fg(theme::DIM),
        ),
        Line::from(Span::from("· cyan pushed · red deleted.").fg(theme::DIM)),
    ];
    let popup = centered_rect(72, 70, area);
    frame.render_widget(Clear, popup);
    let p = Paragraph::new(lines)
        .block(Block::bordered().title(" Help "))
        .wrap(Wrap { trim: true });
    frame.render_widget(p, popup);
}

fn active_count(repo: &RepoSnapshot) -> usize {
    (0..repo.worktrees.len())
        .filter(|&i| repo.processes.worktree_is_active(i))
        .count()
}

fn dirty_count(repo: &RepoSnapshot) -> usize {
    repo.worktrees
        .iter()
        .filter(|wt: &&WorktreeRecord| wt.status.as_ref().is_some_and(|s| !s.clean))
        .count()
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
