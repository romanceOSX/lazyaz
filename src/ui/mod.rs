pub mod config_tab;
pub mod detail;
pub mod help;
pub mod info_editor;
pub mod input;
pub mod picker;
pub mod tabs;
pub mod theme;
pub mod tree;
pub mod wizard;
pub mod workitems;

use crate::app::{App, Tab};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tab bar
            Constraint::Min(1),    // body
            Constraint::Length(1), // status bar
        ])
        .split(f.area());

    // Wizard takes over the whole body when active.
    if app.wizard.is_some() {
        tabs::render(f, app, chunks[0]);
        wizard::render(f, app, chunks[1]);
        status_bar(f, app, chunks[2]);
        if app.show_help {
            help::render(f, app);
        }
        return;
    }

    tabs::render(f, app, chunks[0]);
    match app.tab {
        Tab::Tree => tree::render(f, app, chunks[1]),
        Tab::WorkItems => workitems::render(f, app, chunks[1]),
        Tab::Detail => detail::render(f, app, chunks[1]),
        Tab::Config => config_tab::render(f, app, chunks[1]),
    }

    if app.conflict.is_some() {
        conflict_bar(f, app, chunks[2]);
    } else {
        status_bar(f, app, chunks[2]);
    }

    if app.info_editor.is_some() {
        info_editor::render(f, app);
    }
    if app.show_help {
        help::render(f, app);
    }
}

/// A loud, pulsating prompt shown while an edit conflict is unresolved.
fn conflict_bar(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let red = if app.pulse() { Color::Red } else { Color::LightRed };
    let line = Line::from(vec![
        Span::styled(
            " CONFLICT ",
            Style::default().fg(Color::Black).bg(red),
        ),
        Span::raw(" "),
        Span::styled(
            "someone changed this item — [m] merge in editor   [f] force-push your changes",
            Style::default().fg(red),
        ),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn status_bar(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let line = Line::from(vec![
        Span::styled(" ? help ", theme::active_tab()),
        Span::raw(" "),
        Span::styled(&app.status, Style::default().fg(theme::ACCENT)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}
