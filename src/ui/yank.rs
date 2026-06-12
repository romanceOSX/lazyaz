//! Floating "yank" menu for copying a work item's details to the system
//! clipboard. Opened with `y` while a work item is focused in the Work Items or
//! Tree window; the second key selects what to copy:
//!   yy → summary (title)        yl → hyperlink (markdown link)
//!   yn → number (#id)           yv → verbose (full details)
//! More entries can be added to [`YANK_ENTRIES`] over time. Esc cancels.

use crate::ui::theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// What a yank produces. The renderer/clipboard logic lives in `App::yank`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YankKind {
    /// The work item's title.
    Summary,
    /// A markdown hyperlink to the item in the ADO web UI.
    Hyperlink,
    /// The item's number (`#id`).
    Number,
    /// A multi-line dump of the item's key fields.
    Verbose,
}

/// A single selectable entry in the yank menu.
pub struct YankEntry {
    /// The key (the second `y…` keystroke) that triggers this entry.
    pub key: char,
    pub label: &'static str,
    pub kind: YankKind,
}

/// The yank menu's entries, in display order. Add more here as needed.
pub const YANK_ENTRIES: &[YankEntry] = &[
    YankEntry { key: 'y', label: "summary (title)", kind: YankKind::Summary },
    YankEntry { key: 'l', label: "hyperlink (markdown link)", kind: YankKind::Hyperlink },
    YankEntry { key: 'n', label: "number (#id)", kind: YankKind::Number },
    YankEntry { key: 'v', label: "verbose (full details)", kind: YankKind::Verbose },
];

/// Modal state for the yank menu. Holds the work item it was opened on so the
/// header can show it.
pub struct YankMenu {
    pub id: u32,
    pub title: String,
}

impl YankMenu {
    pub fn new(id: u32, title: String) -> Self {
        Self { id, title }
    }

    /// Map the second keystroke to the kind it selects, if any.
    pub fn kind_for(key: char) -> Option<YankKind> {
        YANK_ENTRIES
            .iter()
            .find(|e| e.key == key)
            .map(|e| e.kind)
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let h = (YANK_ENTRIES.len() as u16) + 4;
        let w = 52u16.min(area.width);
        let h = h.min(area.height);
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
            .title(" Yank (Esc cancel) ");
        let inner = block.inner(rect);
        f.render_widget(block, rect);

        let mut lines = Vec::with_capacity(YANK_ENTRIES.len() + 1);
        lines.push(Line::from(vec![
            Span::styled(format!("#{} ", self.id), Style::default().fg(theme::ACCENT)),
            Span::styled(self.title.clone(), Style::default().fg(theme::DIM)),
        ]));
        for e in YANK_ENTRIES {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {}  ", e.key),
                    Style::default()
                        .fg(theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(e.label),
            ]));
        }
        f.render_widget(Paragraph::new(lines), inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_for_maps_documented_keys() {
        assert_eq!(YankMenu::kind_for('y'), Some(YankKind::Summary));
        assert_eq!(YankMenu::kind_for('l'), Some(YankKind::Hyperlink));
        assert_eq!(YankMenu::kind_for('n'), Some(YankKind::Number));
        assert_eq!(YankMenu::kind_for('v'), Some(YankKind::Verbose));
    }

    #[test]
    fn kind_for_unknown_key_is_none() {
        assert_eq!(YankMenu::kind_for('x'), None);
        assert_eq!(YankMenu::kind_for('q'), None);
    }
}
