//! Renders flattened [`TreeRow`]s into styled list items: indent guides,
//! connectors, the lifecycle status column, and the live flash language
//! (yellow = uncommitted, magenta flash = commit, green flash = push,
//! blue ◆ = being written right now).

use ratatui::{
    style::{Color, Stylize},
    text::{Line, Span},
    widgets::ListItem,
};

use super::theme;
use crate::app::TransitionKind;
use crate::app::tree::{RowKind, TreeRow};
use crate::git::{BranchLifecycle, BranchNode, ChangeKind, WorktreeRecord, lifecycle};
use crate::process::ProcessInfo;

/// Width reserved for the right-hand status column on wide terminals.
const RIGHT_COL: usize = 34;
/// Below this width the status is appended inline instead of column-aligned.
const NARROW: u16 = 70;

/// Build one styled list item per row. `transition_for` is the flash lookup
/// (keyed by [`NodeId::flash_key`]); `width` is the list area's inner width.
pub(crate) fn tree_items<'a>(
    rows: &'a [TreeRow<'a>],
    transition_for: &dyn Fn(&str) -> Option<TransitionKind>,
    width: u16,
) -> Vec<ListItem<'a>> {
    rows.iter()
        .map(|row| tree_item(row, transition_for(&row.id.flash_key()), width))
        .collect()
}

fn tree_item<'a>(
    row: &'a TreeRow<'a>,
    transition: Option<TransitionKind>,
    width: u16,
) -> ListItem<'a> {
    match &row.kind {
        RowKind::Repo {
            snap,
            branch_total,
            agent_total,
        } => {
            let arrow = if row.expanded { "▾ " } else { "▸ " };
            let left = vec![
                Span::from(arrow).fg(theme::ACCENT),
                Span::from(crate::home::repo_name(snap))
                    .bold()
                    .fg(theme::ACCENT),
            ];
            let mut status = vec![
                Span::from(format!("{branch_total} branches")).fg(theme::DIM),
                Span::from(" · ").fg(theme::DIM),
                Span::from(format!("{agent_total} agents")).fg(if *agent_total > 0 {
                    theme::CLEAN
                } else {
                    theme::DIM
                }),
            ];
            // Per-repo capture staleness: silent while fresh, loud when this
            // repo's data hasn't been recaptured in a while.
            if snap.captured_at > 0 {
                let age = crate::storage::events::epoch_secs().saturating_sub(snap.captured_at);
                if age > 60 {
                    status.push(
                        Span::from(format!("  ⚠ stale {}", super::util::human_dur(age)))
                            .fg(theme::COLLISION),
                    );
                }
            }
            ListItem::new(with_status_column(left, status, width))
        }
        RowKind::Branch {
            node,
            worktree_path,
            agent,
            risk,
            dimmed,
            is_current,
        } => branch_item(
            row,
            node,
            *worktree_path,
            *agent,
            *risk,
            *dimmed,
            *is_current,
            transition,
            width,
        ),
        RowKind::Detached { wt, agent } => detached_item(row, wt, *agent, transition, width),
        RowKind::File { file } => file_item(row, file, transition, width),
        RowKind::FileOverflow { hidden } => {
            let mut spans = prefix_spans(row, None);
            spans.push(Span::from(format!("… {hidden} more files")).fg(theme::DIM));
            ListItem::new(Line::from(spans))
        }
    }
}

