//! Floating window for the timeframe filter: a **Start date** and an **End
//! date**, each independently on/off so the range can be open-ended (start-only
//! = "on or after", end-only = "on or before"). Within a date, move between the
//! year / month / day fields with `h`/`l` (or ←/→); change the focused field
//! with `k`/↑ (increase) and `j`/↓ (decrease) or by typing digits; toggle a
//! bound on/off with Space; or press `c` to pick the whole range on a calendar.

use crate::api::models::{Date, Timeframe};
use crate::ui::calendar::{CalResult, Calendar};
use crate::ui::theme;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

#[derive(Clone, Copy, PartialEq)]
enum Part {
    Year,
    Month,
    Day,
}

const PARTS: [Part; 3] = [Part::Year, Part::Month, Part::Day];

struct DateField {
    date: Date,
    enabled: bool,
}

pub struct DateRangeInput {
    start: DateField,
    end: DateField,
    /// Focused row: 0 = start, 1 = end.
    row: usize,
    /// Focused part within the row.
    part: usize,
    /// Digits typed into the focused field since it was focused (manual entry).
    buf: String,
    error: bool,
    calendar: Option<Calendar>,
}

impl DateRangeInput {
    /// Seed from the current timeframe: enable whichever bounds are set; an empty
    /// window starts with both bounds enabled on today.
    pub fn new(current: Timeframe) -> Self {
        let today = Date::today();
        Self {
            start: DateField {
                date: current.from.unwrap_or(today),
                enabled: current.from.is_some() || current.is_empty(),
            },
            end: DateField {
                date: current.to.unwrap_or(today),
                enabled: current.to.is_some() || current.is_empty(),
            },
            row: 0,
            part: 0,
            buf: String::new(),
            error: false,
            calendar: None,
        }
    }

    fn field_mut(&mut self) -> &mut DateField {
        if self.row == 0 {
            &mut self.start
        } else {
            &mut self.end
        }
    }

    /// Commit any typed digits into the focused field's date.
    fn commit_buf(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let n: u32 = self.buf.parse().unwrap_or(0);
        let part = PARTS[self.part];
        let f = self.field_mut();
        f.date = set_part(f.date, part, n as i64);
        self.buf.clear();
    }

    /// Step the focused field by `delta` (with wraparound / day clamping).
    fn adjust(&mut self, delta: i64) {
        self.commit_buf();
        let part = PARTS[self.part];
        let f = self.field_mut();
        f.date = match part {
            Part::Year => clamp_day(Date::new(f.date.year + delta as i32, f.date.month, f.date.day)),
            Part::Month => f.date.add_months(delta as i32),
            Part::Day => f.date.add_days(delta),
        };
        self.error = false;
    }

    fn move_focus(&mut self, delta: isize) {
        self.commit_buf();
        let idx = (self.row * 3 + self.part) as isize;
        let next = (idx + delta).rem_euclid(6) as usize;
        self.row = next / 3;
        self.part = next % 3;
    }

    /// Some(true) to apply, Some(false) to cancel, None to stay open.
    pub fn handle(&mut self, key: KeyEvent) -> Option<bool> {
        if let Some(cal) = &mut self.calendar {
            match cal.handle(key) {
                CalResult::Done { from, to } => {
                    self.start = DateField { date: from, enabled: true };
                    self.end = DateField { date: to, enabled: true };
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
                self.commit_buf();
                if self.value().is_some() {
                    return Some(true);
                }
                self.error = true;
            }
            KeyCode::Left | KeyCode::Char('h') => self.move_focus(-1),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => self.move_focus(1),
            KeyCode::Up | KeyCode::Char('k') => self.adjust(1),
            KeyCode::Down | KeyCode::Char('j') => self.adjust(-1),
            KeyCode::Char(' ') => {
                self.commit_buf();
                self.field_mut().enabled = !self.field_mut().enabled;
            }
            KeyCode::Char('c') => {
                self.commit_buf();
                let seed = if self.start.enabled {
                    self.start.date
                } else {
                    Date::today()
                };
                self.calendar = Some(Calendar::new(seed));
            }
            KeyCode::Backspace => {
                self.buf.clear();
            }
            KeyCode::Char(d @ '0'..='9') => {
                let max = if PARTS[self.part] == Part::Year { 4 } else { 2 };
                if self.buf.len() >= max {
                    self.buf.clear();
                }
                self.buf.push(d);
                // Preview the typed value immediately.
                let n: u32 = self.buf.parse().unwrap_or(0);
                let part = PARTS[self.part];
                let f = self.field_mut();
                f.date = set_part(f.date, part, n as i64);
                self.error = false;
            }
            _ => {}
        }
        None
    }

    /// The parsed timeframe, or `None` if both bounds are set but out of order.
    pub fn value(&self) -> Option<Timeframe> {
        let from = self.start.enabled.then_some(self.start.date);
        let to = self.end.enabled.then_some(self.end.date);
        if let (Some(f), Some(t)) = (from, to)
            && f.to_epoch_days() > t.to_epoch_days() {
                return None;
            }
        Some(Timeframe { from, to })
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let w = 44u16.min(area.width.saturating_sub(2));
        let h = 8u16.min(area.height);
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let rect = Rect::new(x, y, w, h);
        f.render_widget(Clear, rect);

        let body = vec![
            self.date_line("Start", &self.start, 0),
            self.date_line("End  ", &self.end, 1),
            Line::from(""),
            Line::from(Span::styled(
                if self.error {
                    "invalid range (start must be ≤ end)".to_string()
                } else {
                    "h/l field · j/k or type · Space on/off · c calendar · Enter apply".to_string()
                },
                Style::default().fg(if self.error { Color::Red } else { theme::DIM }),
            )),
        ];
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT))
            .title(" Timeframe filter ")
            .title_alignment(Alignment::Center);
        f.render_widget(Paragraph::new(body).block(block), rect);

