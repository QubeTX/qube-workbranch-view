//! Rendering: turns [`AppState`] into a Ratatui frame. UI never runs Git or
//! mutates state — it reads [`AppState`] and draws (handoff §8).

pub mod home;
pub mod theme;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState, Tabs, Wrap,
    },
};

use crate::app::{AppState, Confirm, LiveStatus, Overlay, Palette, Tab, TransitionKind};
use crate::cleanup::CleanupState;
use crate::collision::Severity;
use crate::git::{WorktreeRecord, WorktreeStatus};
use crate::process::ProcessInfo;
use crate::storage::EventKind;

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
    render_footer(frame, footer, app);

    if app.show_help {
        render_help(frame, frame.area());
    }

    match &app.overlay {
        Overlay::Search { query } => render_search_bar(frame, frame.area(), query),
        Overlay::Confirm(confirm) => render_confirm(frame, frame.area(), confirm),
        Overlay::Palette(palette) => render_palette(frame, frame.area(), palette),
        Overlay::None => {}
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &AppState) {
    let mut head = vec![
        Span::from(" WB-300 ").bold().fg(theme::ACCENT),
        Span::from(format!("· {} ", app.repo_label())).fg(theme::DIM),
        live_indicator(app.live),
    ];
    if app.stale {
        // A capture failed — say so rather than letting "● live" imply fresh data.
        head.push(Span::from("⚠ stale ").bold().fg(theme::COLLISION));
    }
    head.push(remote_indicator(app));
    let title = Line::from(head);
    let tabs = Tabs::new(Tab::ALL.iter().map(|t| Line::from(t.title())))
        .block(Block::bordered().title(title))
        .select(app.active_tab.index())
        .highlight_style(Style::new().bold().fg(theme::ACCENT));
    frame.render_widget(tabs, area);
}

fn live_indicator(status: LiveStatus) -> Span<'static> {
    match status {
        LiveStatus::Live => Span::from("● live ").fg(theme::CLEAN),
        LiveStatus::PollOnly => Span::from("◐ poll-only ").fg(Color::Yellow),
        LiveStatus::Static => Span::from("○ static ").fg(theme::DIM),
    }
}

fn remote_indicator(app: &AppState) -> Span<'static> {
    if app.fetching {
        return Span::from("⟳ fetching… ").fg(Color::Yellow);
    }
    match app.remote_checked {
        Some(epoch) => {
            let age = crate::storage::events::epoch_secs().saturating_sub(epoch);
            Span::from(format!("remote {} ago ", human_dur(age))).fg(theme::DIM)
        }
        None => Span::from("remote: not checked ").fg(theme::DIM),
    }
}