#[allow(clippy::too_many_arguments)] // display context for one row kind
fn branch_item<'a>(
    row: &'a TreeRow<'a>,
    node: &'a BranchNode,
    worktree_path: Option<&'a str>,
    agent: Option<&'a ProcessInfo>,
    risk: usize,
    dimmed: bool,
    is_current: bool,
    transition: Option<TransitionKind>,
    width: u16,
) -> ListItem<'a> {
    let mut left = prefix_spans(row, transition);
    let mut name = Span::from(node.name.as_str()).bold();
    if dimmed {
        name = name.fg(theme::DIM);
    }
    left.push(name);
    if is_current {
        left.push(Span::from(" (current)").fg(theme::ACCENT));
    }
    if row.expandable && !row.expanded {
        left.push(Span::from(" ▸").fg(theme::DIM));
    }

    let lifecycle = lifecycle::refine_with_activity(
        node.lifecycle,
        transition == Some(TransitionKind::Activity),
    );
    let status = status_spans(node, agent, risk, lifecycle, dimmed);

    let mut lines = vec![with_status_column(left, status, width)];
    if let Some(path) = worktree_path {
        let mut spans = continuation_spans(row);
        spans.push(Span::from("⌂ ").fg(theme::DIM));
        spans.push(Span::from(path).fg(theme::DIM));
        lines.push(Line::from(spans));
    }

    // Milestone flashes / persistent uncommitted recolor the whole item, so a
    // committing branch reads magenta and a dirty one holds yellow.
    let row_color = super::milestone_color(transition).or(match lifecycle {
        BranchLifecycle::Uncommitted | BranchLifecycle::Editing => Some(theme::DIRTY),
        _ => None,
    });
    if let Some(color) = row_color {
        let recolored: Vec<Line> = lines
            .into_iter()
            .map(|line| {
                Line::from(
                    line.spans
                        .into_iter()
                        .map(|s| s.fg(color))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        return ListItem::new(recolored);
    }
    ListItem::new(lines)
}

/// The lifecycle / agent / divergence status cell for a branch row.
fn status_spans<'a>(
    node: &'a BranchNode,
    agent: Option<&'a ProcessInfo>,
    risk: usize,
    lifecycle: BranchLifecycle,
    dimmed: bool,
) -> Vec<Span<'a>> {
    let mut v: Vec<Span> = Vec::new();
    if let Some(a) = agent {
        let name = a.name.trim_end_matches(".exe");
        v.push(Span::from(format!("● {name}")).fg(theme::CLEAN));
        v.push(Span::from(" · ").fg(theme::DIM));
    }
    match lifecycle {
        BranchLifecycle::Editing => {
            v.push(Span::from("◆ editing").fg(theme::ACTIVITY));
        }
        BranchLifecycle::Uncommitted => {
            v.push(Span::from("uncommitted").fg(theme::DIRTY));
        }
        BranchLifecycle::Committed => {
            let unpushed = node.ahead.filter(|&n| n > 0);
            match unpushed {
                Some(n) => {
                    v.push(Span::from(format!("↑{n} ")).fg(theme::CLEAN));
                    v.push(Span::from("committed, not pushed"));
                }
                None => v.push(Span::from("committed, not pushed")),
            }
        }
        BranchLifecycle::Pushed => {
            v.push(Span::from("✓ pushed").fg(theme::CLEAN));
            v.push(Span::from(" · clean").fg(theme::DIM));
        }
        BranchLifecycle::Merged => v.push(Span::from("merged").fg(theme::DIM)),
        BranchLifecycle::Fresh => v.push(Span::from("fresh").fg(theme::DIM)),
    }
    if risk > 0 {
        v.push(Span::from(format!("  ⚠ {risk}")).fg(theme::COLLISION));
    }
    if node.behind.unwrap_or(0) > 0 {
        v.push(Span::from(format!("  ↓{}", node.behind.unwrap_or(0))).fg(Color::Yellow));
    }
    if node.behind_parent.unwrap_or(0) > 0 {
        v.push(
            Span::from(format!("  ⇣{} vs parent", node.behind_parent.unwrap_or(0)))
                .fg(Color::Yellow),
        );
    }
    if node.upstream_gone {
        v.push(Span::from("  upstream gone").fg(theme::COLLISION));
    }
    if dimmed {
        v = v.into_iter().map(|s| s.fg(theme::DIM)).collect();
    }
    v
}

