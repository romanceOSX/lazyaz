//! Relationship tree: a lazily-walked view of a focus work item's parent,
//! itself + siblings, and its children. Collapsed nodes that have children show
//! `▸` (expand to fetch the next level); a `…` row at the top means there are
//! more ancestors above. j/k move, l/h expand·collapse, Enter opens, r refresh.

use crate::app::{App, TreeRow};
use crate::ui::theme;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .tree
        .flat
        .iter()
        .map(|row| match row {
            TreeRow::MoreAbove => Line::from(Span::styled(
                "  ⋯ (more above)",
                Style::default().fg(theme::DIM),
            ))
            .into(),
            TreeRow::Node { id, depth } => {
                let indent = "  ".repeat(*depth);
                // A node still being fetched renders as a placeholder.
                let Some(item) = app.tree_item(*id) else {
                    return ListItem::new(Line::from(vec![
                        Span::raw(indent),
                        Span::styled(
                            format!("{} #{id} loading…", app.spinner_frame()),
                            Style::default().fg(theme::DIM),
                        ),
                    ]));
                };
                let marker = if app.tree_has_children(*id) {
                    if app.tree.expanded.contains(id) { "▾ " } else { "▸ " }
                } else {
                    "• "
                };
                ListItem::new(Line::from(vec![
                    Span::raw(indent),
                    Span::styled(marker, Style::default().fg(theme::ACCENT)),
                    Span::styled(format!("#{id} "), Style::default().fg(theme::DIM)),
                    Span::styled(format!("{} ", item.item_type), Style::default().fg(Color::Magenta)),
                    Span::raw(item.title.clone()),
                ]))
            }
        })
        .collect();

    let mut state = ListState::default();
    if !app.tree.flat.is_empty() {
        state.select(Some(app.tree.selected.min(app.tree.flat.len() - 1)));
    }

    let title = if app.tree_loading() {
        format!(" Relationships  {} updating… ", app.spinner_frame())
    } else {
        " Relationships (l/h expand·collapse · J/K sibling · H/L level · r refresh) ".to_string()
    };
    f.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(theme::DIM)),
            )
            .highlight_style(theme::selected_row())
            .highlight_symbol("› "),
        area,
        &mut state,
    );
}
