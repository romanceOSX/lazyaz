//! A small two-field modal for entering a custom `[from, to]` change-date
//! window for the timeframe filter. Both fields accept `YYYY-MM-DD` typed in
//! place, or press `c` to pick the range on a calendar.

use crate::api::models::{Date, Timeframe};
use crate::ui::calendar::{CalResult, Calendar};
use crate::ui::input::TextInput;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

pub struct DateRangeInput {
    from: TextInput,
    to: TextInput,
    /// 0 = from, 1 = to.
    focus: usize,
    error: bool,
    /// Active calendar overlay (opened with `c`).
    calendar: Option<Calendar>,
}

impl DateRangeInput {
    /// Seed from an existing timeframe (a `Custom` range pre-fills both fields;
    /// otherwise both default to today).
    pub fn new(current: Timeframe) -> Self {
        let (from, to) = match current {
            Timeframe::Custom { from, to } => (from.to_iso(), to.to_iso()),
            _ => {
                let today = Date::today().to_iso();
                (today.clone(), today)
            }
        };
        Self {
            from: TextInput::new(&from),
            to: TextInput::new(&to),
            focus: 0,
            error: false,
            calendar: None,
        }
    }

    /// The parsed timeframe, if both fields hold valid dates in order.
    pub fn value(&self) -> Option<Timeframe> {
        let from = Date::from_iso(&self.from.value())?;
        let to = Date::from_iso(&self.to.value())?;
        if from.to_epoch_days() > to.to_epoch_days() {
            return None;
        }
        Some(Timeframe::Custom { from, to })
    }

    /// Returns `Some(true)` to apply, `Some(false)` to cancel, `None` to stay.
    pub fn handle(&mut self, key: KeyEvent) -> Option<bool> {
        // While the calendar is open, all keys go to it.
        if let Some(cal) = &mut self.calendar {
            match cal.handle(key) {
                CalResult::Done { from, to } => {
                    self.from = TextInput::new(&from.to_iso());
                    self.to = TextInput::new(&to.to_iso());
                    self.error = false;
                    self.calendar = None;
                }
                CalResult::Cancel => self.calendar = None,
                CalResult::Continue => {}
            }
            return None;
        }
        match key.code {
            KeyCode::Esc => return Some(false),
            KeyCode::Enter => {
                if self.value().is_some() {
                    return Some(true);
                }
                self.error = true;
            }
            KeyCode::Tab | KeyCode::Down | KeyCode::Up => {
                self.focus = 1 - self.focus;
            }
            // `c` opens the calendar, seeded on the current "from" date (or today).
            KeyCode::Char('c') | KeyCode::Char('C') => {
                let seed = Date::from_iso(&self.from.value()).unwrap_or_else(Date::today);
                self.calendar = Some(Calendar::new(seed));
            }
            _ => {
                let field = if self.focus == 0 {
                    &mut self.from
                } else {
                    &mut self.to
                };
                if field.handle(key) {
                    self.error = false;
                }
            }
        }
        None
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let w = 40u16.min(area.width.saturating_sub(2));
        let h = 7u16.min(area.height);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let rect = Rect::new(x, y, w, h);
        f.render_widget(Clear, rect);

        let base = Style::default().fg(Color::White);
        let label = Style::default().fg(Color::DarkGray);
        let active = Style::default().fg(Color::Yellow);

        let from_lbl = if self.focus == 0 { active } else { label };
        let to_lbl = if self.focus == 1 { active } else { label };

        let mut from_spans = vec![ratatui::text::Span::styled("from ", from_lbl)];
        from_spans.extend(self.from.spans(base));
        let mut to_spans = vec![ratatui::text::Span::styled("to   ", to_lbl)];
        to_spans.extend(self.to.spans(base));

        let hint = if self.error {
            Line::from(ratatui::text::Span::styled(
                "invalid range (YYYY-MM-DD, from ≤ to)",
                Style::default().fg(Color::Red),
            ))
        } else {
            Line::from(ratatui::text::Span::styled(
                "Tab switch · c calendar · Enter apply · Esc cancel",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ))
        };

        let body = vec![
            Line::from(from_spans),
            Line::from(to_spans),
            Line::from(""),
            hint,
        ];
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Custom timeframe ")
            .title_alignment(Alignment::Center);
        f.render_widget(Paragraph::new(body).block(block), rect);

        // The calendar overlays the whole area when open.
        if let Some(cal) = &self.calendar {
            cal.render(f, area);
        }
    }
}
