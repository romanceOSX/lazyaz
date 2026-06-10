//! Translates raw key events into [`Action`]s and feeds them to the [`App`].
//!
//! Modal states intercept input first (conflict prompt, help popup, wizard,
//! config insert). Multi-key `g` sequences (`gg`, `gt`, `gT`) use a pending flag.

use crate::app::{App, Mode};
use crate::keys::{Action, Context};
use crate::ui::help;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn handle_key(app: &mut App, key: KeyEvent) {
    // Ctrl-c always quits, from any state.
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.apply(Action::Quit);
        return;
    }
    // 1. Floating field editor is modal.
    if app.info_editor.is_some() {
        handle_info_editor(app, key);
        return;
    }
    // 2. Unresolved conflict is modal.
    if app.conflict.is_some() {
        handle_conflict(app, key);
        return;
    }
    // 3. Help popup.
    if app.show_help {
        handle_help(app, key);
        return;
    }
    // 4. First-run wizard.
    if app.wizard.is_some() {
        handle_wizard(app, key);
        return;
    }
    // 5. Config field editing (Insert mode).
    if app.mode == Mode::Insert {
        handle_insert(app, key);
        return;
    }
    handle_normal(app, key);
}

fn handle_info_editor(app: &mut App, key: KeyEvent) {
    // Inline-editing a single-line field?
    let editing = app
        .info_editor
        .as_ref()
        .is_some_and(|e| e.editing.is_some());
    if editing {
        match key.code {
            KeyCode::Esc => app.info_cancel_edit(),
            KeyCode::Enter => app.info_commit_edit(),
            _ => {
                if let Some(ed) = app.info_editor.as_mut()
                    && let Some(input) = ed.editing.as_mut() {
                        input.handle(key);
                    }
            }
        }
        return;
    }
    // Navigating the field list.
    match key.code {
        KeyCode::Esc => app.close_info_editor(),
        KeyCode::Char('j') | KeyCode::Down => app.info_nav(1),
        KeyCode::Char('k') | KeyCode::Up => app.info_nav(-1),
        KeyCode::Enter | KeyCode::Char('l') => app.info_activate(),
        _ => {}
    }
}

fn handle_conflict(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('m') => app.apply(Action::ResolveMerge),
        KeyCode::Char('f') => app.apply(Action::ResolveForce),
        KeyCode::Esc => {
            app.conflict = None;
            app.status = "conflict dismissed (no changes saved)".into();
        }
        _ => {}
    }
}

fn handle_help(app: &mut App, key: KeyEvent) {
    let matches = help::filtered(app);
    match key.code {
        KeyCode::Esc => app.apply(Action::ToggleHelp),
        KeyCode::Down => {
            app.help.selected = (app.help.selected + 1).min(matches.len().saturating_sub(1))
        }
        KeyCode::Up => app.help.selected = app.help.selected.saturating_sub(1),
        KeyCode::Enter => {
            if let Some(b) = matches.get(app.help.selected) {
                let action = b.action;
                app.apply(Action::ToggleHelp);
                app.apply(action);
            }
        }
        _ => {
            if app.help.input.handle(key) {
                app.help.selected = 0;
            }
        }
    }
}

fn handle_wizard(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.apply(Action::WizardPrev),
        KeyCode::Enter => app.apply(Action::WizardNext),
        _ => {
            if let Some(w) = app.wizard.as_mut() {
                let picker = if w.step == 0 { &mut w.org } else { &mut w.project };
                picker.handle(key);
            }
        }
    }
}

fn handle_insert(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.cancel_config_edit(),
        KeyCode::Enter => app.commit_config_edit(),
        _ => {
            app.config_edit.buffer.handle(key);
        }
    }
}

fn handle_normal(app: &mut App, key: KeyEvent) {
    // Multi-key `g` sequences.
    if app.pending_g {
        app.pending_g = false;
        match key.code {
            KeyCode::Char('g') => return app.apply(Action::Top),
            KeyCode::Char('t') => return app.apply(Action::NextTab),
            KeyCode::Char('T') => return app.apply(Action::PrevTab),
            _ => {}
        }
    }

    let ctx = app.context();
    match key.code {
        KeyCode::Char('q') => app.apply(Action::Quit),
        KeyCode::Char('?') => app.apply(Action::ToggleHelp),
        KeyCode::Char('g') => app.pending_g = true,
        KeyCode::Char('G') => app.apply(Action::Bottom),
        KeyCode::Tab => app.apply(Action::NextTab),
        KeyCode::BackTab => app.apply(Action::PrevTab),
        KeyCode::Char(c @ '1'..='4') => app.apply(Action::GotoTab(c as u8 - b'1')),

        KeyCode::Char('j') | KeyCode::Down => app.apply(Action::Down),
        KeyCode::Char('k') | KeyCode::Up => app.apply(Action::Up),

        // In Detail, l/h move between sub-windows (panes); elsewhere they
        // expand/collapse (tree) or open/edit.
        KeyCode::Char('l') | KeyCode::Right => match ctx {
            Context::Tree => app.apply(Action::TreeExpand),
            Context::Detail => app.apply(Action::FocusNext),
            Context::Config => app.apply(Action::EditField),
            _ => app.apply(Action::Open),
        },
        KeyCode::Char('h') | KeyCode::Left => match ctx {
            Context::Tree => app.apply(Action::TreeCollapse),
            Context::Detail => app.apply(Action::FocusPrev),
            _ => {}
        },
        KeyCode::Enter => match ctx {
            Context::Config => app.apply(Action::EditField),
            _ => app.apply(Action::Open),
        },
        // Esc steps back from Detail to the list.
        KeyCode::Esc if ctx == Context::Detail => app.apply(Action::Back),

        // Work-items context
        KeyCode::Char('f') if ctx == Context::WorkItems => app.apply(Action::NextFilter),
        KeyCode::Char('F') if ctx == Context::WorkItems => app.apply(Action::PrevFilter),
        KeyCode::Char('r') if ctx == Context::WorkItems => app.apply(Action::Reload),

        // Detail context
        KeyCode::Char('e') if ctx == Context::Detail => app.apply(Action::Edit),
        KeyCode::Char('n') if ctx == Context::Detail => app.apply(Action::EditNotes),
        KeyCode::Char('c') if ctx == Context::Detail => app.apply(Action::AddComment),
        KeyCode::Char('X') if ctx == Context::Detail => app.apply(Action::SimulateRemote),

        // Config context
        KeyCode::Char('i') if ctx == Context::Config => app.apply(Action::EditField),
        KeyCode::Char('s') if ctx == Context::Config => app.apply(Action::SaveConfig),
        KeyCode::Char('L') if ctx == Context::Config => app.apply(Action::Relogin),

        _ => {}
    }
}
