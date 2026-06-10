//! A small single-line text input with Emacs-style editing bindings, reused by
//! every in-app editable field (config edit, fuzzy help, wizard pickers).
//!
//! Bindings: C-a/C-e (home/end), C-f/C-b (char fwd/back), M-f/M-b (word),
//! C-d (delete fwd), Backspace/C-h (delete back), C-k (kill to end),
//! C-u (kill to start), C-w (delete word back).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

#[derive(Default, Clone)]
pub struct TextInput {
    chars: Vec<char>,
    cursor: usize,
}

impl TextInput {
    pub fn new(initial: &str) -> Self {
        let chars: Vec<char> = initial.chars().collect();
        let cursor = chars.len();
        Self { chars, cursor }
    }

    pub fn value(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn clear(&mut self) {
        self.chars.clear();
        self.cursor = 0;
    }

    fn is_word(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    fn prev_word(&self) -> usize {
        let mut i = self.cursor;
        while i > 0 && !Self::is_word(self.chars[i - 1]) {
            i -= 1;
        }
        while i > 0 && Self::is_word(self.chars[i - 1]) {
            i -= 1;
        }
        i
    }

    fn next_word(&self) -> usize {
        let n = self.chars.len();
        let mut i = self.cursor;
        while i < n && !Self::is_word(self.chars[i]) {
            i += 1;
        }
        while i < n && Self::is_word(self.chars[i]) {
            i += 1;
        }
        i
    }

    /// Handle a key; returns true if it was consumed as text editing.
    /// Callers handle navigation/submit keys (Enter, Esc, Tab) themselves.
    pub fn handle(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = self.chars.len(),
            KeyCode::Char('f') if ctrl => self.cursor = (self.cursor + 1).min(self.chars.len()),
            KeyCode::Char('b') if ctrl => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Char('f') if alt => self.cursor = self.next_word(),
            KeyCode::Char('b') if alt => self.cursor = self.prev_word(),
            KeyCode::Char('d') if ctrl => {
                if self.cursor < self.chars.len() {
                    self.chars.remove(self.cursor);
                }
            }
            KeyCode::Char('k') if ctrl => self.chars.truncate(self.cursor),
            KeyCode::Char('u') if ctrl => {
                self.chars.drain(0..self.cursor);
                self.cursor = 0;
            }
            KeyCode::Char('w') if ctrl => {
                let start = self.prev_word();
                self.chars.drain(start..self.cursor);
                self.cursor = start;
            }
            KeyCode::Char('h') if ctrl => self.backspace(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.chars.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.chars.len(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => {
                if self.cursor < self.chars.len() {
                    self.chars.remove(self.cursor);
                }
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                self.chars.insert(self.cursor, c);
                self.cursor += 1;
            }
            _ => return false,
        }
        true
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }

    /// Render the value with a block cursor as a sequence of spans.
    pub fn spans(&self, base: Style) -> Vec<Span<'static>> {
        let cursor_style = base.add_modifier(Modifier::REVERSED);
        let before: String = self.chars[..self.cursor].iter().collect();
        let at: String = if self.cursor < self.chars.len() {
            self.chars[self.cursor].to_string()
        } else {
            " ".to_string()
        };
        let after: String = if self.cursor < self.chars.len() {
            self.chars[self.cursor + 1..].iter().collect()
        } else {
            String::new()
        };
        vec![
            Span::styled(before, base),
            Span::styled(at, cursor_style),
            Span::styled(after, base),
        ]
    }
}
