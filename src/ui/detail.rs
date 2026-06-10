use crate::app::{App, DetailFocus};
use crate::ui::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap};
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
    let pulse = app.pulse();
    use crate::app::FieldStatus;
    // Border style for a pane keyed to a field. Precedence: a live-feed conflict
    // (pulsing red) wins, then an un-pushed local edit (yellow), then an upstream
    // "updated" marker (green), then focus accent, then dim.
    let border = |focused: bool, field: Option<&str>| -> Style {
        let key = field.unwrap_or("");
        if app.field_status(key) == Some(FieldStatus::Conflicted) {
            let red = if pulse { Color::Red } else { Color::LightRed };
            return Style::default()
                .fg(red)
                .add_modifier(ratatui::style::Modifier::BOLD);
        }
        if app.field_pending(key) {
            return Style::default()
                .fg(Color::Yellow)
                .add_modifier(ratatui::style::Modifier::BOLD);
        }
        match app.field_status(key) {
            Some(FieldStatus::Updated) => Style::default().fg(Color::Green),
            _ if focused => Style::default().fg(theme::ACCENT),
            _ => Style::default().fg(theme::DIM),
        }
    };
    // A small marker appended to a field/pane title: ⚠ conflict, ● un-pushed
    // edit, ✓ cleanly updated upstream.
    let marker = |field: &str| -> &'static str {
        if app.field_status(field) == Some(FieldStatus::Conflicted) {
            return " ⚠";
        }
        if app.field_pending(field) {
            return " ●";
        }
        match app.field_status(field) {
            Some(FieldStatus::Updated) => " ✓",
            _ => "",
        }
    };
    // Color of a field's inline marker, matching the border precedence.
    let marker_color = |field: &str| -> Color {
        if app.field_status(field) == Some(FieldStatus::Conflicted) {
            Color::Red
        } else if app.field_pending(field) {
            Color::Yellow
        } else {
            Color::Green
        }
    };
    // Focus is shown with a thicker (double-line) border *type* so it stays
    // visible regardless of the status-driven border colour (yellow/red/green).
    // This lets the selection "overlay" the underlying status colour instead of
    // overriding it.
    let border_kind = |focused: bool| -> BorderType {
        if focused {
            BorderType::Double
        } else {
            BorderType::Plain
        }
    };

    // Info pane: a read-only summary of the item; Enter opens the field editor.
    // Values reflect any un-pushed local edit (effective value).
    let field = |k: &str, key: &str, v: String| {
        Line::from(vec![
            Span::styled(format!("{k:<10}"), theme::label()),
            Span::raw(v),
            Span::styled(marker(key), Style::default().fg(marker_color(key))),
        ])
    };
    let header = vec![
        field("Title", "title", app.effective_field_value("title")),
        field("State", "state", app.effective_field_value("state")),
        field("Type", "type", item.item_type.clone()),
        field("Assignee", "assignee", app.effective_field_value("assignee")),
        field("Iteration", "iteration", app.effective_field_value("iteration")),
        field("Tags", "tags", app.effective_field_value("tags")),
    ];
    // The info pane bundles several fields, so its border reflects the
    // "strongest" status across them (conflict > pending > updated).
    let info_fields = ["title", "state", "assignee", "iteration", "tags"];
    let info_border = if info_fields
        .iter()
        .any(|k| app.field_status(k) == Some(FieldStatus::Conflicted))
    {
        let red = if pulse { Color::Red } else { Color::LightRed };
        Style::default()
            .fg(red)
            .add_modifier(ratatui::style::Modifier::BOLD)
    } else if info_fields.iter().any(|k| app.field_pending(k)) {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(ratatui::style::Modifier::BOLD)
    } else if info_fields
        .iter()
        .any(|k| app.field_status(k) == Some(FieldStatus::Updated))
    {
        Style::default().fg(Color::Green)
    } else if focus == DetailFocus::Info {
        Style::default().fg(theme::ACCENT)
    } else {
        Style::default().fg(theme::DIM)
    };
    f.render_widget(
        Paragraph::new(header).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} #{} info (Enter to edit fields) ", item.item_type, item.id))
                .border_type(border_kind(focus == DetailFocus::Info))
                .border_style(info_border),
        ),
        left[0],
    );

    f.render_widget(
        Paragraph::new(app.effective_field_value("description"))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Description (Enter/e to edit){} ", marker("description")))
                    .border_type(border_kind(focus == DetailFocus::Description))
                    .border_style(border(focus == DetailFocus::Description, Some("description"))),
            ),
        left[1],
    );

    let notes_val = app.effective_field_value("notes");
    let notes_body = if notes_val.is_empty() {
        Span::styled("no notes — press n to add", Style::default().fg(theme::DIM))
    } else {
        Span::raw(notes_val)
    };
    f.render_widget(
        Paragraph::new(Line::from(notes_body))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Notes (Enter/n to edit){} ", marker("notes")))
                    .border_type(border_kind(focus == DetailFocus::Notes))
                    .border_style(border(focus == DetailFocus::Notes, Some("notes"))),
            ),
        left[2],
    );

    let comments_focused = app.detail_focus == DetailFocus::Comments;
    let added = app.pending_added_comments();
    let mut comments: Vec<ListItem> = if item.comments.is_empty() && added.is_empty() {
        vec![ListItem::new(Span::styled("no comments — press c to add", Style::default().fg(theme::DIM)))]
    } else {
        item.comments
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let selected = comments_focused && i == app.comment_selected;
                let deleted = app.pending_comment_deleted(c.id);
                // An un-pushed edit overlays the comment text with a ● marker.
                let pending_edit = app.pending_comment_edit(c.id);
                let mut head_spans = vec![
                    Span::styled(c.author.clone(), Style::default().fg(Color::Green)),
                    Span::styled(format!("  {}", c.when), Style::default().fg(theme::DIM)),
                ];
                if deleted {
                    head_spans.push(Span::styled(
                        "  ✗ delete (unpushed)",
                        Style::default().fg(Color::Red),
                    ));
                } else if pending_edit.is_some() {
                    head_spans.push(Span::styled(
                        "  ● edited (unpushed)",
                        Style::default().fg(Color::Yellow),
                    ));
                }
                let head = Line::from(head_spans);
                let text = pending_edit.unwrap_or(&c.text).to_string();
                let body = if deleted {
                    Line::from(Span::styled(
                        text,
                        Style::default()
                            .fg(theme::DIM)
                            .add_modifier(ratatui::style::Modifier::CROSSED_OUT),
                    ))
                } else {
                    Line::from(Span::raw(text))
                };
                let item = ListItem::new(vec![head, body]);
                if selected { item.style(theme::selected_row()) } else { item }
            })
            .collect()
    };
    // Un-pushed new comments appear at the bottom with a ● marker.
    for (author, text) in &added {
        let head = Line::from(vec![
            Span::styled((*author).to_string(), Style::default().fg(Color::Green)),
            Span::styled("  ● new (unpushed)", Style::default().fg(Color::Yellow)),
        ]);
        let body = Line::from(Span::raw((*text).to_string()));
        comments.push(ListItem::new(vec![head, body]));
    }
    // The comments pane border turns yellow while comment changes are un-pushed.
    let comments_pending = !app.pending_comments.is_empty();
    let comments_border = if comments_pending {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(ratatui::style::Modifier::BOLD)
    } else {
        pane_border(comments_focused)
    };
    f.render_widget(
        List::new(comments).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Comments (c add · Enter edit · d delete · p push) ")
                .border_type(border_kind(comments_focused))
                .border_style(comments_border),
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
                .related_title(*id)
                .unwrap_or("(loading…)")
                .to_string();
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

    // The right column stacks Related over a Development section (PRs, commits,
    // branches, external links). Development only takes space when present.
    let right = if item.dev_links.is_empty() {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3)])
            .split(cols[1])
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length((item.dev_links.len() as u16).saturating_add(2).min(10)),
            ])
            .split(cols[1])
    };

    f.render_widget(
        List::new(related).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Related (l/h switch · Enter open · Esc back) ")
                .border_type(border_kind(relations_focused))
                .border_style(pane_border(relations_focused)),
        ),
        right[0],
    );

    if !item.dev_links.is_empty() {
        let dev: Vec<ListItem> = item
            .dev_links
            .iter()
            .map(|d| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{} ", d.kind), Style::default().fg(Color::Magenta)),
                    Span::styled(d.name.clone(), Style::default().fg(theme::ACCENT)),
                    Span::styled(format!("  {}", d.url), Style::default().fg(theme::DIM)),
                ]))
            })
            .collect();
        f.render_widget(
            List::new(dev).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Development ")
                    .border_style(Style::default().fg(theme::DIM)),
            ),
            right[1],
        );
    }
}

/// Accent border for the focused pane, dim otherwise.
fn pane_border(focused: bool) -> Style {
    if focused {
        Style::default().fg(theme::ACCENT)
    } else {
        Style::default().fg(theme::DIM)
    }
}
