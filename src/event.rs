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
    // 1. Resolution-options menu is modal (can be triggered from the editor).
    if app.resolution.is_some() {
        handle_resolution(app, key);
        return;
    }
    // 2. Floating tags editor / state picker are modal (opened from the field
    //    editor, so they must intercept before it).
    if app.tags_editor.is_some() {
        handle_tags_editor(app, key);
        return;
    }
    if app.state_picker.is_some() {
        handle_state_picker(app, key);
        return;
    }
    if app.iteration_picker.is_some() {
        handle_iteration_picker(app, key);
        return;
    }
    if app.date_range.is_some() {
        handle_date_range(app, key);
        return;
    }
    if app.type_picker.is_some() {
        handle_type_picker(app, key);
        return;
    }
    if app.yank_menu.is_some() {
        handle_yank(app, key);
        return;
    }
    // 3. Floating field editor is modal.
    if app.info_editor.is_some() {
        handle_info_editor(app, key);
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

fn handle_state_picker(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.state_picker_cancel(),
        KeyCode::Enter => app.state_picker_select(),
        _ => {
            if let Some(p) = app.state_picker.as_mut() {
                p.handle(key);
            }
        }
    }
}

fn handle_tags_editor(app: &mut App, key: KeyEvent) {
    if let Some(t) = app.tags_editor.as_mut() {
        match t.handle(key) {
            Some(true) => app.tags_editor_commit(),
            Some(false) => app.tags_editor_cancel(),
            None => {}
        }
    }
}

fn handle_iteration_picker(app: &mut App, key: KeyEvent) {
    if let Some(p) = app.iteration_picker.as_mut() {
        match p.handle(key) {
            Some(true) => app.iteration_picker_commit(),
            Some(false) => app.iteration_picker_cancel(),
            None => {}
        }
    }
}

fn handle_type_picker(app: &mut App, key: KeyEvent) {
    if let Some(p) = app.type_picker.as_mut() {
        match p.handle(key) {
            Some(true) => app.type_picker_commit(),
            Some(false) => app.type_picker_cancel(),
            None => {}
        }
    }
}

/// Keys while the yank menu is open: the second `y…` keystroke selects what to
/// copy; Esc cancels; any other key closes the menu without copying.
fn handle_yank(app: &mut App, key: KeyEvent) {
    use crate::ui::yank::YankMenu;
    match key.code {
        KeyCode::Esc => app.yank_menu = None,
        KeyCode::Char(c) => {
            if let Some(kind) = YankMenu::kind_for(c) {
                app.yank(kind);
            }
            app.yank_menu = None;
        }
        _ => {}
    }
}

fn handle_date_range(app: &mut App, key: KeyEvent) {
    if let Some(d) = app.date_range.as_mut() {
        match d.handle(key) {
            Some(true) => app.date_range_commit(),
            Some(false) => app.date_range_cancel(),
            None => {}
        }
    }
}

fn handle_resolution(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.resolution_nav(1),
        KeyCode::Char('k') | KeyCode::Up => app.resolution_nav(-1),
        KeyCode::Char('m') => app.apply(Action::ResolveMerge),
        KeyCode::Char('f') => app.apply(Action::ResolveForce),
        KeyCode::Esc => app.resolution_cancel(),
        _ => {}
    }
}