        if let Some(cal) = &self.calendar {
            cal.render(f, area);
        }
    }

    fn date_line(&self, label: &str, field: &DateField, row: usize) -> Line<'static> {
        let mut spans = vec![
            Span::styled(format!("{label}  "), theme::label()),
            Span::raw(if field.enabled { "[x] " } else { "[ ] " }),
        ];
        if !field.enabled {
            spans.push(Span::styled("(any)", Style::default().fg(theme::DIM)));
            return Line::from(spans);
        }
        let focused = self.row == row;
        let part_span = |p: usize, text: String| {
            let mut s = Style::default();
            if focused && self.part == p {
                s = s.add_modifier(Modifier::REVERSED);
            }
            Span::styled(text, s)
        };
        spans.push(part_span(0, format!("{:04}", field.date.year)));
        spans.push(Span::raw("-"));
        spans.push(part_span(1, format!("{:02}", field.date.month)));
        spans.push(Span::raw("-"));
        spans.push(part_span(2, format!("{:02}", field.date.day)));
        Line::from(spans)
    }
}

/// Clamp a date's day to its month length (after a year/month change).
fn clamp_day(d: Date) -> Date {
    let dim = Date::new(d.year, d.month, 1).days_in_month();
    Date::new(d.year, d.month, d.day.min(dim).max(1))
}

/// Set one Y/M/D component of `date` to `n` (clamped to a valid value).
fn set_part(date: Date, part: Part, n: i64) -> Date {
    match part {
        Part::Year => clamp_day(Date::new((n as i32).clamp(1, 9999), date.month, date.day)),
        Part::Month => {
            let m = (n as u32).clamp(1, 12);
            clamp_day(Date::new(date.year, m, date.day))
        }
        Part::Day => {
            let dim = date.days_in_month();
            Date::new(date.year, date.month, (n as u32).clamp(1, dim))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn empty() -> DateRangeInput {
        DateRangeInput::new(Timeframe::default())
    }

    #[test]
    fn space_toggles_open_ended_bounds() {
        let mut d = empty();
        // Focus starts on the start row; disable it → end-only ("≤") window.
        d.handle(key(KeyCode::Char(' ')));
        let tf = d.value().unwrap();
        assert!(tf.from.is_none() && tf.to.is_some());
    }

    #[test]
    fn jk_adjust_focused_field() {
        let mut d = empty();
        let y0 = d.start.date.year;
        d.handle(key(KeyCode::Char('k'))); // increase year
        assert_eq!(d.start.date.year, y0 + 1);
        d.handle(key(KeyCode::Char('j'))); // decrease year
        assert_eq!(d.start.date.year, y0);
    }

    #[test]
    fn hl_move_between_fields_and_rows() {
        let mut d = empty();
        assert_eq!((d.row, d.part), (0, 0));
        d.handle(key(KeyCode::Char('l'))); // → month
        assert_eq!((d.row, d.part), (0, 1));
        d.handle(key(KeyCode::Char('h'))); // ← year
        assert_eq!((d.row, d.part), (0, 0));
        d.handle(key(KeyCode::Char('h'))); // wrap to end.day
        assert_eq!((d.row, d.part), (1, 2));
    }

    #[test]
    fn typing_digits_sets_the_field() {
        let mut d = empty();
        d.move_focus(1); // start.month
        for c in "12".chars() {
            d.handle(key(KeyCode::Char(c)));
        }
        assert_eq!(d.start.date.month, 12);
    }

    #[test]
    fn out_of_order_range_is_invalid() {
        let mut d = empty();
        // start.year + 1 makes start later than end → invalid.
        d.handle(key(KeyCode::Char('k')));
        assert!(d.value().is_none());
        assert_eq!(d.handle(key(KeyCode::Enter)), None); // refuses to apply
        assert!(d.error);
    }
}
