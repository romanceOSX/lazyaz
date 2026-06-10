use crate::api::mock::MockClient;
use crate::api::models::{Timeframe, WorkItem};
use crate::api::WorkItemClient;
use crate::auth::mock::MockAuthenticator;
use crate::auth::Authenticator;
use crate::config::Config;
use crate::keys::{Action, Context};
use crate::ui::input::TextInput;
use crate::ui::picker::Picker;
use ratatui::widgets::ListState;
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// How often the app re-fetches server state to spot remote changes.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Tab {
    #[default]
    Tree,
    WorkItems,
    Detail,
    Config,
}

/// Which sub-window has focus inside the Detail view. `l`/`h` cycle through these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailFocus {
    /// Consolidated item-info pane; Enter opens the floating field editor.
    Info,
    Description,
    Notes,
    Comments,
    Relations,
}

impl DetailFocus {
    const ORDER: [DetailFocus; 5] = [
        DetailFocus::Info,
        DetailFocus::Description,
        DetailFocus::Notes,
        DetailFocus::Comments,
        DetailFocus::Relations,
    ];

    fn step(self, delta: isize) -> Self {
        let i = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0) as isize;
        let n = Self::ORDER.len() as isize;
        Self::ORDER[((i + delta).rem_euclid(n)) as usize]
    }
}

/// How a given editable field is edited inside the floating field editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// Single-line value, edited inline with the Emacs text input.
    Line,
    /// Long text, edited in `$EDITOR`.
    Multiline,
    /// Enum cycled in place (e.g. workflow state).
    State,
}

pub struct EditField {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
}

/// The set of editable fields, generic across any item type (story, epic,
/// feature, milestone, …). The backend ignores fields it doesn't support.
pub const EDITABLE_FIELDS: &[EditField] = &[
    EditField { key: "title",       label: "Title",       kind: FieldKind::Line },
    EditField { key: "state",       label: "State",       kind: FieldKind::State },
    EditField { key: "assignee",    label: "Assignee",    kind: FieldKind::Line },
    EditField { key: "iteration",   label: "Iteration",   kind: FieldKind::Line },
    EditField { key: "tags",        label: "Tags",        kind: FieldKind::Line },
    EditField { key: "description", label: "Description", kind: FieldKind::Multiline },
    EditField { key: "notes",       label: "Notes",       kind: FieldKind::Multiline },
];

/// Floating editor for an item's fields, opened from the Info pane.
pub struct InfoEditor {
    pub id: u32,
    pub selected: usize,
    /// `Some` while inline-editing the selected `Line` field.
    pub editing: Option<TextInput>,
}

impl Tab {
    pub const ORDER: [Tab; 4] = [Tab::Tree, Tab::WorkItems, Tab::Detail, Tab::Config];
    pub fn title(&self) -> &'static str {
        match self {
            Tab::Tree => "Tree",
            Tab::WorkItems => "Work Items",
            Tab::Detail => "Detail",
            Tab::Config => "Config",
        }
    }
    pub fn index(&self) -> usize {
        Self::ORDER.iter().position(|t| t == self).unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
}

/// First-run setup: pick an org, then a project, both via fzf-style pickers.
pub struct WizardState {
    pub step: usize,
    pub org: Picker,
    pub project: Picker,
}

/// In-progress edit of a Config field (Emacs-bound text input).
pub struct ConfigEdit {
    pub selected: usize,
    pub buffer: TextInput,
}

pub const CONFIG_FIELDS: [&str; 4] = ["org_url", "project", "team", "account"];

/// A request for the main loop to suspend the TUI and open `$EDITOR`.
pub enum EditorRequest {
    Field {
        id: u32,
        field: &'static str,
        initial: String,
        base_rev: u32,
    },
    /// Resolving a conflict by hand-merging in the editor.
    Merge {
        id: u32,
        field: &'static str,
    },
    Comment {
        id: u32,
    },
    /// Editing an existing comment (last-write-wins, no conflict detection).
    EditComment {
        id: u32,
        comment_id: u32,
        initial: String,
    },
}

/// A detected edit conflict: the user's local edit vs a newer server version.
pub struct Conflict {
    pub id: u32,
    pub field: &'static str,
    pub base: String,
    pub local: String,
    pub remote: String,
}

/// Fuzzy help popup state.
#[derive(Default)]
pub struct HelpState {
    pub input: TextInput,
    pub selected: usize,
}

