//! A month-grid calendar for picking a start and end date. Navigate days with
//! the arrow keys / `hjkl`, change month with `[` / `]` (or PageUp/PageDown),
//! and press Enter twice: the first Enter sets the start, the second the end
//! (the pair is ordered automatically). Esc cancels.

use crate::api::models::{month_name, Date};
use crate::ui::theme;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Outcome of feeding a key to the calendar.
pub enum CalResult {
    /// Both ends chosen (ordered `from ≤ to`).
    Done { from: Date, to: Date },
    Cancel,
    Continue,
}

pub struct Calendar {
    /// Highlighted day.
    cursor: Date,
    /// The chosen start; `None` until the first Enter.
    start: Option<Date>,
}

impl Calendar {
    /// Open the calendar with the cursor on `seed` (typically the current
    /// "from" date or today).
    pub fn new(seed: Date) -> Self {
        Self {
            cursor: seed,
            start: None,
        }
    }

    pub fn handle(&mut self, key: KeyEvent) -> CalResult {
        match key.code {
            KeyCode::Esc => return CalResult::Cancel,
            KeyCode::Left | KeyCode::Char('h') => self.cursor = self.cursor.add_days(-1),
            KeyCode::Right | KeyCode::Char('l') => self.cursor = self.cursor.add_days(1),
            KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.add_days(-7),
            KeyCode::Down | KeyCode::Char('j') => self.cursor = self.cursor.add_days(7),
            KeyCode::PageUp | KeyCode::Char('[') => self.cursor = self.cursor.add_months(-1),
            KeyCode::PageDown | KeyCode::Char(']') => self.cursor = self.cursor.add_months(1),
            KeyCode::Enter => match self.start {
                None => self.start = Some(self.cursor),
                Some(start) => {
                    let (from, to) = if start.to_epoch_days() <= self.cursor.to_epoch_days() {
                        (start, self.cursor)
                    } else {
                        (self.cursor, start)
                    };
                    return CalResult::Done { from, to };
                }
            },
            _ => {}
        }
        CalResult::Continue
    }

    fn in_pending_range(&self, day: Date) -> bool {
        let Some(start) = self.start else { return false };
        let (lo, hi) = (
            start.to_epoch_days().min(self.cursor.to_epoch_days()),
            start.to_epoch_days().max(self.cursor.to_epoch_days()),
        );
        let d = day.to_epoch_days();
        d >= lo && d <= hi
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let w = 30u16.min(area.width.saturating_sub(2));
        let h = 12u16.min(area.height);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let rect = Rect::new(x, y, w, h);
        f.render_widget(Clear, rect);

        let title = format!(" {} {} ", month_name(self.cursor.month), self.cursor.year);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT))
            .title(title)
            .title_alignment(Alignment::Center);
        let inner = block.inner(rect);
        f.render_widget(block, rect);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // weekday header
                Constraint::Min(6),    // weeks
                Constraint::Length(1), // hint
            ])
            .split(inner);

        f.render_widget(
            Paragraph::new(Span::styled(
                "Mo Tu We Th Fr Sa Su",
                Style::default().fg(theme::DIM),
            )),
            rows[0],
        );

        // Build the month grid: leading blanks for the 1st's weekday, then days.
        let first = self.cursor.first_of_month();
        let lead = first.weekday_mon0() as usize;
        let dim = self.cursor.days_in_month();
        let mut weeks: Vec<Line> = Vec::new();
        let mut cells: Vec<Span> = vec![Span::raw("   "); lead];
        for day in 1..=dim {
            let date = Date::new(self.cursor.year, self.cursor.month, day);
            let mut style = Style::default();
            if self.in_pending_range(date) {
                style = style.bg(Color::Blue).fg(Color::White);
            }
            if Some(date) == self.start {
                style = style.fg(Color::Green).add_modifier(Modifier::BOLD);
            }
            if date == self.cursor {
                style = style.add_modifier(Modifier::REVERSED);
            }
            cells.push(Span::styled(format!("{day:>2} "), style));
            if cells.len() == 7 {
                weeks.push(Line::from(std::mem::take(&mut cells)));
            }
        }
        if !cells.is_empty() {
            weeks.push(Line::from(cells));
        }
        f.render_widget(Paragraph::new(weeks), rows[1]);

        let hint = if self.start.is_none() {
            "←↑↓→ move · [ ] month · Enter set start · Esc cancel"
        } else {
            "Enter set end · ←↑↓→ move · Esc cancel"
        };
        f.render_widget(
            Paragraph::new(Span::styled(
                hint,
                Style::default().fg(theme::DIM).add_modifier(Modifier::ITALIC),
            )),
            rows[2],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn enter_twice_returns_ordered_range() {
        let mut cal = Calendar::new(Date::new(2026, 6, 10));
        // First Enter sets the start on the 10th.
        assert!(matches!(cal.handle(key(KeyCode::Enter)), CalResult::Continue));
        // Move back three days to the 7th, then Enter to set the end.
        for _ in 0..3 {
            cal.handle(key(KeyCode::Left));
        }
        match cal.handle(key(KeyCode::Enter)) {
            CalResult::Done { from, to } => {
                // Range is ordered even though the end was earlier than the start.
                assert_eq!(from, Date::new(2026, 6, 7));
                assert_eq!(to, Date::new(2026, 6, 10));
            }
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn week_and_month_navigation() {
        let mut cal = Calendar::new(Date::new(2026, 6, 10));
        cal.handle(key(KeyCode::Down)); // +7 days → 17th
        assert_eq!(cal.cursor, Date::new(2026, 6, 17));
        cal.handle(key(KeyCode::Char(']'))); // +1 month → July 17
        assert_eq!(cal.cursor, Date::new(2026, 7, 17));
        cal.handle(key(KeyCode::Char('['))); // back to June 17
        assert_eq!(cal.cursor, Date::new(2026, 6, 17));
    }

    #[test]
    fn esc_cancels() {
        let mut cal = Calendar::new(Date::today());
        assert!(matches!(cal.handle(key(KeyCode::Esc)), CalResult::Cancel));
    }
}