fn render_body(frame: &mut Frame, area: Rect, app: &AppState) {
    match app.active_tab {
        Tab::Overview => render_overview(frame, area, app),
        Tab::Worktrees => render_worktrees(frame, area, app),
        Tab::Processes => render_processes(frame, area, app),
        Tab::Collisions => render_collisions(frame, area, app),
        Tab::Cleanup => render_cleanup(frame, area, app),
        Tab::Timeline => render_timeline(frame, area, app),
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

fn render_worktrees(frame: &mut Frame, area: Rect, app: &AppState) {
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

/// The leading live-activity dot for a worktree row, in its own colour: blue ◆
/// while editing, ✚ created, ⌫ removed, dim · when idle. Milestones recolor the
/// whole row instead (see [`milestone_color`]), so the dot just dims for those.
pub(crate) fn activity_dot(transition: Option<TransitionKind>) -> Span<'static> {
    match transition {
        Some(TransitionKind::Activity) => Span::from("◆ ").fg(theme::ACTIVITY),
        Some(TransitionKind::Created) => Span::from("✚ ").fg(Color::Cyan),
        Some(TransitionKind::Deleted) => Span::from("⌫ ").fg(theme::COLLISION),
        _ => Span::from("· ").fg(theme::DIM),
    }
}

/// The persistent colour the row BODY takes from worktree state: yellow while it
/// has uncommitted work, dim when its status is unknown (a failed `git status`
/// must never read as clean), none (standard) when known-clean.
pub(crate) fn uncommitted_color(status: Option<&WorktreeStatus>) -> Option<Color> {
    match status {
        Some(s) if !s.clean => Some(theme::DIRTY),
        None => Some(theme::DIM),
        Some(_) => None,
    }
}

/// The solid colour a milestone flash recolors the whole row with, if any
/// (magenta = just committed, green = just pushed). Shared with the home view.
pub(crate) fn milestone_color(kind: Option<TransitionKind>) -> Option<Color> {
    match kind {
        Some(TransitionKind::Committed) => Some(theme::COMMITTED),
        Some(TransitionKind::Pushed) => Some(theme::CLEAN),
        _ => None,
    }
}

/// Compact dirty/divergence badges for the worktree list.
fn status_badges(status: Option<&WorktreeStatus>) -> Vec<Span<'static>> {
    let Some(s) = status else {
        return vec![Span::from("  …").fg(theme::DIM)];
    };
    let mut v = Vec::new();
    if s.clean && !s.diverged() {
        v.push(Span::from("  clean").fg(theme::CLEAN));
    }
    if s.unstaged > 0 {
        v.push(Span::from(format!("  {}~", s.unstaged)).fg(theme::DIRTY));
    }
    if s.staged > 0 {
        v.push(Span::from(format!(" {}+", s.staged)).fg(Color::Blue));
    }
    if s.untracked > 0 {
        v.push(Span::from(format!(" {}?", s.untracked)).fg(theme::DIM));
    }
    if s.conflicted > 0 {
        v.push(Span::from(format!(" {}!", s.conflicted)).fg(theme::COLLISION));
    }
    if s.ahead.unwrap_or(0) > 0 {
        v.push(Span::from(format!("  ↑{}", s.ahead.unwrap_or(0))).fg(theme::CLEAN));
    }
    if s.behind.unwrap_or(0) > 0 {
        v.push(Span::from(format!(" ↓{}", s.behind.unwrap_or(0))).fg(Color::Yellow));
    }
    if s.upstream_gone {
        v.push(Span::from("  upstream gone").fg(theme::COLLISION));
    }
    v
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

fn render_processes(frame: &mut Frame, area: Rect, app: &AppState) {
    let procs = &app.snapshot.processes.processes;
    if procs.is_empty() {
        render_placeholder(
            frame,
            area,
            "Processes",
            "no processes mapped to this repo's worktrees (best-effort — a process CWD may be unavailable).",
        );
        return;
    }

    let header = Row::new(["PID", "WORKTREE", "LABEL", "COMMAND", "CPU", "MEM", "RUN"])
        .style(Style::new().fg(theme::DIM).bold());
    let rows = procs.iter().map(|p| {
        let worktree = p
            .matched_worktree
            .and_then(|i| app.worktrees().get(i))
            .map_or_else(|| "-".to_string(), WorktreeRecord::display_name);
        let row = Row::new(vec![
            p.pid.to_string(),
            worktree,
            p.label.as_str().to_string(),
            truncate_cmd(&p.cmd, 48),
            format!("{:.0}%", p.cpu),
            human_mem(p.memory_bytes),
            human_dur(p.run_secs),
        ]);
        if p.label.is_agent() {
            row.style(Style::new().fg(Color::Green))
        } else {
            row
        }
    });
    let widths = [
        Constraint::Length(7),
        Constraint::Length(16),
        Constraint::Length(8),
        Constraint::Min(20),
        Constraint::Length(5),
        Constraint::Length(8),
        Constraint::Length(6),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::new().bold().fg(theme::ACCENT))
        .highlight_symbol("▸ ")
        .block(Block::bordered().title(format!(
            " Processes ({}) — j/k select · K kills selected ",
            procs.len()
        )));
    let mut state = TableState::default();
    state.select(Some(app.proc_selected.min(procs.len() - 1)));
    frame.render_stateful_widget(table, area, &mut state);
}

fn label(name: &str) -> Vec<Span<'static>> {
    vec![Span::from(format!("{name:<9}")).fg(theme::DIM)]
}

fn field(name: &str, value: String) -> Line<'static> {
    let mut spans = label(name);
    spans.push(Span::from(value));
    Line::from(spans)
}

fn reason_or_yes(reason: &str) -> String {
    if reason.is_empty() {
        "yes".to_string()
    } else {
        reason.to_string()
    }
}