/// Collapsible relationship tree.
#[derive(Default)]
pub struct TreeState {
    pub expanded: HashSet<u32>,
    pub selected: usize,
    /// Visible rows as (id, depth), rebuilt from items + expanded.
    pub flat: Vec<(u32, usize)>,
}

pub struct App {
    pub config: Config,
    pub tab: Tab,
    pub mode: Mode,
    pub should_quit: bool,
    pub status: String,

    pub client: Box<dyn WorkItemClient>,
    pub auth: Box<dyn Authenticator>,

    pub items: Vec<WorkItem>,
    pub list_state: ListState,
    pub timeframe: Timeframe,

    pub tree: TreeState,

    pub current: Option<WorkItem>,
    pub detail_focus: DetailFocus,
    pub detail_selected: usize,
    pub comment_selected: usize,
    /// Floating field editor (modal), opened from the Info pane.
    pub info_editor: Option<InfoEditor>,

    pub wizard: Option<WizardState>,
    pub config_edit: ConfigEdit,

    pub conflict: Option<Conflict>,

    pub show_help: bool,
    pub help: HelpState,

    pub pending_g: bool,
    pub pending_editor: Option<EditorRequest>,
    /// Seed text for a pending Merge editor request (conflict markers).
    pub merge_seed: Option<String>,

    started: Instant,
    last_poll: Instant,
}

