//! Floating field editor opened from the Detail "info" pane. Lists every
//! editable field of the item (generic across story/epic/feature/milestone/…),
//! navigated with j/k; Enter edits the selected field inline (single-line),
//! cycles it (state), or hands off to `$EDITOR` (multiline).

use crate::app::{App, FieldKind, EDITABLE_FIELDS};
use crate::ui::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

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
    let Some(ed) = &app.info_editor else { return };
    let item_type = app
        .current
        .as_ref()
        .map(|w| w.item_type.clone())
        .unwrap_or_else(|| "Item".into());

    let area = centered(f.area(), 64, 70);
    f.render_widget(Clear, area);
    // The editor border turns yellow while it holds un-pushed local edits.
    let any_pending = EDITABLE_FIELDS.iter().any(|f| app.field_pending(f.key));
    let border_color = if any_pending {
        ratatui::style::Color::Yellow
    } else {
        theme::ACCENT
    };
    let title = if any_pending {
        format!(" Edit {item_type} #{} · ● unpushed (p to push) ", ed.id)
    } else {
        format!(" Edit {item_type} #{} ", ed.id)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let editing_idx = ed.editing.as_ref().map(|_| ed.selected);
    let items: Vec<ListItem> = EDITABLE_FIELDS
        .iter()
        .enumerate()
        .map(|(i, field)| {
            let mut spans = vec![Span::styled(format!("{:<12}", field.label), theme::label())];
            if editing_idx == Some(i) {
                // Inline editor for the selected single-line field.
                if let Some(input) = &ed.editing {
                    spans.extend(input.spans(Style::default()));
                }
            } else {
                let mut val = app.info_field_value(field.key);
                if matches!(field.kind, FieldKind::Multiline) {
                    val = val.replace('\n', " ⏎ ");
                    if val.len() > 40 {
                        val.truncate(40);
                        val.push('…');
                    }
                }
                spans.push(Span::raw(val));
                // Flag fields with an un-pushed local edit.
                if app.field_pending(field.key) {
                    spans.push(Span::styled(
                        " ●",
                        Style::default().fg(ratatui::style::Color::Yellow),
                    ));
                }
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(ed.selected));
    f.render_stateful_widget(
        List::new(items)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("› "),
        rows[0],
        &mut state,
    );

    let hint = if ed.editing.is_some() {
        "editing · Enter save · Esc cancel"
    } else {
        "j/k move · Enter edit (state cycles, long text → $EDITOR) · Esc close"
    };
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(theme::DIM))),
        rows[1],
    );
}
