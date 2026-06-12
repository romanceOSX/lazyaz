use crate::api::models::WorkItemState;
use crate::app::App;
use crate::ui::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

fn state_color(s: WorkItemState) -> Color {
    match s {
        WorkItemState::New => Color::Gray,
        WorkItemState::Active => Color::Green,
        WorkItemState::Resolved => Color::Yellow,
        WorkItemState::Closed => Color::DarkGray,
    }
}

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);

    // Filter hint row: the active time filter plus how to change the filters.
    let spans = vec![
        Span::styled("filter ", theme::label()),
        Span::styled(app.time_filter_label(), Style::default().fg(Color::Magenta)),
        Span::styled(
            "   f timeframe · i iteration · t type",
            Style::default().fg(theme::DIM),
        ),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), rows[0]);

    // List.
    let items: Vec<ListItem> = app
        .items
        .iter()
        .map(|w| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("#{:<5}", w.id), Style::default().fg(theme::DIM)),
                Span::styled(format!("{:<11}", w.item_type), Style::default().fg(Color::Magenta)),
                Span::styled(format!("{:<9}", w.state_name), Style::default().fg(state_color(w.state))),
                Span::raw(w.title.clone()),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Assigned to me ")
                .border_style(Style::default().fg(theme::DIM)),
        )
        .highlight_style(theme::selected_row())
        .highlight_symbol("› ");
    f.render_stateful_widget(list, rows[1], &mut app.list_state);
}
