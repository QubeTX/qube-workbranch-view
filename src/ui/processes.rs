//! Processes tab: every OS process mapped to a worktree, agents highlighted.

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Style},
    widgets::{Block, Row, Table, TableState},
};

use super::theme;
use super::util::{human_dur, human_mem, render_placeholder};
use crate::app::AppState;
use crate::git::WorktreeRecord;

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &AppState) {
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

fn truncate_cmd(cmd: &str, max: usize) -> String {
    if cmd.chars().count() <= max {
        return cmd.to_string();
    }
    let kept: String = cmd.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}
