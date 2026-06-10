use ratatui::style::{Color, Modifier, Style};

pub const ACCENT: Color = Color::Cyan;
pub const DIM: Color = Color::DarkGray;

pub fn active_tab() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub fn inactive_tab() -> Style {
    Style::default().fg(DIM)
}

pub fn selected_row() -> Style {
    Style::default()
        .fg(ACCENT)
        .add_modifier(Modifier::BOLD | Modifier::REVERSED)
}

pub fn label() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

