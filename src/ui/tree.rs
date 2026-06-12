//! Relationship tree: a lazily-walked view of a focus work item's parent,
//! itself + siblings, and its children. Hierarchy is drawn with box-drawing
//! connectors (`├─`, `└─`, `│`) like `tree(1)`/`cargo tree`; expandable nodes
//! carry a chevron (`▸` collapsed, `▾` expanded). A `⋯` row at the top means
//! there are more ancestors above. j/k move, l/h expand·collapse, Enter opens,
//! r refresh.

use crate::app::{App, TreeRow};
use crate::ui::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

/// Build the box-drawing prefix for the `Node` row at `idx` (depth `depth`),
/// derived purely from the depth sequence of the visible rows. For each
/// ancestor level we draw a continuation rule (`│  `) when that ancestor has a
/// later sibling, else blank; the node's own slot is an elbow (`├─ ` when it
/// has a following sibling, `└─ ` when it's the last child). The root (depth 0)
/// gets no prefix.
fn connector_prefix(flat: &[TreeRow], idx: usize, depth: usize) -> String {
    // `continues(level)`: scanning forward from `idx`, is the next row at
    // depth <= level actually *at* `level` (a following sibling) rather than
    // shallower (meaning this branch has ended)?
    let continues = |level: usize| -> bool {
        for row in &flat[idx + 1..] {
            if let TreeRow::Node { depth: d, .. } = row {
                if *d < level {
                    return false;
                }
                if *d == level {
                    return true;
                }
            }
        }
        false
    };

    let mut prefix = String::new();
    for level in 1..=depth {
        if level == depth {
            prefix.push_str(if continues(level) { "├─ " } else { "└─ " });
        } else {
            prefix.push_str(if continues(level) { "│  " } else { "   " });
        }
    }
    prefix
}

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    // Reserve a top line for the fuzzy search bar when it's in use.
    let area = if app.tree_filter.active() || app.tree_filter.searching() {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(area);
        f.render_widget(
            Paragraph::new(crate::ui::search::line(&app.tree_filter)),
            rows[0],
        );
        rows[1]
    } else {
        area
    };

    let items: Vec<ListItem> = app
        .tree
        .flat
        .iter()
        .enumerate()
        .map(|(idx, row)| match row {
            TreeRow::MoreAbove => Line::from(Span::styled(
                "⋯ (more above)",
                Style::default().fg(theme::DIM),
            ))
            .into(),
            TreeRow::Node { id, depth } => {
                let prefix = connector_prefix(&app.tree.flat, idx, *depth);
                // A node still being fetched renders as a placeholder.
                let Some(item) = app.tree_item(*id) else {
                    return ListItem::new(Line::from(vec![
                        Span::styled(prefix, Style::default().fg(theme::DIM)),
                        Span::styled(
                            format!("{} #{id} loading…", app.spinner_frame()),
                            Style::default().fg(theme::DIM),
                        ),
                    ]));
                };
                // Fixed-width chevron slot keeps titles aligned: a chevron for
                // expandable nodes, blank for leaves.
                let chevron = if app.tree_has_children(*id) {
                    if app.tree.expanded.contains(id) { "▾ " } else { "▸ " }
                } else {
                    "  "
                };
                ListItem::new(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(theme::DIM)),
                    Span::styled(chevron, Style::default().fg(theme::ACCENT)),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u32, depth: usize) -> TreeRow {
        TreeRow::Node { id, depth }
    }

    // A small tree:
    //   1001 (0)
    //   ├─ 1002 (1)
    //   │  ├─ 1004 (2)
    //   │  └─ 1005 (2)
    //   └─ 1003 (1)
    //      └─ 1006 (2)
    fn sample() -> Vec<TreeRow> {
        vec![
            node(1001, 0),
            node(1002, 1),
            node(1004, 2),
            node(1005, 2),
            node(1003, 1),
            node(1006, 2),
        ]
    }

    #[test]
    fn root_has_no_prefix() {
        assert_eq!(connector_prefix(&sample(), 0, 0), "");
    }

    #[test]
    fn non_last_child_uses_tee_last_child_uses_elbow() {
        let f = sample();
        assert_eq!(connector_prefix(&f, 1, 1), "├─ "); // 1002 has sibling 1003 below
        assert_eq!(connector_prefix(&f, 4, 1), "└─ "); // 1003 is the last depth-1 child
    }

    #[test]
    fn ancestor_continuation_draws_vertical_rule() {
        let f = sample();
        // 1004 sits under 1002, which still has sibling 1003 below → guide rule.
        assert_eq!(connector_prefix(&f, 2, 2), "│  ├─ ");
        // 1005 is the last child of 1002 → elbow, but 1002's parent line continues.
        assert_eq!(connector_prefix(&f, 3, 2), "│  └─ ");
        // 1006 under 1003 (the last depth-1 child) → no vertical rule above it.
        assert_eq!(connector_prefix(&f, 5, 2), "   └─ ");
    }

    #[test]
    fn more_above_marker_is_skipped_in_depth_scan() {
        let mut f = vec![TreeRow::MoreAbove];
        f.extend(sample());
        // 1002 is now at index 2; its sibling 1003 is still found below.
        assert_eq!(connector_prefix(&f, 2, 1), "├─ ");
        assert_eq!(connector_prefix(&f, 5, 1), "└─ ");
    }
}
