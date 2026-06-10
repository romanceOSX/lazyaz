//! fzf-style chooser: a text input on top that fuzzy-filters a list of options.
//! Used by the first-run wizard (org / project selection) and shareable by any
//! future "pick one of N" flow.

use crate::ui::input::TextInput;
use crate::ui::theme;
use crossterm::event::{KeyCode, KeyEvent};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
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
        let query = self.input.value();
        if query.trim().is_empty() {
            return self.options.iter().collect();
        }
        let matcher = SkimMatcherV2::default();
        let mut scored: Vec<(i64, &String)> = self
            .options
            .iter()
            .filter_map(|o| matcher.fuzzy_match(o, &query).map(|s| (s, o)))
            .collect();
        scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
        scored.into_iter().map(|(_, o)| o).collect()
    }

    pub fn current(&self) -> Option<String> {
        self.matches().get(self.selected).map(|s| s.to_string())
    }

    /// Returns true if the key was handled (movement or text editing).
    pub fn handle(&mut self, key: KeyEvent) -> bool {
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
