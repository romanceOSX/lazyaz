//! Collapsible relationship tree: shows the work-item hierarchy (epic → stories
//! → tasks) so the user can grasp structure at a glance instead of chain-opening
//! items one by one. j/k move, l/h expand/collapse, Enter opens in Detail.

use crate::app::App;
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
        .map(|(id, depth)| {
            let item = app.items.iter().find(|w| w.id == *id);
            let indent = "  ".repeat(*depth);
            let marker = if app.tree_has_children(*id) {
                if app.tree.expanded.contains(id) { "▾ " } else { "▸ " }
            } else {
                "• "
            };
            let (ty, title) = item
                .map(|w| (w.item_type.clone(), w.title.clone()))
                .unwrap_or_else(|| ("?".into(), "(unknown)".into()));
            Line::from(vec![
                Span::raw(indent),
                Span::styled(marker, Style::default().fg(theme::ACCENT)),
                Span::styled(format!("#{id} "), Style::default().fg(theme::DIM)),
                Span::styled(format!("{ty} "), Style::default().fg(Color::Magenta)),
                Span::raw(title),
            ])
            .into()
        })
        .collect();

    let mut state = ListState::default();
    if !app.tree.flat.is_empty() {
        state.select(Some(app.tree.selected.min(app.tree.flat.len() - 1)));
    }

    f.render_stateful_widget(
        List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Relationships (l/h expand·collapse · Enter open) ")
                    .border_style(Style::default().fg(theme::DIM)),
            )
            .highlight_style(theme::selected_row())
            .highlight_symbol("› "),
        area,
        &mut state,
    );
}
