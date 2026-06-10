//! Floating editor for a work item's tags. Fuzzy-filter the project's known
//! tags, toggle membership, and add brand-new tags by typing them. The result
//! is committed as a single pending edit to the `tags` field on save.

use crate::ui::input::TextInput;
use crate::ui::theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

pub struct TagsEditor {
    /// The item's currently-selected tags (the working set we'll commit).
    pub selected: Vec<String>,
    /// All tags known to the project, for fuzzy autocomplete.
    pub known: Vec<String>,
    pub input: TextInput,
    pub cursor: usize,
}

impl TagsEditor {
    pub fn new(selected: Vec<String>, mut known: Vec<String>) -> Self {
        // Surface the item's own tags in the option list even if the project
        // tag index didn't include them.
        for t in &selected {
            if !known.iter().any(|k| k.eq_ignore_ascii_case(t)) {
                known.push(t.clone());
            }
        }
        known.sort();
        known.dedup();
        Self {
            selected,
            known,
            input: TextInput::default(),
            cursor: 0,
        }
    }

    /// Known tags matching the current query, ranked by fuzzy score.
    pub fn matches(&self) -> Vec<String> {
        crate::ui::fuzzy::rank(&self.known, &self.input.value(), |o| [o.as_str()])
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn is_selected(&self, tag: &str) -> bool {
        self.selected.iter().any(|t| t.eq_ignore_ascii_case(tag))
    }

    fn toggle(&mut self, tag: &str) {
        if let Some(pos) = self
            .selected
            .iter()
            .position(|t| t.eq_ignore_ascii_case(tag))
        {
            self.selected.remove(pos);
        } else {
            self.selected.push(tag.to_string());
        }
    }

    /// Handle a key. Returns `Some(true)` to commit, `Some(false)` to cancel,
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
            // Tab / Ctrl-y autocomplete the highlighted match into the query
            // while searching; with an empty query, Tab commits (and Ctrl-y is
            // a no-op).
            KeyCode::Tab | KeyCode::Char('y')
                if searching && (key.code != KeyCode::Char('y') || ctrl) =>
            {
                if let Some(tag) = self.matches().get(self.cursor).cloned() {
                    self.input = TextInput::new(&tag);
                    self.cursor = 0;
                }
            }
            KeyCode::Enter => {
                // Enter toggles the highlighted match, or adds the typed query as
                // a brand-new tag when it doesn't match an existing option.
                let matches = self.matches();
                if let Some(tag) = matches.get(self.cursor).cloned() {
                    self.toggle(&tag);
                } else {
                    let q = self.input.value();
                    let q = q.trim();
                    if !q.is_empty() {
                        if !self.known.iter().any(|k| k.eq_ignore_ascii_case(q)) {
                            self.known.push(q.to_string());
                            self.known.sort();
                        }
                        self.toggle(q);
                        self.input.clear();
                        self.cursor = 0;
                    }
                }
            }
            KeyCode::Tab => {
                // Save and commit (only reached when not searching).
                return Some(true);
            }
            _ => {
                if self.input.handle(key) {
                    self.cursor = 0;
                }
            }
        }
        None
    }

    /// The committed value: a comma-separated tag string for the pending edit.
    pub fn value(&self) -> String {
        self.selected.join(", ")
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let w = area.width.saturating_sub(area.width / 4).clamp(30, 60).min(area.width);
        let h = area.height.saturating_sub(area.height / 4).clamp(10, 20).min(area.height);
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
            .title(" Tags (^n/^p move · Tab complete · Enter toggle/add · Esc cancel) ");
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

        // Current selection line.
        let mut sel_spans = vec![Span::styled("tags: ", theme::label())];
        if self.selected.is_empty() {
            sel_spans.push(Span::styled("(none)", Style::default().fg(theme::DIM)));
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
            Paragraph::new(Span::styled(
                "─ known tags ─",
                Style::default().fg(theme::DIM),
            )),
            rows[2],
        );

        let matches = self.matches();
        let items: Vec<ListItem> = matches
            .iter()
            .map(|m| {
                let mark = if self.is_selected(m) { "[x] " } else { "[ ] " };
                let style = if self.is_selected(m) {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                };
                ListItem::new(Span::styled(format!("{mark}{m}"), style))
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