impl App {
    /// Build with the offline mock backend (used by tests). The binary goes
    /// through [`App::with_backends`] so it can pick a real backend.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(config: Config) -> Self {
        Self::with_backends(config, Box::new(MockClient::new()), Box::new(MockAuthenticator::default()))
    }

    /// Build with explicit backends (real Azure DevOps client/authenticator).
    pub fn with_backends(
        config: Config,
        client: Box<dyn WorkItemClient>,
        auth: Box<dyn Authenticator>,
    ) -> Self {
        let needs_wizard = !config.is_complete();
        let wizard = needs_wizard.then(|| WizardState {
            step: 0,
            org: Picker::new(auth.list_orgs()),
            project: Picker::new(Vec::new()),
        });
        let now = Instant::now();
        let mut app = Self {
            wizard,
            tab: Tab::Tree,
            mode: Mode::Normal,
            should_quit: false,
            status: "Welcome to lazyaz — press ? for help".into(),
            client,
            auth,
            items: Vec::new(),
            list_state: ListState::default(),
            timeframe: Timeframe::All,
            tree: TreeState::default(),
            current: None,
            detail_focus: DetailFocus::Info,
            detail_selected: 0,
            comment_selected: 0,
            info_editor: None,
            config_edit: ConfigEdit {
                selected: 0,
                buffer: TextInput::default(),
            },
            conflict: None,
            show_help: false,
            help: HelpState::default(),
            pending_g: false,
            pending_editor: None,
            merge_seed: None,
            started: now,
            last_poll: now,
            config,
        };
        if app.wizard.is_none() {
            app.reload_items();
        }
        app
    }

    /// Force the first-run setup wizard (used by `--setup` to reconfigure).
    pub fn start_wizard(&mut self) {
        self.wizard = Some(WizardState {
            step: 0,
            org: Picker::new(self.auth.list_orgs()),
            project: Picker::new(Vec::new()),
        });
        self.status = "reconfiguring — pick org and project".into();
    }

    /// Restore the last saved session over the current (already-loaded) state.
    /// Called from `main` on a normal launch; skipped in tests and during setup.
    pub fn restore_session(&mut self) {
        if self.wizard.is_some() {
            return;
        }
        let Some(s) = crate::session::SessionState::load() else {
            return;
        };
        self.timeframe = s.timeframe;
        if !s.tree_expanded.is_empty() {
            self.tree.expanded = s.tree_expanded.into_iter().collect();
        }
        self.reload_items(); // clamps selections against current data
        self.tree.selected = s.tree_selected.min(self.tree.flat.len().saturating_sub(1));
        if !self.items.is_empty() {
            self.list_state
                .select(Some(s.list_selected.min(self.items.len() - 1)));
        }
        // Reopen the previously focused item if it still exists.
        if let Some(id) = s.current_id
            && let Ok(item) = self.client.get(id) {
                self.current = Some(item);
                self.detail_selected = s.detail_selected;
            }
        self.tab = s.tab;
        self.status = "restored last session".into();
    }

    /// Persist the current view so the next launch can resume it.
    pub fn save_session(&self) {
        // Nothing meaningful to save mid-wizard.
        if self.wizard.is_some() {
            return;
        }
        let state = crate::session::SessionState {
            tab: self.tab,
            timeframe: self.timeframe,
            current_id: self.current.as_ref().map(|w| w.id),
            tree_expanded: self.tree.expanded.iter().copied().collect(),
            tree_selected: self.tree.selected,
            list_selected: self.list_state.selected().unwrap_or(0),
            detail_selected: self.detail_selected,
        };
        let _ = state.save();
    }

    pub fn context(&self) -> Context {
        if self.info_editor.is_some() {
            return Context::InfoEditor;
        }
        if self.conflict.is_some() {
            return Context::Conflict;
        }
        if self.wizard.is_some() {
            return Context::Wizard;
        }
        match self.tab {
            Tab::Tree => Context::Tree,
            Tab::WorkItems => Context::WorkItems,
            Tab::Detail => Context::Detail,
            Tab::Config => Context::Config,
        }
    }

    /// Slow blink used to make unresolved conflict areas pulsate.
    pub fn pulse(&self) -> bool {
        (self.started.elapsed().as_millis() / 500).is_multiple_of(2)
    }

    pub fn reload_items(&mut self) {
        match self.client.list_assigned(self.timeframe) {
            Ok(items) => {
                self.items = items;
                if self.items.is_empty() {
                    self.list_state.select(None);
                } else {
                    let sel = self
                        .list_state
                        .selected()
                        .unwrap_or(0)
                        .min(self.items.len() - 1);
                    self.list_state.select(Some(sel));
                }
                self.rebuild_tree();
                self.status = format!(
                    "{} item(s) · timeframe: {}",
                    self.items.len(),
                    self.timeframe.label()
                );
            }
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    /// Periodic refresh from the server (called by the main loop).
    pub fn poll(&mut self) {
        if self.last_poll.elapsed() < POLL_INTERVAL {
            return;
        }
        self.last_poll = Instant::now();
        self.reload_items();
        if let Some(cur) = &self.current {
            let id = cur.id;
            if let Ok(item) = self.client.get(id) {
                self.current = Some(item);
            }
        }
    }

    pub fn client_title(&self, id: u32) -> Option<String> {
        self.client.get(id).ok().map(|w| w.title)
    }

    pub fn related_ids(&self) -> Vec<u32> {
        let mut ids = Vec::new();
        if let Some(item) = &self.current {
            if let Some(p) = item.parent {
                ids.push(p);
            }
            ids.extend(item.children.iter().copied());
        }
        ids
    }

    // --- tree ---
    fn rebuild_tree(&mut self) {
        let ids: HashSet<u32> = self.items.iter().map(|w| w.id).collect();
        // First build: expand every node that has children.
        if self.tree.expanded.is_empty() {
            for w in &self.items {
                if !w.children.is_empty() {
                    self.tree.expanded.insert(w.id);
                }
            }
        }
        let mut roots: Vec<u32> = self
            .items
            .iter()
            .filter(|w| w.parent.is_none_or(|p| !ids.contains(&p)))
            .map(|w| w.id)
            .collect();
        roots.sort_unstable();

        let mut flat = Vec::new();
        for r in roots {
            flatten(&self.items, &self.tree.expanded, r, 0, &mut flat);
        }
        self.tree.flat = flat;
        if self.tree.selected >= self.tree.flat.len() {
            self.tree.selected = self.tree.flat.len().saturating_sub(1);
        }
    }

    pub fn tree_has_children(&self, id: u32) -> bool {
        self.items
            .iter()
            .any(|w| w.id == id && !w.children.is_empty())
    }

    fn tree_selected_id(&self) -> Option<u32> {
        self.tree.flat.get(self.tree.selected).map(|(id, _)| *id)
    }

    fn open_id(&mut self, id: u32) {
        match self.client.get(id) {
            Ok(item) => {
                self.current = Some(item);
                self.detail_focus = DetailFocus::Info;
                self.detail_selected = 0;
                self.comment_selected = 0;
                self.tab = Tab::Detail;
            }
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    pub fn current_comment_count(&self) -> usize {
        self.current.as_ref().map_or(0, |w| w.comments.len())
    }


    pub fn apply(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
                self.help.input.clear();
                self.help.selected = 0;
            }
            Action::NextTab => self.tab = Tab::ORDER[(self.tab.index() + 1) % Tab::ORDER.len()],
            Action::PrevTab => {
                self.tab = Tab::ORDER[(self.tab.index() + Tab::ORDER.len() - 1) % Tab::ORDER.len()]
            }
            Action::GotoTab(n) => {
                if let Some(t) = Tab::ORDER.get(n as usize) {
                    self.tab = *t;
                }
            }
            Action::Down => self.move_selection(1),
            Action::Up => self.move_selection(-1),
            Action::Top => self.set_selection(0),
            Action::Bottom => self.set_selection(isize::MAX),
            Action::NextFilter => {
                self.timeframe = self.timeframe.next();
                self.reload_items();
            }
            Action::PrevFilter => {
                self.timeframe = self.timeframe.prev();
                self.reload_items();
            }
            Action::Reload => self.reload_items(),
            Action::Open => self.open(),
            Action::Back => self.tab = Tab::WorkItems,
            Action::FocusNext => self.detail_focus = self.detail_focus.step(1),
            Action::FocusPrev => self.detail_focus = self.detail_focus.step(-1),
            Action::TreeExpand => self.tree_set_expanded(true),
            Action::TreeCollapse => self.tree_set_expanded(false),
            Action::Edit => self.request_field_edit("description"),
            Action::EditNotes => self.request_field_edit("notes"),
            Action::AddComment => {
                if let Some(item) = &self.current {
                    self.pending_editor = Some(EditorRequest::Comment { id: item.id });
                }
            }
            Action::SimulateRemote => self.simulate_remote(),
            Action::ResolveMerge => self.resolve_merge(),
            Action::ResolveForce => self.resolve_force(),
            Action::EditField => self.begin_config_edit(),
            Action::SaveConfig => match self.config.save() {
                Ok(()) => self.status = "config saved".into(),
                Err(e) => self.status = format!("save failed: {e}"),
            },
            Action::Relogin => self.do_login(),
            Action::WizardNext => self.wizard_next(),
            Action::WizardPrev => self.wizard_prev(),
        }
    }

    fn tree_set_expanded(&mut self, expand: bool) {
        if let Some(id) = self.tree_selected_id() {
            if expand {
                if self.tree_has_children(id) {
                    self.tree.expanded.insert(id);
                }
            } else {
                self.tree.expanded.remove(&id);
            }
            self.rebuild_tree();
        }
    }

    fn move_selection(&mut self, delta: isize) {
        match self.context() {
            Context::Tree => {
                let len = self.tree.flat.len();
                if len > 0 {
                    let next = (self.tree.selected as isize + delta).clamp(0, len as isize - 1);
                    self.tree.selected = next as usize;
                }
            }
            Context::WorkItems => {
                if self.items.is_empty() {
                    return;
                }
                let cur = self.list_state.selected().unwrap_or(0) as isize;
                let next = (cur + delta).clamp(0, self.items.len() as isize - 1);
                self.list_state.select(Some(next as usize));
            }
            Context::Detail => match self.detail_focus {
                DetailFocus::Relations => {
                    let len = self.related_ids().len();
                    if len == 0 {
                        return;
                    }
                    let next =
                        (self.detail_selected as isize + delta).clamp(0, len as isize - 1);
                    self.detail_selected = next as usize;
                }
                DetailFocus::Comments => {
                    let len = self.current_comment_count();
                    if len == 0 {
                        return;
                    }
                    let next =
                        (self.comment_selected as isize + delta).clamp(0, len as isize - 1);
                    self.comment_selected = next as usize;
                }
                _ => {} // non-list panes don't have a moving selection
            },
            Context::Config => {
                let len = CONFIG_FIELDS.len() as isize;
                let next = (self.config_edit.selected as isize + delta).clamp(0, len - 1);
                self.config_edit.selected = next as usize;
            }
            _ => {}
        }
    }

    fn set_selection(&mut self, target: isize) {
        match self.context() {
            Context::Tree => {
                let len = self.tree.flat.len();
                if len > 0 {
                    self.tree.selected = target.clamp(0, len as isize - 1) as usize;
                }
            }
            Context::WorkItems if !self.items.is_empty() => {
                let idx = target.clamp(0, self.items.len() as isize - 1) as usize;
                self.list_state.select(Some(idx));
            }
            Context::Detail => match self.detail_focus {
                DetailFocus::Relations => {
                    let len = self.related_ids().len();
                    if len > 0 {
                        self.detail_selected = target.clamp(0, len as isize - 1) as usize;
                    }
                }
                DetailFocus::Comments => {
                    let len = self.current_comment_count();
                    if len > 0 {
                        self.comment_selected = target.clamp(0, len as isize - 1) as usize;
                    }
                }
                _ => {}
            },
            Context::Config => {
                self.config_edit.selected =
                    target.clamp(0, CONFIG_FIELDS.len() as isize - 1) as usize;
            }
            _ => {}
        }
    }

    fn open(&mut self) {
        match self.context() {
            Context::Tree => {
                if let Some(id) = self.tree_selected_id() {
                    self.open_id(id);
                }
            }
            Context::WorkItems => {
                if let Some(idx) = self.list_state.selected()
                    && let Some(item) = self.items.get(idx) {
                        let id = item.id;
                        self.open_id(id);
                    }
            }
            Context::Detail => match self.detail_focus {
                DetailFocus::Info => self.open_info_editor(),
                DetailFocus::Description => self.request_field_edit("description"),
                DetailFocus::Notes => self.request_field_edit("notes"),
                DetailFocus::Comments => self.request_comment_edit(),
                DetailFocus::Relations => {
                    let ids = self.related_ids();
                    if let Some(&id) = ids.get(self.detail_selected) {
                        self.open_id(id);
                    }
                }
            },
            _ => {}
        }
    }

    // --- floating field editor (Info pane) ---
    fn open_info_editor(&mut self) {
        if let Some(item) = &self.current {
            self.info_editor = Some(InfoEditor {
                id: item.id,
                selected: 0,
                editing: None,
            });
        }
    }

    pub fn close_info_editor(&mut self) {
        self.info_editor = None;
    }

    /// Current value of an editable field on the open item, for display.
    pub fn info_field_value(&self, key: &str) -> String {
        let Some(item) = &self.current else {
            return String::new();
        };
        match key {
            "title" => item.title.clone(),
            "state" => item.state.to_string(),
            "assignee" => item.assigned_to.clone(),
            "iteration" => item.iteration.clone(),
            "tags" => item.tags.join(", "),
            "description" => item.description.clone(),
            "notes" => item.notes.clone(),
            _ => String::new(),
        }
    }

    pub fn info_nav(&mut self, delta: isize) {
        if let Some(ed) = &mut self.info_editor {
            let n = EDITABLE_FIELDS.len() as isize;
            ed.selected = (ed.selected as isize + delta).rem_euclid(n) as usize;
        }
    }

    /// Enter on the selected field: inline-edit, cycle, or hand off to $EDITOR.
    pub fn info_activate(&mut self) {
        let Some(ed) = &self.info_editor else { return };
        let field = &EDITABLE_FIELDS[ed.selected];
        match field.kind {
            FieldKind::Line => {
                let value = self.info_field_value(field.key);
                if let Some(ed) = &mut self.info_editor {
                    ed.editing = Some(TextInput::new(&value));
                }
            }
            FieldKind::State => {
                if let Some(item) = &self.current {
                    let id = item.id;
                    let next = item.state.next();
                    let _ = self.client.update_field(id, "state", next.label());
                    self.refresh_current();
                    self.reload_items();
                    self.status = format!("#{id} state → {}", next.label());
                }
            }
            FieldKind::Multiline => {
                // Editor handoff closes the modal; conflict-aware via base_rev.
                let key = field.key;
                self.close_info_editor();
                self.request_field_edit(key);
            }
        }
    }

    pub fn info_commit_edit(&mut self) {
        let Some(ed) = &self.info_editor else { return };
        let key = EDITABLE_FIELDS[ed.selected].key;
        let Some(input) = &ed.editing else { return };
        let value = input.value();
        let id = ed.id;
        match self.client.update_field(id, key, &value) {
            Ok(()) => self.status = format!("{key} updated on #{id}"),
            Err(e) => self.status = format!("update failed: {e}"),
        }
        if let Some(ed) = &mut self.info_editor {
            ed.editing = None;
        }
        self.refresh_current();
        self.reload_items();
    }

    pub fn info_cancel_edit(&mut self) {
        if let Some(ed) = &mut self.info_editor {
            ed.editing = None;
        }
    }

    fn request_comment_edit(&mut self) {
        if let Some(item) = &self.current
            && let Some(c) = item.comments.get(self.comment_selected) {
                self.pending_editor = Some(EditorRequest::EditComment {
                    id: item.id,
                    comment_id: c.id,
                    initial: c.text.clone(),
                });
            }
    }

    fn request_field_edit(&mut self, field: &'static str) {
        if let Some(item) = &self.current {
            let initial = match field {
                "title" => item.title.clone(),
                "notes" => item.notes.clone(),
                _ => item.description.clone(),
            };
            self.pending_editor = Some(EditorRequest::Field {
                id: item.id,
                field,
                initial,
                base_rev: item.rev,
            });
        }
    }

    /// Called by the main loop after `$EDITOR` returns.
    pub fn apply_editor_result(&mut self, req: EditorRequest, text: String) {
        let text = text.trim_end().to_string();
        match req {
            EditorRequest::Field {
                id,
                field,
                initial,
                base_rev,
            } => {
                // Conflict if the server moved on AND the remote value differs.
                let remote_item = self.client.get(id).ok();
                let server_rev = remote_item.as_ref().map(|w| w.rev).unwrap_or(base_rev);
                let remote = remote_item
                    .map(|w| field_value(&w, field))
                    .unwrap_or_default();
                if server_rev != base_rev && remote != initial && remote != text {
                    self.conflict = Some(Conflict {
                        id,
                        field,
                        base: initial,
                        local: text,
                        remote,
                    });
                    self.status =
                        format!("conflict on {field} of #{id}: m to merge, f to force-push");
                } else {
                    match self.client.update_field(id, field, &text) {
                        Ok(()) => self.status = format!("updated {field} on #{id} (local mock)"),
                        Err(e) => self.status = format!("update failed: {e}"),
                    }
                }
            }
            EditorRequest::Merge { id, field } => {
                match self.client.update_field(id, field, &text) {
                    Ok(()) => self.status = format!("merged {field} on #{id}"),
                    Err(e) => self.status = format!("merge failed: {e}"),
                }
                self.conflict = None;
            }
            EditorRequest::Comment { id } => {
                if text.is_empty() {
                    self.status = "empty comment discarded".into();
                } else {
                    let author = self.auth.account().unwrap_or("you@example.com").to_string();
                    match self.client.add_comment(id, &author, &text) {
                        Ok(()) => self.status = format!("comment added to #{id} (local mock)"),
                        Err(e) => self.status = format!("comment failed: {e}"),
                    }
                }
            }
            // Last-write-wins: comment edits skip conflict detection by design.
            EditorRequest::EditComment { id, comment_id, .. } => {
                match self.client.update_comment(id, comment_id, &text) {
                    Ok(()) => self.status = format!("comment {comment_id} edited on #{id}"),
                    Err(e) => self.status = format!("comment edit failed: {e}"),
                }
            }
        }
        self.refresh_current();
        self.reload_items();
    }

    fn refresh_current(&mut self) {
        if let Some(cur) = &self.current {
            let id = cur.id;
            if let Ok(item) = self.client.get(id) {
                self.current = Some(item);
            }
        }
    }

    // --- conflict handling ---
    fn simulate_remote(&mut self) {
        if let Some(cur) = &self.current {
            let id = cur.id;
            match self.client.simulate_remote_edit(id) {
                Ok(()) => {
                    self.status = format!("simulated a teammate editing #{id} — now try editing it");
                    self.refresh_current();
                    self.reload_items();
                }
                Err(e) => self.status = format!("{e}"),
            }
        }
    }

    fn resolve_merge(&mut self) {
        if let Some(c) = &self.conflict {
            let body = format!(
                "<<<<<<< local (your changes)\n{}\n||||||| base (common ancestor)\n{}\n=======\n{}\n>>>>>>> remote (current server)\n",
                c.local, c.base, c.remote
            );
            self.pending_editor = Some(EditorRequest::Merge {
                id: c.id,
                field: c.field,
            });
            // Stash the marker text to seed the editor via the Merge initial.
            self.merge_seed = Some(body);
        }
    }

    fn resolve_force(&mut self) {
        if let Some(c) = self.conflict.take() {
            match self.client.update_field(c.id, c.field, &c.local) {
                Ok(()) => self.status = format!("force-pushed {} on #{}", c.field, c.id),
                Err(e) => self.status = format!("force-push failed: {e}"),
            }
            self.refresh_current();
            self.reload_items();
        }
    }

    fn begin_config_edit(&mut self) {
        let idx = self.config_edit.selected;
        self.config_edit.buffer = TextInput::new(&self.config_field_value(idx));
        self.mode = Mode::Insert;
    }

    pub fn commit_config_edit(&mut self) {
        let idx = self.config_edit.selected;
        let val = self.config_edit.buffer.value();
        match CONFIG_FIELDS[idx] {
            "org_url" => self.config.org_url = val,
            "project" => self.config.project = val,
            "team" => self.config.team = val,
            "account" => self.config.account = val,
            _ => {}
        }
        self.mode = Mode::Normal;
        self.status = format!("{} set (press s to save)", CONFIG_FIELDS[idx]);
    }

    pub fn cancel_config_edit(&mut self) {
        self.mode = Mode::Normal;
    }

    pub fn config_field_value(&self, idx: usize) -> String {
        match CONFIG_FIELDS[idx] {
            "org_url" => self.config.org_url.clone(),
            "project" => self.config.project.clone(),
            "team" => self.config.team.clone(),
            "account" => self.config.account.clone(),
            _ => String::new(),
        }
    }

    fn do_login(&mut self) {
        match self.auth.login() {
            Ok(state) => {
                self.config.account = state.account.clone();
                self.status = format!("signed in as {} (mock)", state.account);
            }
            Err(e) => self.status = format!("login failed: {e}"),
        }
    }

    // --- wizard ---
    fn wizard_next(&mut self) {
        let Some(w) = self.wizard.as_mut() else { return };
        match w.step {
            0 => {
                if let Some(org) = w.org.current() {
                    let projects = self.client.list_projects(&org);
                    if let Some(w) = self.wizard.as_mut() {
                        w.project.set_options(projects);
                        w.step = 1;
                    }
                }
            }
            1 => {
                if w.project.current().is_some() {
                    w.step = 2;
                }
            }
            _ => {
                let org = w.org.current().unwrap_or_default();
                let project = w.project.current().unwrap_or_default();
                self.config.org_url = format!("https://dev.azure.com/{org}");
                self.config.project = project;
                self.do_login();
                let _ = self.config.save();
                self.wizard = None;
                self.tab = Tab::Tree;
                self.reload_items();
                self.status = "setup complete".into();
            }
        }
    }

    fn wizard_prev(&mut self) {
        if let Some(w) = self.wizard.as_mut()
            && w.step > 0 {
                w.step -= 1;
            }
    }
}

fn field_value(w: &WorkItem, field: &str) -> String {
    match field {
        "notes" => w.notes.clone(),
        "title" => w.title.clone(),
        _ => w.description.clone(),
    }
}

fn flatten(
    items: &[WorkItem],
    expanded: &HashSet<u32>,
    id: u32,
    depth: usize,
    out: &mut Vec<(u32, usize)>,
) {
    out.push((id, depth));
    if !expanded.contains(&id) {
        return;
    }
    if let Some(item) = items.iter().find(|w| w.id == id) {
        for c in &item.children {
            if items.iter().any(|w| w.id == *c) {
                flatten(items, expanded, *c, depth + 1, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_app() -> App {
        let cfg = Config {
            org_url: "https://dev.azure.com/acme".into(),
            project: "Widgets".into(),
            ..Default::default()
        };
        let mut app = App::new(cfg);
        app.timeframe = Timeframe::All;
        app.reload_items();
        app
    }

    #[test]
    fn empty_config_starts_fzf_wizard() {
        let app = App::new(Config::default());
        let w = app.wizard.as_ref().expect("wizard");
        assert_eq!(w.step, 0);
        assert!(!w.org.options.is_empty()); // orgs pre-populated for fuzzy pick
        assert_eq!(app.context(), Context::Wizard);
    }

    #[test]
    fn tree_flattens_and_collapses() {
        let mut app = ready_app();
        // Epic 1001 expanded shows its two story children.
        assert!(app.tree.flat.iter().any(|(id, _)| *id == 1002));
        // Select the epic and collapse it; children disappear.
        let epic_row = app.tree.flat.iter().position(|(id, _)| *id == 1001).unwrap();
        app.tree.selected = epic_row;
        app.apply(Action::TreeCollapse);
        assert!(!app.tree.flat.iter().any(|(id, _)| *id == 1002));
        app.apply(Action::TreeExpand);
        assert!(app.tree.flat.iter().any(|(id, _)| *id == 1002));
    }

    #[test]
    fn tree_open_navigates_to_detail() {
        let mut app = ready_app();
        app.tree.selected = app.tree.flat.iter().position(|(id, _)| *id == 1002).unwrap();
        app.apply(Action::Open);
        assert_eq!(app.tab, Tab::Detail);
        assert_eq!(app.current.as_ref().unwrap().id, 1002);
    }

    #[test]
    fn notes_edit_applies() {
        let mut app = ready_app();
        app.open_id(1001);
        let base_rev = app.current.as_ref().unwrap().rev;
        app.apply_editor_result(
            EditorRequest::Field { id: 1001, field: "notes", initial: "x".into(), base_rev },
            "new notes".into(),
        );
        assert_eq!(app.current.as_ref().unwrap().notes, "new notes");
    }

    #[test]
    fn remote_change_triggers_conflict_then_force_resolves() {
        let mut app = ready_app();
        app.open_id(1002);
        let base_rev = app.current.as_ref().unwrap().rev;
        // Teammate edits the same item after we started editing.
        app.client.simulate_remote_edit(1002).unwrap();
        app.apply_editor_result(
            EditorRequest::Field {
                id: 1002,
                field: "description",
                initial: "my original".into(),
                base_rev,
            },
            "my edit".into(),
        );
        assert!(app.conflict.is_some(), "expected a conflict");
        // Force-push keeps our local value.
        app.apply(Action::ResolveForce);
        assert!(app.conflict.is_none());
        assert_eq!(app.current.as_ref().unwrap().description, "my edit");
    }

    #[test]
    fn comment_edit_applies_to_focused_comment() {
        let mut app = ready_app();
        app.open_id(1001); // epic has one seeded comment
        app.detail_focus = DetailFocus::Comments;
        app.comment_selected = 0;
        let comment_id = app.current.as_ref().unwrap().comments[0].id;
        app.apply_editor_result(
            EditorRequest::EditComment {
                id: 1001,
                comment_id,
                initial: "old".into(),
            },
            "edited text".into(),
        );
        assert_eq!(app.current.as_ref().unwrap().comments[0].text, "edited text");
    }

    #[test]
    fn l_h_cycle_all_detail_panes() {
        let mut app = ready_app();
        app.open_id(1001);
        let start = app.detail_focus;
        // Five panes: stepping forward five times returns to start.
        for _ in 0..5 {
            app.apply(Action::FocusNext);
        }
        assert_eq!(app.detail_focus, start);
        // FocusPrev is the inverse of FocusNext.
        app.apply(Action::FocusNext);
        app.apply(Action::FocusPrev);
        assert_eq!(app.detail_focus, start);
    }

    fn field_index(key: &str) -> usize {
        EDITABLE_FIELDS.iter().position(|f| f.key == key).unwrap()
    }

    #[test]
    fn info_editor_edits_title_inline() {
        let mut app = ready_app();
        app.open_id(1002);
        app.detail_focus = DetailFocus::Info;
        app.apply(Action::Open); // open the floating editor
        assert!(app.info_editor.is_some());
        app.info_editor.as_mut().unwrap().selected = field_index("title");
        app.info_activate(); // begin inline edit
        app.info_editor.as_mut().unwrap().editing = Some(TextInput::new("Renamed item"));
        app.info_commit_edit();
        assert_eq!(app.current.as_ref().unwrap().title, "Renamed item");
    }

    #[test]
    fn info_editor_cycles_state() {
        let mut app = ready_app();
        app.open_id(1002);
        let before = app.current.as_ref().unwrap().state;
        app.open_info_editor();
        app.info_editor.as_mut().unwrap().selected = field_index("state");
        app.info_activate();
        assert_eq!(app.current.as_ref().unwrap().state, before.next());
    }

    #[test]
    fn info_editor_multiline_hands_off_to_editor() {
        let mut app = ready_app();
        app.open_id(1002);
        app.open_info_editor();
        app.info_editor.as_mut().unwrap().selected = field_index("description");
        app.info_activate();
        // Modal closes and an editor request is queued.
        assert!(app.info_editor.is_none());
        assert!(app.pending_editor.is_some());
    }

    #[test]
    fn session_round_trips_view_state() {
        use crate::session::SessionState;
        let mut app = ready_app();
        app.open_id(1002);
        app.tab = Tab::Detail;
        let saved = SessionState {
            tab: app.tab,
            timeframe: app.timeframe,
            current_id: app.current.as_ref().map(|w| w.id),
            tree_expanded: app.tree.expanded.iter().copied().collect(),
            tree_selected: app.tree.selected,
            list_selected: app.list_state.selected().unwrap_or(0),
            detail_selected: app.detail_selected,
        };
        // Serialize → deserialize must preserve the key fields.
        let text = toml::to_string_pretty(&saved).unwrap();
        let back: SessionState = toml::from_str(&text).unwrap();
        assert_eq!(back.tab, Tab::Detail);
        assert_eq!(back.current_id, Some(1002));
        assert_eq!(back.timeframe, Timeframe::All);
    }

    #[test]
    fn config_edit_updates_in_memory() {
        let mut app = ready_app();
        app.apply(Action::GotoTab(3)); // Config tab
        assert_eq!(app.context(), Context::Config);
        app.config_edit.selected = 1; // project
        app.apply(Action::EditField);
        app.config_edit.buffer = TextInput::new("NewProject");
        app.commit_config_edit();
        assert_eq!(app.config.project, "NewProject");
        assert_eq!(app.mode, Mode::Normal);
    }
}
