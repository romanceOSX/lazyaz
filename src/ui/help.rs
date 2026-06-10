//! Context-aware fuzzy help popup (opened with `?`).
//!
//! Lists the keybindings for the currently focused area (plus Global), with a
//! text input that fuzzy-filters as you type. `j/k` (or arrows) move the
//! selection, `Enter` runs the highlighted action, `Esc` closes.

use crate::app::App;
use crate::keys::{bindings_for, Binding};
use crate::ui::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

/// Bindings matching the current help-input, ranked by fuzzy score.
pub fn filtered(app: &App) -> Vec<&'static Binding> {
    let all = bindings_for(app.context());
    crate::ui::fuzzy::rank(&all, &app.help.input.value(), |b| {
        [format!("{} {}", b.keys, b.desc)]
    })
    .into_iter()
    .copied()
    .collect()
}

fn centered(area: Rect, pct_x: u16, pct_y: u16) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .split(v[1])[1]
}

pub fn render(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 60, 60);
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::ACCENT))
        .title(format!(" Help · {:?} ", app.context()));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let mut prompt = vec![Span::styled("search: ", theme::label())];
    prompt.extend(app.help.input.spans(Style::default()));
    f.render_widget(Paragraph::new(Line::from(prompt)), rows[0]);

    let matches = filtered(app);
    let items: Vec<ListItem> = matches
        .iter()
        .map(|b| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<12}", b.keys), Style::default().fg(theme::ACCENT)),
                Span::raw(b.desc),
            ]))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("› ");
    let mut state = ListState::default();
    if !matches.is_empty() {
        state.select(Some(app.help.selected.min(matches.len() - 1)));
    }
    f.render_stateful_widget(list, rows[1], &mut state);

    f.render_widget(
        Paragraph::new(Span::styled(
            "type to filter · ↑/↓ move · Enter run · Esc close",
            Style::default().fg(theme::DIM),
        )),
        rows[2],
    );
}
