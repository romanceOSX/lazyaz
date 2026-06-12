//! Floating fuzzy multi-select for the iteration (sprint) filter. Type to
//! fuzzy-filter; navigate with `^n`/`^p` or the arrow keys; toggle which
//! iterations to show with Tab or Space; submit with Enter. The list stays in
//! chronological order and the cursor starts on the current iteration so the
//! next/previous sprint is one keystroke away. The current iteration is marked
//! with `● current`.

use crate::api::models::Iteration;
use crate::ui::input::TextInput;
use crate::ui::theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

pub struct IterationPicker {
    /// All known iterations (the option list).
    pub options: Vec<Iteration>,
    /// Selected iteration paths (the working set we'll apply).
    pub selected: Vec<String>,
    pub input: TextInput,
    pub cursor: usize,
}

impl IterationPicker {
    pub fn new(options: Vec<Iteration>, selected: Vec<String>) -> Self {
        // Keep the iterations in their given (chronological) order and start the
        // cursor on the current one, so up/down reach the previous/next sprint.
        let cursor = options.iter().position(|i| i.is_current).unwrap_or(0);
        Self {
            options,
            selected,
            input: TextInput::default(),
            cursor,
        }
    }

    /// Replace the option list (used when the iterations finish loading while
    /// the picker is already open). Keeps the current selection/query; recentres
    /// the cursor on the current iteration when it was empty before.
    pub fn refresh_options(&mut self, options: Vec<Iteration>) {
        let was_empty = self.options.is_empty();
        self.options = options;
        if was_empty {
            self.cursor = self.options.iter().position(|i| i.is_current).unwrap_or(0);
        } else {
            self.cursor = self.cursor.min(self.options.len().saturating_sub(1));
        }
    }

    /// Iterations matching the current query, ranked by fuzzy score (matched on
    /// both the short name and full path).
    pub fn matches(&self) -> Vec<&Iteration> {
        crate::ui::fuzzy::rank(&self.options, &self.input.value(), |it| {
            [it.name.as_str(), it.path.as_str()]
        })
    }

    pub fn is_selected(&self, path: &str) -> bool {
        self.selected.iter().any(|p| p == path)
    }

    fn toggle(&mut self, path: &str) {
        if let Some(pos) = self.selected.iter().position(|p| p == path) {
            self.selected.remove(pos);
        } else {
            self.selected.push(path.to_string());
        }
    }

