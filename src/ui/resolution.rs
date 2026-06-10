//! Floating "resolution-options" menu. Spawned only on genuine divergence —
//! when a manual push (or an attempt to edit a live-feed–flagged field) finds
//! that the server value changed in a way that conflicts with a local edit.
//! Lists the conflicting fields; `j/k` selects, `m` merges the selected field in
//! `$EDITOR`, `f` force-pushes all local changes, `Esc` cancels.

use crate::app::App;
use crate::ui::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
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

fn truncate(s: &str, max: usize) -> String {
    let oneline = s.replace('\n', " ⏎ ");
    if oneline.chars().count() > max {
        let mut out: String = oneline.chars().take(max).collect();
        out.push('…');
        out
    } else {
        oneline
    }
}

pub fn render(f: &mut Frame, app: &App) {
    let Some(res) = &app.resolution else { return };

    let red = if app.pulse() { Color::Red } else { Color::LightRed };
    let area = centered(f.area(), 72, 60);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(red))
        .title(format!(
            " ⚠ Conflicts on #{} — resolve before pushing ",
            res.id
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let items: Vec<ListItem> = res
        .conflicts
        .iter()
        .map(|c| {
            let lines = vec![
                Line::from(Span::styled(
                    c.field,
                    Style::default()
                        .fg(theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(vec![
                    Span::styled("  yours:  ", theme::label()),
                    Span::styled(truncate(&c.local, 48), Style::default().fg(Color::Green)),
                ]),
                Line::from(vec![
                    Span::styled("  theirs: ", theme::label()),
                    Span::styled(truncate(&c.remote, 48), Style::default().fg(red)),
                ]),
            ];
            ListItem::new(lines)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(res.selected));
    f.render_stateful_widget(
        List::new(items)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("› "),
        rows[0],
        &mut state,
    );

    let hint = "j/k select · m merge selected ($EDITOR) · f force-push all mine · Esc cancel";
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(theme::DIM))),
        rows[1],
    );
}
