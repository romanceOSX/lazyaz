pub mod calendar;
pub mod config_tab;
pub mod detail;
pub mod help;
pub mod date_range;
pub mod fuzzy;
pub mod info_editor;
pub mod input;
pub mod iteration_picker;
pub mod picker;
pub mod resolution;
pub mod search;
pub mod tabs;
pub mod tags_editor;
pub mod theme;
pub mod tree;
pub mod type_filter;
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

    status_bar(f, app, chunks[2]);

    if app.info_editor.is_some() {
        info_editor::render(f, app);
    }
    // The state picker / tags editor float above the field editor.
    if let Some(picker) = &app.state_picker {
        let area = centered_rect(40, 14, f.area());
        f.render_widget(ratatui::widgets::Clear, area);
        picker.render(f, area, "State (Enter select · Esc cancel)");
    }
    if let Some(tags) = &app.tags_editor {
        tags.render(f, f.area());
    }
    if let Some(picker) = &app.iteration_picker {
        picker.render(f, f.area());
    }
    if let Some(dr) = &app.date_range {
        dr.render(f, f.area());
    }
    if let Some(picker) = &app.type_picker {
        picker.render(f, f.area());
    }
    // The resolution-options menu floats above everything else.
    if app.resolution.is_some() {
        resolution::render(f, app);
    }
    if app.show_help {
        help::render(f, app);
    }
}

fn status_bar(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let mut spans = vec![
        Span::styled(" ? help ", theme::active_tab()),
        Span::raw(" "),
    ];
    if app.is_loading() {
        spans.push(Span::styled(
            format!("{} ", app.spinner_frame()),
            Style::default().fg(Color::Yellow),
        ));
    }
    // A background push shows its own spinner so the user knows it's in flight
    // while they keep navigating.
    if app.is_pushing() {
        spans.push(Span::styled(
            format!("{} pushing… ", app.spinner_frame()),
            Style::default().fg(Color::Cyan),
        ));
    }
    // Tree nodes loading in the background.
    if app.tree_loading() {
        spans.push(Span::styled(
            format!("{} tree… ", app.spinner_frame()),
            Style::default().fg(Color::Yellow),
        ));
    }
    // Un-pushed local edits: show a ● badge and prompt to push.
    if app.has_pending() {
        spans.push(Span::styled(
            format!("● {} unpushed ", app.pending_count()),
            Style::default().fg(Color::Yellow),
        ));
        spans.push(Span::styled(
            "[p] push  ",
            Style::default().fg(Color::DarkGray),
        ));
    }
    // Active time filter (iteration- OR timeframe-based — mutually exclusive)
    // and the type filter, shown as chips.
    spans.push(Span::styled(
        format!(" {} ", app.time_filter_label()),
        Style::default().fg(Color::Magenta),
    ));
    spans.push(Span::styled(
        format!("{} ", app.type_filter_label()),
        Style::default().fg(Color::Green),
    ));
    spans.push(Span::styled(&app.status, Style::default().fg(theme::ACCENT)));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// A centred rectangle `width`×`height` (in columns/rows), clamped to `area`.
fn centered_rect(width: u16, height: u16, area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    ratatui::layout::Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}
