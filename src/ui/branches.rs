//! Branches tab: the branch-hierarchy tree (left) and a details pane (right)
//! for the selected repo / branch / file.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, List, ListState, Paragraph, Wrap},
};

use super::theme;
use super::tree::tree_items;
use super::util::{field, human_dur, human_mem, label, reason_or_yes, render_placeholder};
use crate::app::AppState;
use crate::app::tree::{NodeId, RowKind, TreeRow};
use crate::git::{BranchNode, WorktreeRecord};

pub(crate) fn render(frame: &mut Frame, area: Rect, app: &AppState) {
    let rows = app.tree_rows();
    if rows.is_empty() {
        let note = if app.filter.is_some() {
            "no branches match the filter (Esc to clear)."
        } else {
            "no branches found in this repository."
        };
        render_placeholder(frame, area, "Branches", note);
        return;
    }

    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).areas(area);

    let inner_width = list_area.width.saturating_sub(4); // borders + cursor
    let items = tree_items(&rows, &|key| app.transitions.get(key), inner_width);

    let mut state = ListState::default();
    state.select(app.tree.selected_index(&rows));

    let active = app.snapshot.hierarchy.active_count();
    let all = app.snapshot.hierarchy.nodes.len();
    let scope = if app.tree.show_all {
        format!("all {all}")
    } else {
        format!("active {active}/{all}")
    };
    let title = match &app.filter {
        Some(f) => format!(" Branches ({scope}) /{f} "),
        None => format!(" Branches ({scope}) — a toggles all "),
    };
    let list = List::new(items)
        .block(Block::bordered().title(title))
        // Bold + the ▸ cursor mark the selection WITHOUT overriding the row's
        // own foreground, so flashes and the uncommitted-yellow stay visible.
        .highlight_style(Style::new().bold())
        .highlight_symbol("▸ ");
    frame.render_stateful_widget(list, list_area, &mut state);

    let selected = app.tree.selected_index(&rows).and_then(|i| rows.get(i));
    render_details(frame, detail_area, app, selected);
}

