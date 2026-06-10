//! Floating multi-select for the work-item *type* filter on the Work Items
//! window (Task / User Story / Feature / Capability / Epic). Navigate with
//! `^n`/`^p` (or arrows), toggle with Space/Enter, and apply with Tab (when the
//! query is empty). While searching, Tab/`^y` autocomplete the highlighted type
//! into the query.

use crate::ui::input::TextInput;
use crate::ui::theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

pub struct TypeFilter {
    /// All selectable work-item types (the option list).
    pub options: Vec<String>,
    /// Selected type names (the working set we'll apply).
    pub selected: Vec<String>,
    pub input: TextInput,
    pub cursor: usize,
}

impl TypeFilter {
    pub fn new(options: Vec<String>, selected: Vec<String>) -> Self {
        Self {
            options,
            selected,
            input: TextInput::default(),
            cursor: 0,
        }
    }

    /// Types matching the current query, ranked by fuzzy score.
    pub fn matches(&self) -> Vec<&String> {
        crate::ui::fuzzy::rank(&self.options, &self.input.value(), |o| [o.as_str()])
    }

    pub fn is_selected(&self, name: &str) -> bool {
        self.selected.iter().any(|t| t.eq_ignore_ascii_case(name))
    }

    fn toggle(&mut self, name: &str) {
        if let Some(pos) = self.selected.iter().position(|t| t.eq_ignore_ascii_case(name)) {
            self.selected.remove(pos);
        } else {
            self.selected.push(name.to_string());
        }
    }

    /// Handle a key. Returns `Some(true)` to apply, `Some(false)` to cancel,
    /// `None` to stay open.
    pub fn handle(&mut self, key: KeyEvent) -> Option<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let searching = !self.input.value().trim().is_empty();
        match key.code {
            KeyCode::Esc => return Some(false),
            KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down => {
                let max = self.matches().len().saturating_sub(1);
                self.cursor = (self.cursor + 1).min(max);
            }
            KeyCode::Char('p') if ctrl => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Char('n') if ctrl => {
                let max = self.matches().len().saturating_sub(1);
                self.cursor = (self.cursor + 1).min(max);
            }
            // Tab / Ctrl-y autocomplete the highlighted type into the query
            // while searching; with an empty query, Tab applies the selection
            // (and Ctrl-y is a no-op).
            KeyCode::Tab | KeyCode::Char('y')
                if searching && (key.code != KeyCode::Char('y') || ctrl) =>
            {
                if let Some(name) = self.matches().get(self.cursor).map(|t| (*t).clone()) {
                    self.input = TextInput::new(&name);
                    self.cursor = 0;
                }
            }
            KeyCode::Tab => return Some(true),
            KeyCode::Char(' ') | KeyCode::Enter => {
                if let Some(name) = self.matches().get(self.cursor).map(|t| (*t).clone()) {
                    self.toggle(&name);
                }
            }
            _ => {
                if self.input.handle(key) {
                    self.cursor = 0;
                }
            }
        }
        None
    }

    /// The committed value: the selected type names.
    pub fn value(&self) -> Vec<String> {
        self.selected.clone()
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let w = area.width.saturating_sub(area.width / 4).clamp(36, 60).min(area.width);
        let h = area.height.saturating_sub(area.height / 4).clamp(9, 16).min(area.height);
        let rect = Rect {
            x: area.x + (area.width.saturating_sub(w)) / 2,
            y: area.y + (area.height.saturating_sub(h)) / 2,
            width: w,
            height: h,
        };
        f.render_widget(Clear, rect);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT))
            .title(" Item types (^n/^p move · Tab complete · Space toggle · Esc cancel) ");
        let inner = block.inner(rect);
        f.render_widget(block, rect);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .split(inner);

        let mut sel_spans = vec![Span::styled("show: ", theme::label())];
        if self.selected.is_empty() {
            sel_spans.push(Span::styled("all types", Style::default().fg(theme::DIM)));
        } else {
            sel_spans.push(Span::styled(
                self.selected.join(", "),
                Style::default().fg(Color::Yellow),
            ));
        }
        f.render_widget(Paragraph::new(Line::from(sel_spans)), rows[0]);

        let mut prompt = vec![Span::styled("› ", theme::label())];
        prompt.extend(self.input.spans(Style::default()));
        f.render_widget(Paragraph::new(Line::from(prompt)), rows[1]);

        f.render_widget(
            Paragraph::new(Span::styled("─ types ─", Style::default().fg(theme::DIM))),
            rows[2],
        );

        let matches = self.matches();
        let items: Vec<ListItem> = matches
            .iter()
            .map(|name| {
                let mark = if self.is_selected(name) { "[x] " } else { "[ ] " };
                let style = if self.is_selected(name) {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(vec![
                    Span::raw(mark.to_string()),
                    Span::styled((*name).clone(), style),
                ]))
            })
            .collect();
        let mut state = ListState::default();
        if !matches.is_empty() {
            state.select(Some(self.cursor.min(matches.len() - 1)));
        }
        f.render_stateful_widget(
            List::new(items)
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                .highlight_symbol("» "),
            rows[3],
            &mut state,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{DEFAULT_WORK_ITEM_TYPES, WORK_ITEM_TYPES};

    fn opts() -> Vec<String> {
        WORK_ITEM_TYPES.iter().map(|s| s.to_string()).collect()
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn toggles_selection_with_space() {
        let mut f = TypeFilter::new(opts(), Vec::new());
        // Cursor starts on "Task".
        f.handle(key(KeyCode::Char(' ')));
        assert!(f.is_selected("Task"));
        f.handle(key(KeyCode::Char(' ')));
        assert!(!f.is_selected("Task"));
    }

    #[test]
    fn defaults_round_trip() {
        let selected: Vec<String> = DEFAULT_WORK_ITEM_TYPES.iter().map(|s| s.to_string()).collect();
        let f = TypeFilter::new(opts(), selected);
        assert!(f.is_selected("User Story"));
        assert!(f.is_selected("Feature"));
        assert!(!f.is_selected("Epic"));
    }

    #[test]
    fn ctrl_n_p_navigate_and_tab_applies_when_empty() {
        let mut f = TypeFilter::new(opts(), Vec::new());
        f.handle(ctrl('n'));
        assert_eq!(f.cursor, 1);
        f.handle(ctrl('p'));
        assert_eq!(f.cursor, 0);
        assert_eq!(f.handle(key(KeyCode::Tab)), Some(true));
    }

    #[test]
    fn tab_autocompletes_while_searching() {
        let mut f = TypeFilter::new(opts(), Vec::new());
        for c in "epi".chars() {
            f.handle(key(KeyCode::Char(c)));
        }
        assert_eq!(f.handle(key(KeyCode::Tab)), None);
        assert_eq!(f.input.value(), "Epic");
    }
}
