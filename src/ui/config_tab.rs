use crate::app::{App, Mode, CONFIG_FIELDS};
use crate::ui::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(5)])
        .split(area);

    let editing = app.mode == Mode::Insert;
    let items: Vec<ListItem> = CONFIG_FIELDS
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let selected = i == app.config_edit.selected;
            let style = if selected {
                theme::selected_row()
            } else {
                Style::default()
            };
            let mut spans = vec![Span::styled(format!("{name:<10}"), theme::label())];
            if selected && editing {
                spans.extend(app.config_edit.buffer.spans(Style::default()));
            } else {
                let v = app.config_field_value(i);
                spans.push(Span::raw(if v.is_empty() { "<unset>".to_string() } else { v }));
            }
            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();

    f.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Credentials (i/Enter edit · s save · L re-login) ")
                .border_style(Style::default().fg(theme::DIM)),
        ),
        rows[0],
    );

    let signed = app
        .auth
        .account()
        .map(|a| format!("signed in as {a} (mock)"))
        .unwrap_or_else(|| "not signed in".to_string());
    let path = crate::config::Config::path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let info = vec![
        Line::from(vec![Span::styled("auth:  ", theme::label()), Span::raw(signed)]),
        Line::from(vec![Span::styled("file:  ", theme::label()), Span::raw(path)]),
        Line::from(Span::styled(
            "Real Entra device-code login arrives in a later pass.",
            Style::default().fg(theme::DIM),
        )),
    ];
    f.render_widget(
        Paragraph::new(info).block(Block::default().borders(Borders::ALL).title(" Account ")),
        rows[1],
    );
}