fn detached_item<'a>(
    row: &'a TreeRow<'a>,
    wt: &'a WorktreeRecord,
    agent: Option<&'a ProcessInfo>,
    transition: Option<TransitionKind>,
    width: u16,
) -> ListItem<'a> {
    let mut left = prefix_spans(row, transition);
    left.push(Span::from(wt.display_name()).bold().fg(Color::Magenta));
    let mut status: Vec<Span> = Vec::new();
    if let Some(a) = agent {
        let name = a.name.trim_end_matches(".exe");
        status.push(Span::from(format!("● {name}")).fg(theme::CLEAN));
        status.push(Span::from(" · ").fg(theme::DIM));
    }
    status.push(Span::from("detached").fg(Color::Magenta));
    status.extend(super::status_badges(wt.status.as_ref()));

    let mut lines = vec![with_status_column(left, status, width)];
    let mut spans = continuation_spans(row);
    spans.push(Span::from("⌂ ").fg(theme::DIM));
    spans.push(Span::from(wt.path.as_str()).fg(theme::DIM));
    lines.push(Line::from(spans));

    let row_color =
        super::milestone_color(transition).or_else(|| super::uncommitted_color(wt.status.as_ref()));
    if let Some(color) = row_color {
        let recolored: Vec<Line> = lines
            .into_iter()
            .map(|line| {
                Line::from(
                    line.spans
                        .into_iter()
                        .map(|s| s.fg(color))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        return ListItem::new(recolored);
    }
    ListItem::new(lines)
}

fn file_item<'a>(
    row: &'a TreeRow<'a>,
    file: &'a crate::git::FileChange,
    transition: Option<TransitionKind>,
    width: u16,
) -> ListItem<'a> {
    let mut left = prefix_spans(row, None);
    // A live save pulse replaces the change glyph while hot.
    if transition == Some(TransitionKind::Activity) {
        left.push(Span::from("◆ ").fg(theme::ACTIVITY));
    }
    left.push(Span::from(file.path.as_str()));

    let (glyph, word, color) = match file.kind {
        ChangeKind::Modified => ("~", "modified", theme::DIRTY),
        ChangeKind::Added => ("+", "added", theme::CLEAN),
        ChangeKind::Deleted => ("-", "deleted", theme::COLLISION),
        ChangeKind::Renamed => ("→", "renamed", Color::Blue),
        ChangeKind::Untracked => ("?", "untracked", theme::DIM),
        ChangeKind::Conflicted => ("!", "conflicted", theme::COLLISION),
    };
    let status = vec![Span::from(format!("{glyph} {word}")).fg(color)];
    ListItem::new(with_status_column(left, status, width))
}

/// Indent guides + connector for a row: `│  ` per ancestor-with-siblings,
/// then `├─ ` / `└─ `, plus the transient ✚/⌫ created/removed marker.
fn prefix_spans<'a>(row: &TreeRow<'a>, transition: Option<TransitionKind>) -> Vec<Span<'a>> {
    let mut spans = Vec::new();
    if row.depth == 0 {
        return spans;
    }
    let mut prefix = String::new();
    for &has_sibling in &row.guides {
        prefix.push_str(if has_sibling { "│  " } else { "   " });
    }
    prefix.push_str(if row.is_last_child {
        "└─ "
    } else {
        "├─ "
    });
    spans.push(Span::from(prefix).fg(theme::DIM));
    match transition {
        Some(TransitionKind::Created) => spans.push(Span::from("✚ ").fg(Color::Cyan)),
        Some(TransitionKind::Deleted) => spans.push(Span::from("⌫ ").fg(theme::COLLISION)),
        _ => {}
    }
    spans
}

/// Continuation prefix for a second line under a row (the `⌂ path` line):
/// the same guides, with this row's connector replaced by its continuation.
fn continuation_spans<'a>(row: &TreeRow<'a>) -> Vec<Span<'a>> {
    let mut prefix = String::new();
    for &has_sibling in &row.guides {
        prefix.push_str(if has_sibling { "│  " } else { "   " });
    }
    if row.depth > 0 {
        prefix.push_str(if row.is_last_child { "   " } else { "│  " });
    }
    vec![Span::from(prefix).fg(theme::DIM)]
}

/// Lay `left` out with `status` right-anchored into a fixed column on wide
/// terminals; on narrow ones, append the status inline after two spaces.
fn with_status_column<'a>(left: Vec<Span<'a>>, status: Vec<Span<'a>>, width: u16) -> Line<'a> {
    let mut spans = left;
    if status.is_empty() {
        return Line::from(spans);
    }
    if width >= NARROW {
        let used: usize = spans.iter().map(|s| s.width()).sum();
        let budget = (width as usize).saturating_sub(RIGHT_COL + 3);
        let pad = budget.saturating_sub(used).max(2);
        spans.push(Span::from(" ".repeat(pad)));
    } else {
        spans.push(Span::from("  "));
    }
    spans.extend(status);
    Line::from(spans)
}
