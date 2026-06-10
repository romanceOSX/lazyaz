use crate::app::{App, Tab};
use crate::ui::theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Browser-style tab bar across the top.
pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::raw(" ")];
    for (i, tab) in Tab::ORDER.iter().enumerate() {
        let style = if *tab == app.tab {
            theme::active_tab()
        } else {
            theme::inactive_tab()
        };
        spans.push(Span::styled(format!(" {}:{} ", i + 1, tab.title()), style));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::styled(
        "  lazyaz",
        ratatui::style::Style::default().fg(theme::DIM),
    ));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