fn render_details(frame: &mut Frame, area: Rect, app: &AppState, row: Option<&TreeRow>) {
    let block = Block::bordered().title(" Details ");
    let Some(row) = row else {
        frame.render_widget(Paragraph::new("Nothing selected.").block(block), area);
        return;
    };
    let lines = match &row.kind {
        RowKind::Repo { snap, .. } => vec![
            field("repo", crate::home::repo_name(snap)),
            field("root", snap.repo.root.display().to_string()),
            field("base", snap.base.clone().unwrap_or_else(|| "—".into())),
            field(
                "branches",
                format!(
                    "{} local · {} remote-tracking",
                    snap.local_branch_count(),
                    snap.remote_branch_count()
                ),
            ),
            field("worktrees", snap.worktrees.len().to_string()),
        ],
        RowKind::Branch { node, .. } => branch_details(app, node),
        RowKind::Detached { wt, .. } => detached_details(wt),
        RowKind::File { file } => file_details(app, row, file),
        RowKind::FileOverflow { hidden } => vec![field("hidden", format!("{hidden} more files"))],
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn branch_details(app: &AppState, node: &BranchNode) -> Vec<Line<'static>> {
    let mut lines = vec![
        field("branch", node.name.clone()),
        field("role", node.role.as_str().to_string()),
        field(
            "parent",
            node.parent.clone().unwrap_or_else(|| "—".to_string()),
        ),
        field("stage", node.lifecycle.as_str().to_string()),
    ];
    match node.worktree.and_then(|i| app.snapshot.worktrees.get(i)) {
        Some(wt) => {
            lines.push(field("worktree", wt.path.clone()));
            if let Some(s) = &wt.status {
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
            }
            if let Some(reason) = &wt.locked {
                lines.push(field("locked", reason_or_yes(reason)));
            }
            if let Some(reason) = &wt.prunable {
                lines.push(field("prunable", reason_or_yes(reason)));
            }
        }
        None => lines.push(field("worktree", "none — branch only".to_string())),
    }
    if let Some(up) = &node.upstream {
        let mut spans = label("upstream");
        spans.push(Span::from(up.clone()));
        if node.upstream_gone {
            spans.push(Span::from("  (gone)").fg(theme::COLLISION));
        }
        lines.push(Line::from(spans));
        lines.push(field(
            "vs up",
            format!(
                "ahead {} · behind {}",
                node.ahead.unwrap_or(0),
                node.behind.unwrap_or(0)
            ),
        ));
    }
    if node.parent.is_some() {
        lines.push(field(
            "vs parent",
            format!(
                "ahead {} · behind {}",
                node.ahead_of_parent,
                node.behind_parent
                    .map_or_else(|| "?".to_string(), |b| b.to_string())
            ),
        ));
    }
    if let Some(idx) = node.worktree
        && let Some(agent) = app.snapshot.processes.agent_for_worktree(idx)
    {
        lines.push(field(
            "agent",
            format!(
                "{} pid {} · {:.0}% · {} · up {}",
                agent.name.trim_end_matches(".exe"),
                agent.pid,
                agent.cpu,
                human_mem(agent.memory_bytes),
                human_dur(agent.run_secs)
            ),
        ));
    }
    // Tip subject + date from the ref scan.
    if let Some(info) = app
        .snapshot
        .branches
        .iter()
        .find(|b| !b.is_remote && b.short == node.name)
    {
        if let Some(subject) = &info.subject {
            lines.push(field("tip", subject.clone()));
        }
        if let Some(date) = &info.committer_date {
            lines.push(field("when", date.clone()));
        }
    }
    // Merge-conflict risks involving this branch's worktree.
    if let Some(idx) = node.worktree {
        let risks: Vec<String> = app
            .snapshot
            .collisions
            .iter()
            .filter(|c| c.worktrees.contains(&idx))
            .map(|c| {
                let others: Vec<String> = c
                    .worktrees
                    .iter()
                    .filter(|&&i| i != idx)
                    .filter_map(|&i| app.snapshot.worktrees.get(i))
                    .map(WorktreeRecord::display_name)
                    .collect();
                format!("{} (also on {})", c.file, others.join(", "))
            })
            .collect();
        if !risks.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(
                Span::from("⚠ merge-conflict risk").fg(theme::COLLISION),
            ));
            for r in risks.iter().take(8) {
                lines.push(Line::from(Span::from(format!("  {r}")).fg(theme::DIM)));
            }
        }
    }
    lines
}

fn detached_details(wt: &WorktreeRecord) -> Vec<Line<'static>> {
    let mut lines = vec![
        field("worktree", wt.path.clone()),
        field("state", "detached HEAD — no branch".to_string()),
    ];
    if let Some(head) = wt.short_head() {
        lines.push(field("head", head.to_string()));
    }
    if let Some(s) = &wt.status {
        lines.push(field(
            "status",
            if s.clean {
                "clean".to_string()
            } else {
                format!(
                    "{} staged · {} unstaged · {} untracked",
                    s.staged, s.unstaged, s.untracked
                )
            },
        ));
    }
    lines
}

fn file_details(
    app: &AppState,
    row: &TreeRow,
    file: &crate::git::FileChange,
) -> Vec<Line<'static>> {
    let branch = match &row.id {
        NodeId::File { branch, .. } => branch.clone(),
        _ => String::new(),
    };
    let mut lines = vec![
        field("file", file.path.clone()),
        field("change", file.kind.as_str().to_string()),
        field("branch", branch),
    ];
    if let Some(c) = app.snapshot.collisions.iter().find(|c| c.file == file.path) {
        let others: Vec<String> = c
            .worktrees
            .iter()
            .filter_map(|&i| app.snapshot.worktrees.get(i))
            .map(WorktreeRecord::display_name)
            .collect();
        lines.push(Line::from(vec![
            Span::from("⚠ ").fg(theme::COLLISION),
            Span::from(format!("also changed on: {}", others.join(", "))).fg(theme::COLLISION),
        ]));
    }
    lines
}
