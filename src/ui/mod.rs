//! Rendering: turns [`AppState`] into a Ratatui frame. UI never runs Git or
//! mutates state — it reads [`AppState`] and draws (handoff §8).

pub mod theme;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Row, Table, Tabs, Wrap},
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
    render_footer(frame, footer);

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
    let title = Line::from(vec![
        Span::from(" WB-300 ").bold().fg(theme::ACCENT),
        Span::from(format!("· {} ", app.repo_label())).fg(theme::DIM),
        live_indicator(app.live),
        remote_indicator(app),
    ]);
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
    let dirty = s
        .worktrees
        .iter()
        .filter(|wt| wt.status.as_ref().is_some_and(|st| !st.clean))
        .count();
    let active = (0..s.worktrees.len())
        .filter(|&i| s.processes.worktree_is_active(i))
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
            Span::from(" active    ").fg(theme::DIM),
            Span::from(dirty.to_string()).bold().fg(theme::DIRTY),
            Span::from(" dirty    ").fg(theme::DIM),
            Span::from(s.collisions.len().to_string())
                .bold()
                .fg(theme::COLLISION),
            Span::from(" collisions    ").fg(theme::DIM),
            Span::from(s.local_branch_count().to_string()).bold(),
            Span::from(" local    ").fg(theme::DIM),
            Span::from(s.remote_branch_count().to_string()).bold(),
            Span::from(" remote-tracking").fg(theme::DIM),
        ]),
        Line::from(""),
        Line::from(
            "Live lanes (active / changing / stale / archive) arrive in later phases."
                .fg(theme::DIM),
        ),
    ];
    let p = Paragraph::new(lines)
        .block(Block::bordered().title(" Overview "))
        .wrap(Wrap { trim: true });
    frame.render_widget(p, area);
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
        .highlight_style(Style::new().bold().fg(theme::ACCENT))
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
    let mut spans = Vec::new();
    if let Some(kind) = transition {
        spans.push(transition_marker(kind));
    }
    spans.push(Span::from(wt.display_name()).bold());
    if is_current {
        spans.push(Span::from(" (current)").fg(theme::ACCENT));
    }
    if let Some(agent) = agent {
        let name = agent.name.trim_end_matches(".exe");
        spans.push(Span::from(format!("  ● {name} pid {}", agent.pid)).fg(Color::Green));
    }
    if wt.detached {
        spans.push(Span::from(" detached").fg(Color::Magenta));
    }
    if wt.locked.is_some() {
        spans.push(Span::from(" locked").fg(Color::Blue));
    }
    if wt.prunable.is_some() {
        spans.push(Span::from(" prunable").fg(theme::DIM));
    }
    if collisions > 0 {
        spans.push(Span::from(format!("  ⚠ {collisions}")).fg(theme::COLLISION));
    }
    spans.extend(status_badges(wt.status.as_ref()));
    ListItem::new(Line::from(spans))
}

/// A short-lived "flash" glyph for a just-changed worktree.
fn transition_marker(kind: TransitionKind) -> Span<'static> {
    match kind {
        TransitionKind::Created => Span::from("✚ ").fg(Color::Cyan),
        TransitionKind::Modified => Span::from("✎ ").fg(theme::DIRTY),
        TransitionKind::Pushed => Span::from("↑ ").fg(theme::CLEAN),
        TransitionKind::Deleted => Span::from("⌫ ").fg(theme::COLLISION),
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
        .block(Block::bordered().title(format!(" Processes ({}) ", procs.len())));
    frame.render_widget(table, area);
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
            "no recorded events yet — created/removed worktrees will appear here.",
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
    let no_base = app.snapshot.base.is_none();

    if collisions.is_empty() {
        let note = if no_base {
            "no working-tree collisions. (No base branch like origin/main found, so \
             committed-but-clean conflicts aren't detected.)"
        } else {
            "no changed-file collisions across worktrees — nobody's stepping on each other."
        };
        render_placeholder(frame, area, "Collisions", note);
        return;
    }

    let mut items: Vec<ListItem> = Vec::new();
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
        let names: Vec<String> = collision
            .worktrees
            .iter()
            .map(|&i| {
                app.worktrees()
                    .get(i)
                    .map_or_else(|| format!("(worktree #{i})"), WorktreeRecord::display_name)
            })
            .collect();
        items.push(ListItem::new(Line::from(vec![
            Span::from(format!("  {}", collision.file)).bold(),
            Span::from(format!("   {}", names.join(", "))).fg(theme::DIM),
        ])));
    }

    let base_note = if no_base {
        " · base: none — committed-conflict detection inactive"
    } else {
        ""
    };
    let list = List::new(items)
        .block(Block::bordered().title(format!(" Collisions ({}){base_note} ", collisions.len())));
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

    let list =
        List::new(items).block(Block::bordered().title(
            " Cleanup — ✓ safe · ! caution/dirty · ✗ active   (select in Worktrees, then x) ",
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

fn render_footer(frame: &mut Frame, area: Rect) {
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
