//! Central keybinding registry. This is the single source of truth for the
//! context-aware fuzzy help popup (`?`). The actual key matching lives in
//! `event.rs`; keep the two in sync when adding bindings.

/// Which area a binding applies to. `Global` bindings are always shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    Global,
    Tree,
    WorkItems,
    Detail,
    Config,
    Wizard,
    Conflict,
    InfoEditor,
}

/// A semantic action produced by a keypress and consumed by `App::apply`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Quit,
    NextTab,
    PrevTab,
    GotoTab(u8),
    ToggleHelp,
    Down,
    Up,
    Top,
    Bottom,
    Open,
    Back,
    FocusNext,
    FocusPrev,
    TreeExpand,
    TreeCollapse,
    Edit,
    EditNotes,
    AddComment,
    SimulateRemote,
    ResolveMerge,
    ResolveForce,
    NextFilter,
    PrevFilter,
    Reload,
    EditField,
    SaveConfig,
    Relogin,
    WizardNext,
    WizardPrev,
}

pub struct Binding {
    pub keys: &'static str,
    pub desc: &'static str,
    pub ctx: Context,
    pub action: Action,
}

pub const BINDINGS: &[Binding] = &[
    // Global
    Binding { keys: "q",        desc: "Quit",                 ctx: Context::Global,    action: Action::Quit },
    Binding { keys: "?",        desc: "Toggle this help",     ctx: Context::Global,    action: Action::ToggleHelp },
    Binding { keys: "Tab / gt", desc: "Next tab",             ctx: Context::Global,    action: Action::NextTab },
    Binding { keys: "S-Tab/gT", desc: "Previous tab",         ctx: Context::Global,    action: Action::PrevTab },
    Binding { keys: "1-4",      desc: "Jump to tab",          ctx: Context::Global,    action: Action::GotoTab(0) },
    // Tree
    Binding { keys: "j / k",    desc: "Move down / up",       ctx: Context::Tree,      action: Action::Down },
    Binding { keys: "l",        desc: "Expand node",          ctx: Context::Tree,      action: Action::TreeExpand },
    Binding { keys: "h",        desc: "Collapse node",        ctx: Context::Tree,      action: Action::TreeCollapse },
    Binding { keys: "Enter",    desc: "Open in detail",       ctx: Context::Tree,      action: Action::Open },
    // Work items list
    Binding { keys: "j / k",    desc: "Move down / up",       ctx: Context::WorkItems, action: Action::Down },
    Binding { keys: "gg / G",   desc: "Top / bottom",         ctx: Context::WorkItems, action: Action::Top },
    Binding { keys: "l / Enter",desc: "Open detail",          ctx: Context::WorkItems, action: Action::Open },
    Binding { keys: "f / F",    desc: "Next / prev timeframe",ctx: Context::WorkItems, action: Action::NextFilter },
    Binding { keys: "r",        desc: "Reload list",          ctx: Context::WorkItems, action: Action::Reload },
    // Detail
    Binding { keys: "l / h",    desc: "Next/prev sub-window (info→desc→notes→comments→related)", ctx: Context::Detail, action: Action::FocusNext },
    Binding { keys: "j / k",    desc: "Move within list pane (comments/related)", ctx: Context::Detail,  action: Action::Down },
    Binding { keys: "Enter",    desc: "Act on pane: open field editor, edit desc/notes, edit comment, open related", ctx: Context::Detail, action: Action::Open },
    Binding { keys: "Esc",      desc: "Back to list",         ctx: Context::Detail,    action: Action::Back },
    // Floating field editor
    Binding { keys: "j / k",    desc: "Move between fields",  ctx: Context::InfoEditor, action: Action::Down },
    Binding { keys: "Enter",    desc: "Edit field (state cycles · long text → $EDITOR)", ctx: Context::InfoEditor, action: Action::Open },
    Binding { keys: "Esc",      desc: "Close editor",         ctx: Context::InfoEditor, action: Action::Back },
    Binding { keys: "e",        desc: "Edit description ($EDITOR)", ctx: Context::Detail, action: Action::Edit },
    Binding { keys: "n",        desc: "Edit notes ($EDITOR)", ctx: Context::Detail,    action: Action::EditNotes },
    Binding { keys: "c",        desc: "Add comment ($EDITOR)",ctx: Context::Detail,    action: Action::AddComment },
    Binding { keys: "X",        desc: "Simulate teammate edit (demo)", ctx: Context::Detail, action: Action::SimulateRemote },
    // Conflict prompt
    Binding { keys: "m",        desc: "Merge in $EDITOR",     ctx: Context::Conflict,  action: Action::ResolveMerge },
    Binding { keys: "f",        desc: "Force-push my changes",ctx: Context::Conflict,  action: Action::ResolveForce },
    // Config
    Binding { keys: "j / k",    desc: "Move among fields",    ctx: Context::Config,    action: Action::Down },
    Binding { keys: "i / Enter",desc: "Edit selected field",  ctx: Context::Config,    action: Action::EditField },
    Binding { keys: "s",        desc: "Save config to disk",  ctx: Context::Config,    action: Action::SaveConfig },
    Binding { keys: "L",        desc: "Re-login (credentials)",ctx: Context::Config,   action: Action::Relogin },
    // Wizard
    Binding { keys: "Enter",    desc: "Next step",            ctx: Context::Wizard,    action: Action::WizardNext },
    Binding { keys: "Esc",      desc: "Previous step",        ctx: Context::Wizard,    action: Action::WizardPrev },
];

/// Bindings shown in the help popup for `ctx` (always includes Global).
pub fn bindings_for(ctx: Context) -> Vec<&'static Binding> {
    BINDINGS
        .iter()
        .filter(|b| b.ctx == Context::Global || b.ctx == ctx)
        .collect()
}