fn handle_help(app: &mut App, key: KeyEvent) {
    let matches = help::filtered(app);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let searching = !app.help.input.value().trim().is_empty();
    match key.code {
        KeyCode::Esc => app.apply(Action::ToggleHelp),
        KeyCode::Down => {
            app.help.selected = (app.help.selected + 1).min(matches.len().saturating_sub(1))
        }
        KeyCode::Up => app.help.selected = app.help.selected.saturating_sub(1),
        KeyCode::Char('n') if ctrl => {
            app.help.selected = (app.help.selected + 1).min(matches.len().saturating_sub(1))
        }
        KeyCode::Char('p') if ctrl => app.help.selected = app.help.selected.saturating_sub(1),
        // Tab / Ctrl-y autocomplete the highlighted binding's description into
        // the query (only while searching).
        KeyCode::Tab | KeyCode::Char('y')
            if searching && (key.code != KeyCode::Char('y') || ctrl) =>
        {
            if let Some(b) = matches.get(app.help.selected) {
                app.help.input = crate::ui::input::TextInput::new(b.desc);
                app.help.selected = 0;
            }
        }
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

/// Keys while the in-pane fuzzy search bar is open: Enter commits the query as a
/// tag, Esc closes the bar, Backspace on an empty input deletes the last tag,
/// everything else edits the live query (live-filtering as you type).
fn handle_search(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => app.search_commit(),
        KeyCode::Esc => app.search_cancel(),
        KeyCode::Backspace if app.search_input_empty() => app.search_pop_tag(),
        _ => app.search_handle(key),
    }
}

fn handle_normal(app: &mut App, key: KeyEvent) {
    // The in-pane fuzzy search bar (`/`) captures all keys while open.
    if app.filter_searching() {
        handle_search(app, key);
        return;
    }

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
        // Open the fuzzy filter in the Work Items / Tree panes.
        KeyCode::Char('/') if matches!(ctx, Context::WorkItems | Context::Tree) => {
            app.open_search()
        }
        // Esc clears an active filter in those panes.
        KeyCode::Esc
            if matches!(ctx, Context::WorkItems | Context::Tree) && app.filter_active() =>
        {
            app.clear_filter()
        }
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
        KeyCode::Char('f') if ctx == Context::WorkItems => app.apply(Action::OpenTimeframeFilter),
        KeyCode::Char('F') if matches!(ctx, Context::WorkItems | Context::Tree) => {
            app.apply(Action::ClearTimeFilter)
        }
        KeyCode::Char('r') if ctx == Context::WorkItems => app.apply(Action::Reload),
        KeyCode::Char('r') if ctx == Context::Tree => app.apply(Action::RefreshTree),
        // Tree: J/K move between siblings, H/L move between levels (in/out).
        KeyCode::Char('J') if ctx == Context::Tree => app.apply(Action::TreeNextSibling),
        KeyCode::Char('K') if ctx == Context::Tree => app.apply(Action::TreePrevSibling),
        KeyCode::Char('c') if ctx == Context::Tree => app.apply(Action::TreeToggleRecursive),
        KeyCode::Char('R') if ctx == Context::Tree => app.apply(Action::ExportTreeReport),
        KeyCode::Char('L') if ctx == Context::Tree => app.apply(Action::TreeLevelIn),
        KeyCode::Char('H') if ctx == Context::Tree => app.apply(Action::TreeLevelOut),
        KeyCode::Char('v') if ctx == Context::WorkItems => app.apply(Action::ViewInTree),
        KeyCode::Char('i')
            if matches!(ctx, Context::WorkItems | Context::Tree) =>
        {
            app.apply(Action::OpenIterationFilter)
        }
        KeyCode::Char('t') if ctx == Context::WorkItems => {
            app.apply(Action::OpenTypeFilter)
        }
        // Yank menu: copy the focused work item's details to the clipboard.
        KeyCode::Char('y') if matches!(ctx, Context::WorkItems | Context::Tree) => {
            app.apply(Action::OpenYankMenu)
        }

        // Detail context
        KeyCode::Char('e') if ctx == Context::Detail => app.apply(Action::Edit),
        KeyCode::Char('n') if ctx == Context::Detail => app.apply(Action::EditNotes),
        KeyCode::Char('c') if ctx == Context::Detail => app.apply(Action::AddComment),
        KeyCode::Char('d') if ctx == Context::Detail => app.apply(Action::DeleteComment),
        KeyCode::Char('p') if ctx == Context::Detail => app.apply(Action::Push),
        KeyCode::Char('X') if ctx == Context::Detail => app.apply(Action::SimulateRemote),

        // Open the selected/current work item in the ADO web UI (any item view).
        KeyCode::Char('o')
            if matches!(ctx, Context::Tree | Context::WorkItems | Context::Detail) =>
        {
            app.apply(Action::OpenInBrowser)
        }

        // Config context
        KeyCode::Char('i') if ctx == Context::Config => app.apply(Action::EditField),
        KeyCode::Char('s') if ctx == Context::Config => app.apply(Action::SaveConfig),
        KeyCode::Char('L') if ctx == Context::Config => app.apply(Action::Relogin),

        _ => {}
    }
}
