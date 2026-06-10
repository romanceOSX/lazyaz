use crate::app::App;
use crate::ui::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let Some(w) = &app.wizard else { return };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::ACCENT))
        .title(format!(" First-run setup · step {} of 3 ", w.step + 1));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Length(2), Constraint::Min(5), Constraint::Length(2)])
        .split(inner);

    let hint = match w.step {
        0 => "Select your Azure DevOps organization (type to fuzzy-filter).",
        1 => "Select a project under that organization.",
        _ => "Confirm and finish — press Enter to sign in (mock) and start.",
    };
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(theme::DIM))).wrap(Wrap { trim: true }),
        rows[0],
    );

    match w.step {
        0 => w.org.render(f, rows[1], "Organization"),
        1 => w.project.render(f, rows[1], "Project"),
        _ => {
            let summary = vec![
                Line::from(vec![
                    Span::styled("org:     ", theme::label()),
                    Span::raw(w.org.current().unwrap_or_default()),
                ]),
                Line::from(vec![
                    Span::styled("project: ", theme::label()),
                    Span::raw(w.project.current().unwrap_or_default()),
                ]),
            ];
            f.render_widget(
                Paragraph::new(summary)
                    .block(Block::default().borders(Borders::ALL).title(" Confirm ")),
                rows[1],
            );
        }
    }

    f.render_widget(
        Paragraph::new(Span::styled(
            "type to filter · ↑/↓ select · Enter next · Esc back",
            Style::default().fg(theme::DIM),
        )),
        rows[2],
    );
}