fn human_mem(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1}gb", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{}mb", bytes / MB)
    } else if bytes >= KB {
        format!("{}kb", bytes / KB)
    } else {
        format!("{bytes}b")
    }
}

fn human_dur(secs: u64) -> String {
    if secs >= 86_400 {
        format!("{}d", secs / 86_400)
    } else if secs >= 3_600 {
        format!("{}h", secs / 3_600)
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

fn truncate_cmd(cmd: &str, max: usize) -> String {
    if cmd.chars().count() <= max {
        return cmd.to_string();
    }
    let kept: String = cmd.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

fn render_timeline(frame: &mut Frame, area: Rect, app: &AppState) {
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

fn render_collisions(frame: &mut Frame, area: Rect, app: &AppState) {
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

fn render_cleanup(frame: &mut Frame, area: Rect, app: &AppState) {
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
        " Cleanup — ✓ safe · ! caution/uncommitted · ✗ active   (select in Worktrees, then x) ",
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

fn render_search_bar(frame: &mut Frame, area: Rect, query: &str) {
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

fn render_confirm(frame: &mut Frame, area: Rect, confirm: &Confirm) {
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

fn render_palette(frame: &mut Frame, area: Rect, palette: &Palette) {
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

fn render_placeholder(frame: &mut Frame, area: Rect, title: &str, note: &str) {
    let body = Paragraph::new(format!("{title} — {note}"))
        .block(Block::bordered().title(format!(" {title} ")))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &AppState) {
    // A transient action result (e.g. a kill outcome) takes over the footer so a
    // requested destructive action is always acknowledged on screen.
    if let Some((text, is_error)) = app.status() {
        let color = if is_error {
            theme::COLLISION
        } else {
            theme::CLEAN
        };
        let line = Line::from(Span::from(format!(" {text}")).bold().fg(color));
        frame.render_widget(Paragraph::new(line), area);
        return;
    }
    let hints = Line::from(vec![
        Span::from(" q ").bold(),
        Span::from("quit  ").fg(theme::DIM),
        Span::from("Tab ").bold(),
        Span::from("tab  ").fg(theme::DIM),
        Span::from("j/k ").bold(),
        Span::from("move  ").fg(theme::DIM),
        Span::from("r ").bold(),
        Span::from("refresh  ").fg(theme::DIM),
        Span::from("f ").bold(),
        Span::from("fetch  ").fg(theme::DIM),
        Span::from("/ ").bold(),
        Span::from("find  ").fg(theme::DIM),
        Span::from(": ").bold(),
        Span::from("cmd  ").fg(theme::DIM),
        Span::from("x ").bold(),
        Span::from("remove  ").fg(theme::DIM),
        Span::from("K ").bold(),
        Span::from("kill  ").fg(theme::DIM),
        Span::from("1-7 ").bold(),
        Span::from("jump  ").fg(theme::DIM),
        Span::from("? ").bold(),
        Span::from("help").fg(theme::DIM),
    ]);
    frame.render_widget(Paragraph::new(hints), area);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let popup = center(area, Constraint::Percentage(60), Constraint::Length(19));
    let text = vec![
        Line::from("WB-300 — keybindings".bold()),
        Line::from(""),
        Line::from("  q / Esc    quit / close overlay"),
        Line::from("  ?          toggle this help"),
        Line::from("  Tab        next tab"),
        Line::from("  Shift+Tab  previous tab"),
        Line::from("  j / k      move selection down / up"),
        Line::from("  r          refresh the snapshot"),
        Line::from("  f          fetch from remotes"),
        Line::from("  /          search / filter worktrees"),
        Line::from("  :          command palette"),
        Line::from("  x          remove selected worktree (type-to-confirm)"),
        Line::from("  K          kill attached agent / selected process (confirm)"),
        Line::from("  p          prune stale worktree metadata"),
        Line::from("  1 – 7      jump to a tab"),
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

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::app::TransitionKind;
    use crate::git::{RepoIdentity, RepoSnapshot};
    use crate::process::ProcessSnapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn snapshot(worktrees: Vec<WorktreeRecord>) -> RepoSnapshot {
        RepoSnapshot {
            repo: RepoIdentity {
                start_dir: "/repo".into(),
                root: "/repo".into(),
                git_dir: "/repo/.git".into(),
                common_git_dir: "/repo/.git".into(),
                is_worktree: false,
            },
            base: None,
            worktrees,
            branches: Vec::new(),
            collisions: Vec::new(),
            processes: ProcessSnapshot::default(),
        }
    }

    /// A worktree on branch `feat`; `clean` toggles a single staged change so a
    /// dirty case has no yellow *badge* (staged is blue) — only the name carries
    /// the persistent-uncommitted yellow.
    fn wt(path: &str, clean: bool) -> WorktreeRecord {
        WorktreeRecord {
            path: path.into(),
            branch: Some("refs/heads/feat".into()),
            status: Some(WorktreeStatus {
                clean,
                staged: if clean { 0 } else { 1 },
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn app_on_worktrees(worktrees: Vec<WorktreeRecord>) -> AppState {
        let mut app = AppState::new(snapshot(worktrees));
        app.active_tab = Tab::Worktrees;
        app
    }

    fn render_buffer(app: &AppState) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(120, 12)).unwrap();
        terminal.draw(|f| render(f, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn has_fg(buf: &Buffer, color: Color) -> bool {
        buf.content().iter().any(|c| c.fg == color)
    }

    fn has_symbol(buf: &Buffer, needle: &str) -> bool {
        buf.content().iter().any(|c| c.symbol() == needle)
    }

    #[test]
    fn dirty_worktree_name_is_held_yellow() {
        let app = app_on_worktrees(vec![wt("/repo", false)]);
        assert!(has_fg(&render_buffer(&app), theme::DIRTY));
    }

    #[test]
    fn clean_worktree_shows_no_uncommitted_yellow() {
        let app = app_on_worktrees(vec![wt("/repo", true)]);
        assert!(!has_fg(&render_buffer(&app), theme::DIRTY));
    }

    #[test]
    fn activity_on_a_clean_worktree_shows_a_blue_dot() {
        // Clean + editing (the brief window before git sees the change) → blue ◆.
        let mut app = app_on_worktrees(vec![wt("/repo", true)]);
        app.note_activity(&[std::path::PathBuf::from("/repo/src/x.rs")]);
        let buf = render_buffer(&app);
        assert!(has_symbol(&buf, "◆"), "expected the editing dot");
        assert!(has_fg(&buf, theme::ACTIVITY), "blue while clean");
    }

    #[test]
    fn commit_flash_recolors_the_row_magenta() {
        let mut app = app_on_worktrees(vec![wt("/repo", true)]);
        app.transitions
            .note("/repo".into(), TransitionKind::Committed);
        assert!(has_fg(&render_buffer(&app), theme::COMMITTED));
    }

    #[test]
    fn push_flash_recolors_the_row_green() {
        // Dirty (so no "clean" green badge) and live=Static (so no green "● live"):
        // the only green in the pane can come from the push recolor.
        let mut app = app_on_worktrees(vec![wt("/repo", false)]);
        app.transitions.note("/repo".into(), TransitionKind::Pushed);
        assert!(has_fg(&render_buffer(&app), theme::CLEAN));
    }

    #[test]
    fn every_tab_renders_without_panicking() {
        let mut app = app_on_worktrees(vec![wt("/repo", false)]);
        app.set_status("✓ terminated foo (pid 1)".into(), false);
        app.transitions
            .note("/repo".into(), TransitionKind::Committed);
        for tab in Tab::ALL {
            app.active_tab = tab;
            let _ = render_buffer(&app); // every tab must render, not panic
        }
    }

    #[test]
    fn editing_an_uncommitted_worktree_is_all_yellow() {
        // Yellow line AND a yellow editing dot — no blue once uncommitted.
        let mut app = app_on_worktrees(vec![wt("/repo", false)]);
        app.note_activity(&[std::path::PathBuf::from("/repo/src/x.rs")]);
        let buf = render_buffer(&app);
        assert!(has_symbol(&buf, "◆"), "editing dot present");
        assert!(has_fg(&buf, theme::DIRTY), "all yellow");
        assert!(
            !has_fg(&buf, theme::ACTIVITY),
            "dot is not blue when uncommitted"
        );
    }
}
