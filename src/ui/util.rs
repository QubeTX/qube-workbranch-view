//! Small shared rendering helpers used across the per-tab UI modules.

use ratatui::{
    Frame,
    layout::Rect,
    style::Stylize,
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};

use super::theme;

pub(crate) fn label(name: &str) -> Vec<Span<'static>> {
    vec![Span::from(format!("{name:<9}")).fg(theme::DIM)]
}

pub(crate) fn field(name: &str, value: String) -> Line<'static> {
    let mut spans = label(name);
    spans.push(Span::from(value));
    Line::from(spans)
}

pub(crate) fn reason_or_yes(reason: &str) -> String {
    if reason.is_empty() {
        "yes".to_string()
    } else {
        reason.to_string()
    }
}

pub(crate) fn human_mem(bytes: u64) -> String {
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

pub(crate) fn human_dur(secs: u64) -> String {
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

pub(crate) fn render_placeholder(frame: &mut Frame, area: Rect, title: &str, note: &str) {
    let body = Paragraph::new(format!("{title} — {note}"))
        .block(Block::bordered().title(format!(" {title} ")))
        .wrap(Wrap { trim: true });
    frame.render_widget(body, area);
}
