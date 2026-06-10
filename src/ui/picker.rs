//! fzf-style chooser: a text input on top that fuzzy-filters a list of options.
//! Used by the first-run wizard (org / project selection) and shareable by any
//! future "pick one of N" flow.

use crate::ui::input::TextInput;
use crate::ui::theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

pub struct Picker {
    pub input: TextInput,
    pub options: Vec<String>,
    pub selected: usize,
}

impl Picker {
    pub fn new(options: Vec<String>) -> Self {
        Self {
            input: TextInput::default(),
            options,
            selected: 0,
        }
    }

    pub fn set_options(&mut self, options: Vec<String>) {
        self.options = options;
        self.selected = 0;
        self.input.clear();
    }

    /// Options matching the current query, ranked by fuzzy score.
    pub fn matches(&self) -> Vec<&String> {
        crate::ui::fuzzy::rank(&self.options, &self.input.value(), |o| [o.as_str()])
    }

    pub fn current(&self) -> Option<String> {
        self.matches().get(self.selected).map(|s| s.to_string())
    }

    /// Returns true if the key was handled (movement or text editing).
    pub fn handle(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                true
            }
            KeyCode::Down => {
                let max = self.matches().len().saturating_sub(1);
                self.selected = (self.selected + 1).min(max);
                true
            }
            KeyCode::Char('p') if ctrl => {
                self.selected = self.selected.saturating_sub(1);
                true
            }
            KeyCode::Char('n') if ctrl => {
                let max = self.matches().len().saturating_sub(1);
                self.selected = (self.selected + 1).min(max);
                true
            }
            // Tab / Ctrl-y autocomplete the highlighted option into the query,
            // but only while in search mode (non-empty query); otherwise Tab
            // falls through to the default (e.g. switch tab).
            KeyCode::Tab | KeyCode::Char('y')
                if (key.code != KeyCode::Char('y') || ctrl)
                    && !self.input.value().trim().is_empty() =>
            {
                if let Some(opt) = self.current() {
                    self.input = TextInput::new(&opt);
                    self.selected = 0;
                }
                true
            }
            _ => {
                let handled = self.input.handle(key);
                if handled {
                    self.selected = 0;
                }
                handled
            }
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect, title: &str) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT))
            .title(format!(" {title} "));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(inner);

        let mut prompt = vec![Span::styled("› ", theme::label())];
        prompt.extend(self.input.spans(Style::default()));
        f.render_widget(Paragraph::new(Line::from(prompt)), rows[0]);

        let matches = self.matches();
        let items: Vec<ListItem> = matches
            .iter()
            .map(|m| ListItem::new(Span::raw((*m).clone())))
            .collect();
        let mut state = ListState::default();
        if !matches.is_empty() {
            state.select(Some(self.selected.min(matches.len() - 1)));
        }
        f.render_stateful_widget(
            List::new(items)
                .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
                .highlight_symbol("» "),
            rows[1],
            &mut state,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn ctrl_n_p_navigate() {
        let mut p = Picker::new(vec!["alpha".into(), "beta".into(), "gamma".into()]);
        assert_eq!(p.selected, 0);
        p.handle(ctrl('n'));
        assert_eq!(p.selected, 1);
        p.handle(ctrl('p'));
        assert_eq!(p.selected, 0);
    }

    #[test]
    fn tab_and_ctrl_y_autocomplete_while_searching() {
        let mut p = Picker::new(vec!["alpha".into(), "beta".into()]);
        p.handle(key(KeyCode::Char('a')));
        p.handle(key(KeyCode::Char('l')));
        p.handle(key(KeyCode::Tab));
        assert_eq!(p.input.value(), "alpha");

        let mut p2 = Picker::new(vec!["alpha".into(), "beta".into()]);
        p2.handle(key(KeyCode::Char('b')));
        p2.handle(ctrl('y'));
        assert_eq!(p2.input.value(), "beta");
    }

    #[test]
    fn tab_falls_through_when_query_empty() {
        let mut p = Picker::new(vec!["alpha".into()]);
        // Not consumed by the picker → caller handles it (e.g. switch tab).
        assert!(!p.handle(key(KeyCode::Tab)));
    }
}
