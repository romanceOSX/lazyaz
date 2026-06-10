//! Floating fuzzy multi-select for the iteration (sprint) filter. Fuzzy-filter
//! the team's iterations, navigate with `^n`/`^p` (or arrows), toggle which
//! ones to show with Space/Enter, and apply with Tab (when the query is empty).
//! While searching, Tab/`^y` autocomplete the highlighted iteration into the
//! query. The current iteration is marked with `● current`.

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
        Self {
            options: Self::order_relative_to_current(options),
            selected,
            input: TextInput::default(),
            cursor: 0,
        }
    }

    /// Order iterations relative to the current one: the current iteration
    /// first, then the rest by proximity to it in the (chronological) list,
    /// preferring upcoming sprints over past ones at equal distance. If no
    /// iteration is marked current, the original order is preserved.
    fn order_relative_to_current(options: Vec<Iteration>) -> Vec<Iteration> {
        let Some(current) = options.iter().position(|i| i.is_current) else {
            return options;
        };
        let mut ordered: Vec<(usize, Iteration)> = options.into_iter().enumerate().collect();
        ordered.sort_by_key(|(idx, _)| {
            let delta = *idx as i64 - current as i64;
            // Primary: absolute distance from current; secondary: future before
            // past (a future sprint has delta > 0, so it sorts first on ties).
            (delta.abs(), -delta)
        });
        ordered.into_iter().map(|(_, it)| it).collect()
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
            // Tab / Ctrl-y autocomplete the highlighted iteration into the query
            // while searching; with an empty query, Tab applies the selection
            // (and Ctrl-y is a no-op).
            KeyCode::Tab | KeyCode::Char('y')
                if searching && (key.code != KeyCode::Char('y') || ctrl) =>
            {
                if let Some(name) = self.matches().get(self.cursor).map(|it| it.name.clone()) {
                    self.input = TextInput::new(&name);
                    self.cursor = 0;
                }
            }
            KeyCode::Tab => return Some(true),
            KeyCode::Char(' ') | KeyCode::Enter => {
                if let Some(path) = self.matches().get(self.cursor).map(|it| it.path.clone()) {
                    self.toggle(&path);
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

    /// The committed value: the selected iteration paths.
    pub fn value(&self) -> Vec<String> {
        self.selected.clone()
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
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
            .title(" Iterations (^n/^p move · Tab complete · Space toggle · Esc cancel) ");
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

    #[test]
    fn orders_iterations_relative_to_current() {
        // Chronological input: 22, 23, 24(current), 25, 26.
        let opts = vec![
            iter("Sprint 22", false),
            iter("Sprint 23", false),
            iter("Sprint 24", true),
            iter("Sprint 25", false),
            iter("Sprint 26", false),
        ];
        let picker = IterationPicker::new(opts, Vec::new());
        let names: Vec<&str> = picker.options.iter().map(|i| i.name.as_str()).collect();
        // Current first, then nearest neighbours (future before past on ties).
        assert_eq!(
            names,
            vec!["Sprint 24", "Sprint 25", "Sprint 23", "Sprint 26", "Sprint 22"]
        );
    }

    #[test]
    fn preserves_order_when_no_current() {
        let opts = vec![iter("Sprint 23", false), iter("Sprint 24", false)];
        let picker = IterationPicker::new(opts, Vec::new());
        let names: Vec<&str> = picker.options.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["Sprint 23", "Sprint 24"]);
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn ctrl_n_p_navigate_the_list() {
        let opts = vec![iter("Sprint 23", true), iter("Sprint 24", false)];
        let mut picker = IterationPicker::new(opts, Vec::new());
        assert_eq!(picker.cursor, 0);
        picker.handle(ctrl('n'));
        assert_eq!(picker.cursor, 1);
        picker.handle(ctrl('p'));
        assert_eq!(picker.cursor, 0);
    }

    #[test]
    fn tab_and_ctrl_y_autocomplete_while_searching() {
        let opts = vec![iter("Sprint 23", true), iter("Backlog", false)];
        // Tab fills the query with the highlighted match and does not apply.
        let mut p = IterationPicker::new(opts.clone(), Vec::new());
        for c in "spr".chars() {
            p.handle(key(KeyCode::Char(c)));
        }
        assert_eq!(p.handle(key(KeyCode::Tab)), None);
        assert_eq!(p.input.value(), "Sprint 23");
        // Ctrl-y does the same.
        let mut p2 = IterationPicker::new(opts, Vec::new());
        for c in "spr".chars() {
            p2.handle(key(KeyCode::Char(c)));
        }
        assert_eq!(p2.handle(ctrl('y')), None);
        assert_eq!(p2.input.value(), "Sprint 23");
    }

    #[test]
    fn tab_applies_when_query_empty() {
        let opts = vec![iter("Sprint 23", true)];
        let mut p = IterationPicker::new(opts, Vec::new());
        assert_eq!(p.handle(key(KeyCode::Tab)), Some(true));
    }
}
