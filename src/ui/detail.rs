use crate::app::{App, DetailFocus};
use crate::ui::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let Some(item) = &app.current else {
        let hint = Paragraph::new("No item selected. Open one from the Work Items tab (l/Enter).")
            .block(Block::default().borders(Borders::ALL).title(" Detail "));
        f.render_widget(hint, area);
        return;
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(area);

    // Left: fields + description + notes + comments.
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),
            Constraint::Min(3),
            Constraint::Length(5),
            Constraint::Min(3),
        ])
        .split(cols[0]);

    let focus = app.detail_focus;
    let conflict_field = app.conflict.as_ref().map(|c| c.field);
    let pulse = app.pulse();
    // Border style for a pane: conflict (pulsing red) wins, then focus accent, then dim.
    let border = |focused: bool, field: Option<&str>| -> Style {
        if field.is_some() && conflict_field == field {
            let red = if pulse { Color::Red } else { Color::LightRed };
            Style::default().fg(red).add_modifier(ratatui::style::Modifier::BOLD)
        } else if focused {
            Style::default().fg(theme::ACCENT)
        } else {
            Style::default().fg(theme::DIM)
        }
    };

    // Info pane: a read-only summary of the item; Enter opens the field editor.
    let field = |k: &str, v: String| {
        Line::from(vec![
            Span::styled(format!("{k:<10}"), theme::label()),
            Span::raw(v),
        ])
    };
    let header = vec![
        field("Title", item.title.clone()),
        field("State", item.state.to_string()),
        field("Type", item.item_type.clone()),
        field("Assignee", item.assigned_to.clone()),
        field("Iteration", item.iteration.clone()),
        field("Tags", item.tags.join(", ")),
    ];
    f.render_widget(
        Paragraph::new(header).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} #{} info (Enter to edit fields) ", item.item_type, item.id))
                .border_style(border(focus == DetailFocus::Info, None)),
        ),
        left[0],
    );

    f.render_widget(
        Paragraph::new(item.description.clone())
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Description (Enter/e to edit) ")
                    .border_style(border(focus == DetailFocus::Description, Some("description"))),
            ),
        left[1],
    );

    let notes_body = if item.notes.is_empty() {
        Span::styled("no notes — press n to add", Style::default().fg(theme::DIM))
    } else {
        Span::raw(item.notes.clone())
    };
    f.render_widget(
        Paragraph::new(Line::from(notes_body))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Notes (Enter/n to edit) ")
                    .border_style(border(focus == DetailFocus::Notes, Some("notes"))),
            ),
        left[2],
    );

    let comments_focused = app.detail_focus == DetailFocus::Comments;
    let comments: Vec<ListItem> = if item.comments.is_empty() {
        vec![ListItem::new(Span::styled("no comments — press c to add", Style::default().fg(theme::DIM)))]
    } else {
        item.comments
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let selected = comments_focused && i == app.comment_selected;
                let head = Line::from(vec![
                    Span::styled(c.author.clone(), Style::default().fg(Color::Green)),
                    Span::styled(format!("  {}", c.when), Style::default().fg(theme::DIM)),
                ]);
                let body = Line::from(Span::raw(c.text.clone()));
                let item = ListItem::new(vec![head, body]);
                if selected { item.style(theme::selected_row()) } else { item }
            })
            .collect()
    };
    f.render_widget(
        List::new(comments).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Comments (c add · Enter edit) ")
                .border_style(pane_border(comments_focused)),
        ),
        left[3],
    );

    // Right: related items (parent + children), selectable.
    let relations_focused = app.detail_focus == DetailFocus::Relations;
    let ids = app.related_ids();
    let related: Vec<ListItem> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let rel = if Some(*id) == item.parent { "parent" } else { "child " };
            let title = app
                .client_title(*id)
                .unwrap_or_else(|| "(unknown)".to_string());
            let style = if relations_focused && i == app.detail_selected {
                theme::selected_row()
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{rel} "), Style::default().fg(theme::DIM)),
                Span::styled(format!("#{id} "), Style::default().fg(theme::ACCENT)),
                Span::raw(title),
            ]))
            .style(style)
        })
        .collect();

    let related = if related.is_empty() {
        vec![ListItem::new(Span::styled("no parent or children", Style::default().fg(theme::DIM)))]
    } else {
        related
    };
    f.render_widget(
        List::new(related).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Related (l/h switch · Enter open · Esc back) ")
                .border_style(pane_border(relations_focused)),
        ),
        cols[1],
    );
}

/// Accent border for the focused pane, dim otherwise.
fn pane_border(focused: bool) -> Style {
    if focused {
        Style::default().fg(theme::ACCENT)
    } else {
        Style::default().fg(theme::DIM)
    }
}