    /// Handle a key. Returns `Some(true)` to apply, `Some(false)` to cancel,
    /// `None` to stay open.
    pub fn handle(&mut self, key: KeyEvent) -> Option<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return Some(false),
            // Navigate the list: arrows or ^n / ^p.
            KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Char('p') if ctrl => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down => {
                let max = self.matches().len().saturating_sub(1);
                self.cursor = (self.cursor + 1).min(max);
            }
            KeyCode::Char('n') if ctrl => {
                let max = self.matches().len().saturating_sub(1);
                self.cursor = (self.cursor + 1).min(max);
            }
            // Tab or Space toggle the highlighted checkbox.
            KeyCode::Tab | KeyCode::Char(' ') => {
                if let Some(path) = self.matches().get(self.cursor).map(|it| it.path.clone()) {
                    self.toggle(&path);
                }
            }
            // Enter submits the query.
            KeyCode::Enter => return Some(true),
            _ => {
                if self.input.handle(key) {
                    self.cursor = 0;
                }
            }
        }
        None
    }

    /// The committed value: the selected iteration paths.
    pub fn value(&self) -> Vec<String> {
        self.selected.clone()
    }

    pub fn render(&self, f: &mut Frame, area: Rect, loading: bool, spinner: char) {
        let w = area.width.saturating_sub(area.width / 4).clamp(36, 70).min(area.width);
        let h = area.height.saturating_sub(area.height / 4).clamp(10, 22).min(area.height);
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
            .title(" Iterations (^n/^p move · Tab/Space toggle · Enter apply · Esc cancel) ");
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

        // Current selection summary.
        let mut sel_spans = vec![Span::styled("show: ", theme::label())];
        if self.selected.is_empty() {
            sel_spans.push(Span::styled("all iterations", Style::default().fg(theme::DIM)));
        } else {
            sel_spans.push(Span::styled(
                self.selected.join(", "),
                Style::default().fg(Color::Yellow),
            ));
        }
        f.render_widget(Paragraph::new(Line::from(sel_spans)), rows[0]);

        // Query prompt.
        let mut prompt = vec![Span::styled("› ", theme::label())];
        prompt.extend(self.input.spans(Style::default()));
        f.render_widget(Paragraph::new(Line::from(prompt)), rows[1]);

        f.render_widget(
            Paragraph::new(Span::styled("─ iterations ─", Style::default().fg(theme::DIM))),
            rows[2],
        );

        // Nothing to show yet: surface the background fetch instead of a blank box.
        if self.options.is_empty() {
            let msg = if loading {
                Span::styled(
                    format!("{spinner} fetching iterations…"),
                    Style::default().fg(Color::Yellow),
                )
            } else {
                Span::styled("no iterations found", Style::default().fg(theme::DIM))
            };
            f.render_widget(Paragraph::new(Line::from(msg)), rows[3]);
            return;
        }

        let matches = self.matches();
        let items: Vec<ListItem> = matches
            .iter()
            .map(|it| {
                let mark = if self.is_selected(&it.path) { "[x] " } else { "[ ] " };
                let mut spans = vec![Span::raw(mark.to_string())];
                let style = if self.is_selected(&it.path) {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                };
                spans.push(Span::styled(it.name.clone(), style));
                if it.is_current {
                    spans.push(Span::styled(
                        "  ● current",
                        Style::default().fg(Color::Green),
                    ));
                }
                ListItem::new(Line::from(spans))
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

    fn iter(name: &str, current: bool) -> Iteration {
        Iteration {
            path: format!("Proj\\{name}"),
            name: name.to_string(),
            is_current: current,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn cursor_starts_on_current_iteration() {
        // Chronological input: 22, 23, 24(current), 25, 26.
        let opts = vec![
            iter("Sprint 22", false),
            iter("Sprint 23", false),
            iter("Sprint 24", true),
            iter("Sprint 25", false),
            iter("Sprint 26", false),
        ];
        let picker = IterationPicker::new(opts, Vec::new());
        // Order preserved (chronological)…
        let names: Vec<&str> = picker.options.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Sprint 22", "Sprint 23", "Sprint 24", "Sprint 25", "Sprint 26"]
        );
        // …and the cursor sits on the current iteration.
        assert_eq!(picker.cursor, 2);
        assert!(picker.matches()[picker.cursor].is_current);
    }

    #[test]
    fn cursor_defaults_to_zero_without_current() {
        let opts = vec![iter("Sprint 23", false), iter("Sprint 24", false)];
        let picker = IterationPicker::new(opts, Vec::new());
        assert_eq!(picker.cursor, 0);
    }

    #[test]
    fn ctrl_n_p_and_arrows_navigate_the_list() {
        let opts = vec![iter("Sprint 23", true), iter("Sprint 24", false)];
        let mut picker = IterationPicker::new(opts, Vec::new());
        assert_eq!(picker.cursor, 0);
        picker.handle(ctrl('n'));
        assert_eq!(picker.cursor, 1);
        picker.handle(ctrl('p'));
        assert_eq!(picker.cursor, 0);
        picker.handle(key(KeyCode::Down));
        assert_eq!(picker.cursor, 1);
        picker.handle(key(KeyCode::Up));
        assert_eq!(picker.cursor, 0);
    }

    #[test]
    fn tab_and_space_toggle_the_checkbox() {
        let opts = vec![iter("Sprint 23", true), iter("Sprint 24", false)];
        let mut p = IterationPicker::new(opts, Vec::new());
        // Cursor starts on the current iteration (Sprint 23).
        assert_eq!(p.handle(key(KeyCode::Tab)), None);
        assert!(p.is_selected("Proj\\Sprint 23"));
        // Space toggles it back off.
        assert_eq!(p.handle(key(KeyCode::Char(' '))), None);
        assert!(!p.is_selected("Proj\\Sprint 23"));
    }

    #[test]
    fn enter_submits() {
        let opts = vec![iter("Sprint 23", true)];
        let mut p = IterationPicker::new(opts, Vec::new());
        assert_eq!(p.handle(key(KeyCode::Enter)), Some(true));
    }
}
