//! Renders the in-pane fuzzy-filter search bar (committed query "tags" as chips
//! plus the live input) shown at the top of the Work Items / Tree panes.

use crate::app::FuzzyFilter;
use crate::ui::theme;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// A one-line search bar: `🔍 [tag] [tag] query▏   (hint)`.
pub fn line(filter: &FuzzyFilter) -> Line<'static> {
    let mut spans = vec![Span::styled("/ ", theme::label())];
    let chip = Style::default()
        .fg(Color::Black)
        .bg(theme::ACCENT)
        .add_modifier(Modifier::BOLD);
    for tag in &filter.tags {
        spans.push(Span::styled(format!(" {tag} "), chip));
        spans.push(Span::raw(" "));
    }
    if let Some(input) = &filter.input {
        spans.extend(input.spans(Style::default().fg(Color::White)));
        spans.push(Span::styled(
            "   Enter add · ⌫ del tag · Esc close",
            Style::default().fg(theme::DIM),
        ));
    } else {
        spans.push(Span::styled(
            "  (/ to edit · Esc to clear)",
            Style::default().fg(theme::DIM),
        ));
    }
    Line::from(spans)
}
