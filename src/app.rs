use crate::api::mock::MockClient;
use crate::api::models::{Iteration, Timeframe, WorkItem, WorkItemFilter};
use crate::api::WorkItemClient;
use crate::auth::mock::MockAuthenticator;
use crate::auth::Authenticator;
use crate::config::Config;
use crate::keys::{Action, Context};
use crate::ui::date_range::DateRangeInput;
use crate::ui::input::TextInput;
use crate::ui::iteration_picker::IterationPicker;
use crate::ui::picker::Picker;
use crate::ui::type_filter::TypeFilter;
use crate::ui::tags_editor::TagsEditor;
use ratatui::widgets::ListState;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

/// How often the app re-fetches server state to spot remote changes. Each poll
/// is several blocking REST round trips on the UI thread, so keep it infrequent
/// to avoid stutter; users can force an immediate refresh with the reload key.
const POLL_INTERVAL: Duration = Duration::from_secs(60);

/// How often the open item alone is re-fetched for the live-feed (cheaper and
/// more frequent than the full list refresh).
const LIVE_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Tab {
    Tree,
    #[default]
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
    /// Tags, edited in the floating fuzzy tags editor.
    Tags,
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
    EditField { key: "tags",        label: "Tags",        kind: FieldKind::Tags },
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
    // Tab bar / cycling order: Work Items first, the relationship Tree sits
    // after Detail and before Config.
    pub const ORDER: [Tab; 4] = [Tab::WorkItems, Tab::Detail, Tab::Tree, Tab::Config];
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
        field: &'static str,
        initial: String,
    },
    /// Resolving a conflict by hand-merging in the editor.
    Merge {
        id: u32,
        field: &'static str,
    },
    Comment,
    /// Editing an existing comment (last-write-wins, no conflict detection).
    EditComment {
        comment_id: u32,
        initial: String,
    },
}

/// A local edit that has not yet been pushed to the server. `base` is the
/// server value we believe is the common ancestor (captured at first edit).
#[derive(Clone, Debug)]
pub struct PendingEdit {
    pub field: &'static str,
    pub base: String,
    pub base_rev: u32,
    pub value: String,
}

/// A local, un-pushed comment change. Like field edits, these are held until
/// the user manually pushes.
#[derive(Clone, Debug)]
pub enum PendingComment {
    /// A brand-new comment to be added on push.
    Add { author: String, text: String },
    /// An edit to an existing comment, applied on push.
    Edit { comment_id: u32, text: String },
    /// Deletion of an existing comment, applied on push.
    Delete { comment_id: u32 },
}

/// One field whose local edit diverged from a newer server value.
#[derive(Clone, Debug)]
pub struct FieldConflict {
    pub field: &'static str,
    pub base: String,
    pub local: String,
    pub remote: String,
}

/// Floating resolution-options menu: shown only when a push (or an attempt to
/// edit a live-feed–flagged field) detects genuine divergence.
pub struct Resolution {
    pub id: u32,
    pub conflicts: Vec<FieldConflict>,
    /// Highlighted conflicting field (for per-field merge).
    pub selected: usize,
}

/// Live-feed status of a single field on the open item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldStatus {
    /// Someone changed this field upstream and it clashes with our pending edit.
    Conflicted,
    /// The field was updated upstream with no conflict (latest content adopted).
    Updated,
}

/// Fuzzy help popup state.
#[derive(Default)]
pub struct HelpState {
    pub input: TextInput,
    pub selected: usize,
}

/// One visible row of the relationship tree.
#[derive(Clone, Copy, Debug)]
pub enum TreeRow {
    /// A work-item node at the given indentation depth.
    Node { id: u32, depth: usize },
    /// A "…" continuation marker: more ancestors exist above the shown root.
    MoreAbove,
}

/// Lazily-walked relationship tree. The dataset lives in [`App::tree_cache`];
/// this holds only the expand state, selection and the flattened view.
#[derive(Default)]
pub struct TreeState {
    pub expanded: HashSet<u32>,
    /// Index into `flat`; always kept on a `Node` row.
    pub selected: usize,
    pub flat: Vec<TreeRow>,
}

/// An in-pane fzf-style filter: a set of committed query "tags" (all of which
/// must match) plus a live input while the user is typing a new one. Opened
/// with `/` in the Work Items and Tree panes.
#[derive(Default)]
pub struct FuzzyFilter {
    pub tags: Vec<String>,
    /// `Some` while the search bar is capturing input.
    pub input: Option<TextInput>,
}

impl FuzzyFilter {
    /// True while the search bar is open and capturing keys.
    pub fn searching(&self) -> bool {
        self.input.is_some()
    }

    /// True if anything is filtering the list (a committed tag or live input).
    pub fn active(&self) -> bool {
        !self.tags.is_empty()
            || self
                .input
                .as_ref()
                .is_some_and(|i| !i.value().trim().is_empty())
    }

    /// All query terms in effect: committed tags plus the live input.
    fn terms(&self) -> Vec<String> {
        let mut t = self.tags.clone();
        if let Some(inp) = &self.input {
            let v = inp.value();
            if !v.trim().is_empty() {
                t.push(v);
            }
        }
        t
    }

    /// True if `hay` fuzzy-matches every active term (AND semantics).
    pub fn matches(&self, hay: &str) -> bool {
        self.terms()
            .iter()
            .all(|term| crate::ui::fuzzy::matches(hay, term))
    }

    fn open(&mut self) {
        if self.input.is_none() {
            self.input = Some(TextInput::default());
        }
    }

    /// Commit the live input as a tag and close the search bar.
    fn commit(&mut self) {
        if let Some(inp) = &self.input {
            let v = inp.value().trim().to_string();
            if !v.is_empty() {
                self.tags.push(v);
            }
        }
        self.input = None;
    }

    fn cancel(&mut self) {
        self.input = None;
    }

    fn input_is_empty(&self) -> bool {
        self.input.as_ref().is_none_or(|i| i.value().is_empty())
    }
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
    /// In-pane fuzzy filter for the Work Items list (`/`).
    pub list_filter: FuzzyFilter,
    pub timeframe: Timeframe,
    /// Available iterations (sprints) for the configured team, fetched lazily.
    pub iterations: Vec<Iteration>,
    /// Iteration paths the list is filtered to (empty = no iteration filter).
    pub selected_iterations: Vec<String>,
    /// Floating iteration multi-select picker (modal), opened with `i`.
    pub iteration_picker: Option<IterationPicker>,
    /// Floating custom date-range entry (modal), opened with `c`.
    pub date_range: Option<DateRangeInput>,
    /// Whether iterations have been loaded & the default-to-current applied.
    pub iterations_initialized: bool,

    /// Work-item types the list is filtered to (empty = no type filter).
    pub item_types: Vec<String>,
    /// Floating work-item-type multi-select picker (modal), opened with `t`.
    pub type_picker: Option<TypeFilter>,
    /// Whether the default type filter has been applied on first async load.
    pub item_types_initialized: bool,

    pub tree: TreeState,
    /// In-pane fuzzy filter for the Tree view (`/`).
    pub tree_filter: FuzzyFilter,
    /// The tree's lazily-built cache of work items (fetched directly via the
    /// client, so it is independent of the timeframe / iteration / type
    /// filters). Walking the tree fetches one nest level at a time.
    pub tree_cache: HashMap<u32, WorkItem>,
    /// The work item the tree is anchored on (the user pressed `v` on it).
    pub tree_focus: Option<u32>,
    /// The topmost shown node (the focus's parent, or the focus itself).
    pub tree_root: Option<u32>,

    pub current: Option<WorkItem>,
    /// Titles of the open item's parent/children, fetched once when the item is
    /// opened (and refreshed in the background) so the relations pane never has
    /// to hit the network while the user navigates panes.
    pub related_titles: HashMap<u32, String>,
    pub detail_focus: DetailFocus,
    pub detail_selected: usize,
    pub comment_selected: usize,
    /// Floating field editor (modal), opened from the Info pane.
    pub info_editor: Option<InfoEditor>,
    /// Floating state picker (modal), opened when editing the State field and
    /// the backend supplied the item type's valid states.
    pub state_picker: Option<Picker>,
    /// Floating tags editor (modal), opened when editing the Tags field.
    pub tags_editor: Option<TagsEditor>,

    pub wizard: Option<WizardState>,
    pub config_edit: ConfigEdit,

    /// Un-pushed local edits to the open item, keyed by field (manual push).
    pub pending: Vec<PendingEdit>,
    /// Un-pushed local comment additions/edits to the open item.
    pub pending_comments: Vec<PendingComment>,
    /// Floating resolution-options menu, shown on genuine divergence.
    pub resolution: Option<Resolution>,
    /// Live-feed markers for the open item's fields (⚠ conflict / ✓ updated).
    pub field_status: HashMap<&'static str, FieldStatus>,

    pub show_help: bool,
    pub help: HelpState,

    pub pending_g: bool,
    pub pending_editor: Option<EditorRequest>,
    /// Seed text for a pending Merge editor request (conflict markers).
    pub merge_seed: Option<String>,

    started: Instant,
    last_poll: Instant,

    /// True while a background refresh is in flight (drives the status spinner).
    loading: bool,
    /// Set when a refresh is requested while one is already running, so we fire
    /// a fresh one as soon as the in-flight refresh completes.
    pending_refresh: bool,
    refresh_tx: Sender<RefreshOutcome>,
    refresh_rx: Receiver<RefreshOutcome>,

    /// Live-feed: periodic lightweight re-fetch of just the open item.
    last_live: Instant,
    live_inflight: bool,
    live_tx: Sender<CurrentOutcome>,
    live_rx: Receiver<CurrentOutcome>,

    /// True while a manual push is uploading in the background (drives the status
    /// bar's push spinner). The UI stays interactive during the push.
    pushing: bool,
    push_tx: Sender<PushOutcome>,
    push_rx: Receiver<PushOutcome>,

    /// Tree fetching runs on a background thread so walking the tree never
    /// freezes the UI; nodes populate progressively as they arrive.
    tree_pending: HashSet<u32>,
    tree_tx: Sender<(u32, Result<WorkItem, String>)>,
    tree_rx: Receiver<(u32, Result<WorkItem, String>)>,
    /// When the focus item arrives, re-root the view on its parent.
    tree_reroot_focus: Option<u32>,
    /// Move the cursor onto this node once it becomes visible.
    tree_select_after: Option<u32>,
}

/// Result of a background push of the open item's pending edits.
enum PushOutcome {
    /// Server diverged: genuine conflicts detected, nothing was uploaded. The
    /// in-flight edits are returned so they can be restored for resolution.
    Conflicts {
        id: u32,
        conflicts: Vec<FieldConflict>,
        pending: Vec<PendingEdit>,
        comments: Vec<PendingComment>,
    },
    /// Uploaded `total` change(s), `failed` of which errored.
    Done { id: u32, total: usize, failed: usize },
    /// The pre-flight fetch failed; edits are returned to restore them.
    Error {
        message: String,
        pending: Vec<PendingEdit>,
        comments: Vec<PendingComment>,
    },
}

/// Result of a live-feed fetch of a single (open) work item.
struct CurrentOutcome {
    id: u32,
    item: Result<WorkItem, String>,
    /// Related-item titles refreshed alongside.
    related: HashMap<u32, String>,
}

/// Result of a background refresh, sent from the worker thread to the UI thread.
struct RefreshOutcome {
    /// Filter the fetch was issued for; lets us drop stale results.
    filter: WorkItemFilter,
    items: Result<Vec<WorkItem>, String>,
    /// Iterations fetched alongside (only when the list was empty / requested).
    iterations: Option<Vec<Iteration>>,
    /// Refreshed copy of the open item, if one was focused at request time.
    current: Option<Result<WorkItem, String>>,
    /// Titles of the refreshed item's related (parent/child) work items.
    related: HashMap<u32, String>,
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
        let (refresh_tx, refresh_rx) = std::sync::mpsc::channel();
        let (live_tx, live_rx) = std::sync::mpsc::channel();
        let (push_tx, push_rx) = std::sync::mpsc::channel();
        let (tree_tx, tree_rx) = std::sync::mpsc::channel();
        let mut app = Self {
            wizard,
            // Default landing view: the current iteration's work items.
            tab: Tab::WorkItems,
            mode: Mode::Normal,
            should_quit: false,
            status: "Welcome to lazyaz — press ? for help".into(),
            client,
            auth,
            items: Vec::new(),
            list_state: ListState::default(),
            list_filter: FuzzyFilter::default(),
            timeframe: Timeframe::default(),
            iterations: Vec::new(),
            selected_iterations: Vec::new(),
            iteration_picker: None,
            date_range: None,
            iterations_initialized: false,
            item_types: Vec::new(),
            type_picker: None,
            item_types_initialized: false,
            tree: TreeState::default(),
            tree_filter: FuzzyFilter::default(),
            tree_cache: HashMap::new(),
            tree_focus: None,
            tree_root: None,
            current: None,
            related_titles: HashMap::new(),
            detail_focus: DetailFocus::Info,
            detail_selected: 0,
            comment_selected: 0,
            info_editor: None,
            state_picker: None,
            tags_editor: None,
            config_edit: ConfigEdit {
                selected: 0,
                buffer: TextInput::default(),
            },
            pending: Vec::new(),
            pending_comments: Vec::new(),
            resolution: None,
            field_status: HashMap::new(),
            show_help: false,
            help: HelpState::default(),
            pending_g: false,
            pending_editor: None,
            merge_seed: None,
            started: now,
            last_poll: now,
            loading: false,
            pending_refresh: false,
            refresh_tx,
            refresh_rx,
            last_live: now,
            live_inflight: false,
            live_tx,
            live_rx,
            pushing: false,
            push_tx,
            push_rx,
            tree_pending: HashSet::new(),
            tree_tx,
            tree_rx,
            tree_reroot_focus: None,
            tree_select_after: None,
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
        self.selected_iterations = s.selected_iterations;
        self.item_types = s.item_types;
        // Respect the restored (possibly empty) iteration selection rather than
        // auto-defaulting to the current sprint on first iteration load.
        self.iterations_initialized = true;
        // Likewise respect the restored (possibly empty) type selection rather
        // than re-applying the User Story + Feature default.
        self.item_types_initialized = true;
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
                self.set_current(item);
                self.detail_selected = s.detail_selected;
            }
        // goto_tab anchors the relationship tree if we land on the Tree tab.
        self.goto_tab(s.tab);
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
            selected_iterations: self.selected_iterations.clone(),
            item_types: self.item_types.clone(),
            current_id: self.current.as_ref().map(|w| w.id),
            tree_expanded: self.tree.expanded.iter().copied().collect(),
            tree_selected: self.tree.selected,
            list_selected: self.list_state.selected().unwrap_or(0),
            detail_selected: self.detail_selected,
        };
        let _ = state.save();
    }

    pub fn context(&self) -> Context {
        if self.resolution.is_some() {
            return Context::Conflict;
        }
        if self.iteration_picker.is_some() {
            return Context::IterationFilter;
        }
        if self.date_range.is_some() {
            return Context::IterationFilter;
        }
        if self.type_picker.is_some() {
            return Context::TypeFilter;
        }
        if self.tags_editor.is_some() || self.state_picker.is_some() {
            return Context::InfoEditor;
        }
        if self.info_editor.is_some() {
            return Context::InfoEditor;
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
        let filter = self.filter();
        match self.client.list_assigned(&filter) {
            Ok(items) => self.apply_items(items),
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    /// The active server-side filter (timeframe + selected iterations + types).
    pub fn filter(&self) -> WorkItemFilter {
        WorkItemFilter {
            timeframe: self.timeframe,
            iterations: self.selected_iterations.clone(),
            item_types: self.item_types.clone(),
        }
    }

    /// Short status-bar label summarising the iteration filter.
    pub fn iteration_filter_label(&self) -> String {
        match self.selected_iterations.len() {
            0 => "iter:all".to_string(),
            1 => {
                let path = &self.selected_iterations[0];
                let name = self
                    .iterations
                    .iter()
                    .find(|i| &i.path == path)
                    .map(|i| i.name.as_str())
                    .unwrap_or_else(|| path.rsplit(['\\', '/']).next().unwrap_or(path));
                format!("iter:{name}")
            }
            n => format!("iter:{n} sprints"),
        }
    }

    /// The single active time filter (iteration-based or timeframe-based — they
    /// are mutually exclusive) for the status bar.
    pub fn time_filter_label(&self) -> String {
        if !self.selected_iterations.is_empty() {
            self.iteration_filter_label()
        } else {
            format!("tf:{}", self.timeframe.label())
        }
    }

    /// Short status-bar label summarising the type filter.
    pub fn type_filter_label(&self) -> String {
        match self.item_types.len() {
            0 => "type:all".to_string(),
            1 => format!("type:{}", self.item_types[0]),
            n => format!("type:{n} types"),
        }
    }

    /// Adopt a freshly fetched item list: clamp selections, rebuild the tree,
    /// and update the status line. Shared by the sync and async refresh paths.
    fn apply_items(&mut self, items: Vec<WorkItem>) {
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
        // The relationship tree is intentionally NOT rebuilt here: it is scoped
        // to a focus item and independent of the (timeframe/iteration/type)
        // filter that produced this list.
        self.clamp_list_selection();
        self.status = format!(
            "{} item(s) · timeframe: {}",
            self.items.len(),
            self.timeframe.label()
        );
    }

    // --- in-pane fuzzy filter (`/`) ---

    /// Haystack string a work item is fuzzy-matched against.
    fn item_haystack(w: &WorkItem) -> String {
        format!("#{} {} {} {}", w.id, w.item_type, w.title, w.tags.join(" "))
    }

    /// Indices into `items` that pass the Work Items fuzzy filter.
    pub fn visible_item_indices(&self) -> Vec<usize> {
        if !self.list_filter.active() {
            return (0..self.items.len()).collect();
        }
        self.items
            .iter()
            .enumerate()
            .filter(|(_, w)| self.list_filter.matches(&Self::item_haystack(w)))
            .map(|(i, _)| i)
            .collect()
    }

    /// The currently selected work item, mapping the list cursor through the
    /// fuzzy filter to the underlying `items` entry.
    fn selected_item(&self) -> Option<&WorkItem> {
        let vis = self.visible_item_indices();
        let pos = self.list_state.selected()?;
        vis.get(pos).and_then(|i| self.items.get(*i))
    }

    fn clamp_list_selection(&mut self) {
        let n = self.visible_item_indices().len();
        if n == 0 {
            self.list_state.select(None);
        } else {
            let pos = self.list_state.selected().unwrap_or(0).min(n - 1);
            self.list_state.select(Some(pos));
        }
    }

    fn active_filter(&self) -> Option<&FuzzyFilter> {
        match self.context() {
            Context::WorkItems => Some(&self.list_filter),
            Context::Tree => Some(&self.tree_filter),
            _ => None,
        }
    }

    fn active_filter_mut(&mut self) -> Option<&mut FuzzyFilter> {
        match self.context() {
            Context::WorkItems => Some(&mut self.list_filter),
            Context::Tree => Some(&mut self.tree_filter),
            _ => None,
        }
    }

    /// True while the active pane's search bar is capturing input.
    pub fn filter_searching(&self) -> bool {
        self.active_filter().is_some_and(|f| f.searching())
    }

    /// True if a fuzzy filter is in effect in the active pane.
    pub fn filter_active(&self) -> bool {
        self.active_filter().is_some_and(|f| f.active())
    }

    pub fn open_search(&mut self) {
        if let Some(f) = self.active_filter_mut() {
            f.open();
        }
    }

    pub fn search_input_empty(&self) -> bool {
        self.active_filter().is_none_or(|f| f.input_is_empty())
    }

    /// Apply a key to the search input (live filtering).
    pub fn search_handle(&mut self, key: crossterm::event::KeyEvent) {
        if let Some(f) = self.active_filter_mut()
            && let Some(inp) = f.input.as_mut() {
                inp.handle(key);
            }
        self.after_filter_change();
    }

    pub fn search_commit(&mut self) {
        if let Some(f) = self.active_filter_mut() {
            f.commit();
        }
        self.after_filter_change();
    }

    pub fn search_cancel(&mut self) {
        if let Some(f) = self.active_filter_mut() {
            f.cancel();
        }
        self.after_filter_change();
    }

    pub fn search_pop_tag(&mut self) {
        if let Some(f) = self.active_filter_mut() {
            f.tags.pop();
        }
        self.after_filter_change();
    }

    /// Clear all filter tags + input in the active pane.
    pub fn clear_filter(&mut self) {
        if let Some(f) = self.active_filter_mut() {
            f.tags.clear();
            f.input = None;
        }
        self.after_filter_change();
    }

    /// Re-clamp the selection (and rebuild the tree) after the filter changes.
    fn after_filter_change(&mut self) {
        match self.context() {
            Context::WorkItems => self.clamp_list_selection(),
            Context::Tree => self.rebuild_tree(),
            _ => {}
        }
    }

    /// Kick off a non-blocking refresh on a background thread. The UI keeps
    /// running (showing a spinner) and picks up the result via [`drain_refresh`].
    /// If a refresh is already running, remembers to fire another when it ends.
    pub fn request_refresh(&mut self) {
        if self.loading {
            self.pending_refresh = true;
            return;
        }
        self.loading = true;
        self.status = "loading…".into();
        let client = self.client.clone_box();
        let tx = self.refresh_tx.clone();
        let filter = self.filter();
        let want_iterations = self.iterations.is_empty();
        let current_id = self.current.as_ref().map(|w| w.id);
        std::thread::spawn(move || {
            let items = client.list_assigned(&filter).map_err(|e| e.to_string());
            let iterations = want_iterations.then(|| client.list_iterations());
            let current = current_id.map(|id| client.get(id).map_err(|e| e.to_string()));
            // Also refresh the related items' titles for the relations pane.
            let mut related = HashMap::new();
            if let Some(Ok(item)) = &current {
                let mut ids = Vec::new();
                if let Some(p) = item.parent {
                    ids.push(p);
                }
                ids.extend(item.children.iter().copied());
                for id in ids {
                    if let Ok(rel) = client.get(id) {
                        related.insert(id, rel.title);
                    }
                }
            }
            let _ = tx.send(RefreshOutcome {
                filter,
                items,
                iterations,
                current,
                related,
            });
        });
    }

    /// Apply any completed background refreshes. Called every loop iteration;
    /// non-blocking, so it never stalls the UI.
    pub fn drain_refresh(&mut self) {
        while let Ok(outcome) = self.refresh_rx.try_recv() {
            self.loading = false;
            let RefreshOutcome {
                filter,
                items,
                iterations,
                current,
                related,
            } = outcome;
            // Store any freshly fetched iterations; on first load default the
            // filter to the current sprint and re-issue the fetch.
            if let Some(iters) = iterations {
                self.iterations = iters;
                if !self.iterations_initialized {
                    self.iterations_initialized = true;
                    if let Some(cur) = self.iterations.iter().find(|i| i.is_current) {
                        self.selected_iterations = vec![cur.path.clone()];
                        self.request_refresh();
                    }
                }
            }
            // On the first async load, default the type filter to User Stories
            // and Features (kept out of the sync path so tests see all types).
            if !self.item_types_initialized {
                self.item_types_initialized = true;
                self.item_types = crate::api::models::DEFAULT_WORK_ITEM_TYPES
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                self.request_refresh();
            }
            // Ignore results for a filter the user has since switched away from.
            if filter != self.filter() {
                continue;
            }
            match items {
                Ok(items) => self.apply_items(items),
                Err(e) => self.status = format!("error: {e}"),
            }
            if let Some(Ok(item)) = current {
                // Only adopt it if the same item is still open. Route through the
                // live-feed diff so upstream changes get ✓/⚠ markers.
                if self.current.as_ref().map(|c| c.id) == Some(item.id) {
                    self.related_titles.extend(related);
                    self.apply_remote_current(item);
                }
            }
        }
        // A refresh was requested while one was in flight — run it now.
        if !self.loading && self.pending_refresh {
            self.pending_refresh = false;
            self.request_refresh();
        }
    }

    /// Periodic refresh from the server (called by the main loop). Non-blocking.
    pub fn poll(&mut self) {
        if self.last_poll.elapsed() < POLL_INTERVAL {
            return;
        }
        self.last_poll = Instant::now();
        self.request_refresh();
    }

    /// Live-feed: periodically re-fetch just the open item so we notice a
    /// teammate's edits without waiting for the full-list poll. Non-blocking.
    pub fn live_poll(&mut self) {
        if self.live_inflight || self.last_live.elapsed() < LIVE_INTERVAL {
            return;
        }
        let Some(id) = self.current.as_ref().map(|w| w.id) else {
            return;
        };
        self.last_live = Instant::now();
        self.live_inflight = true;
        let client = self.client.clone_box();
        let tx = self.live_tx.clone();
        let related_ids = self.related_ids();
        std::thread::spawn(move || {
            let item = client.get(id).map_err(|e| e.to_string());
            let mut related = HashMap::new();
            for rid in related_ids {
                if let Ok(rel) = client.get(rid) {
                    related.insert(rid, rel.title);
                }
            }
            let _ = tx.send(CurrentOutcome { id, item, related });
        });
    }

    /// Apply any completed live-feed fetches. Non-blocking; called each loop.
    pub fn drain_live(&mut self) {
        while let Ok(outcome) = self.live_rx.try_recv() {
            self.live_inflight = false;
            let CurrentOutcome { id, item, related } = outcome;
            if self.current.as_ref().map(|c| c.id) != Some(id) {
                continue; // user moved on to another item
            }
            if let Ok(item) = item {
                self.related_titles.extend(related);
                self.apply_remote_current(item);
            }
        }
    }

    /// Adopt a freshly fetched server copy of the open item, diffing it against
    /// what we had and our pending edits to drive the ⚠/✓ field markers.
    fn apply_remote_current(&mut self, remote: WorkItem) {
        let old = self.current.clone();
        for f in EDITABLE_FIELDS.iter() {
            let key = f.key;
            let new_val = item_field_value(&remote, key);
            let old_val = old
                .as_ref()
                .map(|w| item_field_value(w, key))
                .unwrap_or_default();
            if new_val == old_val {
                continue; // no upstream change for this field
            }
            // Upstream changed this field. If we have a divergent pending edit,
            // it's a conflict; otherwise the latest content was simply adopted.
            match self.pending.iter().find(|p| p.field == key) {
                Some(p) if p.value != new_val => {
                    self.field_status.insert(key, FieldStatus::Conflicted);
                }
                _ => {
                    self.field_status.insert(key, FieldStatus::Updated);
                }
            }
        }
        self.set_current(remote);
    }

    /// True while a background refresh is running.
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Current frame of the loading spinner (advances with wall-clock time).
    pub fn spinner_frame(&self) -> char {
        const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let i = (self.started.elapsed().as_millis() / 90) as usize % FRAMES.len();
        FRAMES[i]
    }

    /// Set the open item and refresh the cached titles of its related items.
    /// Switching to a *different* item drops any pending edits and live markers.
    fn set_current(&mut self, item: WorkItem) {
        let changed = self.current.as_ref().map(|c| c.id) != Some(item.id);
        if changed {
            self.pending.clear();
            self.pending_comments.clear();
            self.field_status.clear();
            self.resolution = None;
            self.state_picker = None;
            self.tags_editor = None;
            self.last_live = Instant::now();
        }
        self.current = Some(item);
        self.cache_related_titles();
    }

    /// Fetch (once) the titles of the open item's parent/children into the
    /// cache. Called only when the open item changes — never per render frame.
    fn cache_related_titles(&mut self) {
        self.prune_related_titles();
        for id in self.related_ids() {
            if let std::collections::hash_map::Entry::Vacant(slot) =
                self.related_titles.entry(id)
                && let Ok(item) = self.client.get(id)
            {
                slot.insert(item.title);
            }
        }
    }

    /// Drop cached titles that are no longer related to the open item.
    fn prune_related_titles(&mut self) {
        let ids = self.related_ids();
        self.related_titles.retain(|id, _| ids.contains(id));
    }

    /// Cached title of a related work item, if known. Cheap; no network.
    pub fn related_title(&self, id: u32) -> Option<&str> {
        self.related_titles.get(&id).map(String::as_str)
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

    // --- tree (lazy, level-by-level) ---

    pub fn tree_item(&self, id: u32) -> Option<&WorkItem> {
        self.tree_cache.get(&id)
    }

    pub fn tree_has_children(&self, id: u32) -> bool {
        self.tree_cache
            .get(&id)
            .is_some_and(|w| !w.children.is_empty())
    }

    /// True while tree nodes are being fetched in the background.
    pub fn tree_loading(&self) -> bool {
        !self.tree_pending.is_empty()
    }

    /// Request the work items needed to render the current tree (the root plus
    /// the children of every expanded node) on a background thread. Already
    /// cached / in-flight ids are skipped, so this is cheap to call repeatedly.
    fn tree_ensure_fetched(&mut self) {
        let mut want: HashSet<u32> = HashSet::new();
        if let Some(root) = self.tree_root {
            want.insert(root);
            let mut stack = vec![root];
            while let Some(id) = stack.pop() {
                if self.tree.expanded.contains(&id)
                    && let Some(item) = self.tree_cache.get(&id) {
                        for c in &item.children {
                            if want.insert(*c) {
                                stack.push(*c);
                            }
                        }
                    }
            }
        }
        let needed: Vec<u32> = want
            .into_iter()
            .filter(|id| !self.tree_cache.contains_key(id) && !self.tree_pending.contains(id))
            .collect();
        if needed.is_empty() {
            return;
        }
        for id in &needed {
            self.tree_pending.insert(*id);
        }
        let client = self.client.clone_box();
        let tx = self.tree_tx.clone();
        std::thread::spawn(move || {
            for id in needed {
                let _ = tx.send((id, client.get(id).map_err(|e| e.to_string())));
            }
        });
    }

    /// Expand a node, fetching its children in the background.
    fn tree_expand_node(&mut self, id: u32) {
        self.tree.expanded.insert(id);
        self.tree_ensure_fetched();
    }

    /// Drain any tree fetches that have completed. Non-blocking; populates the
    /// tree progressively so the UI never freezes while walking.
    pub fn drain_tree(&mut self) {
        let mut changed = false;
        while let Ok((id, res)) = self.tree_rx.try_recv() {
            self.tree_pending.remove(&id);
            if let Ok(item) = res {
                self.tree_cache.insert(id, item);
            }
            // Once the focus arrives we know its parent — re-root the view there
            // so the focus is shown with its siblings.
            if self.tree_reroot_focus == Some(id) {
                self.tree_reroot_focus = None;
                if let Some(p) = self.tree_cache.get(&id).and_then(|w| w.parent) {
                    self.tree_root = Some(p);
                    self.tree.expanded.insert(p);
                }
            }
            changed = true;
        }
        if changed {
            self.tree_ensure_fetched(); // pull in children newly discovered above
            self.rebuild_tree();
            // Land the cursor on a deferred target once it becomes visible.
            if let Some(target) = self.tree_select_after
                && self
                    .tree
                    .flat
                    .iter()
                    .any(|r| matches!(r, TreeRow::Node { id, .. } if *id == target))
                {
                    self.tree_select_node(target);
                    self.tree_select_after = None;
                }
        }
    }

    /// Anchor the tree on `focus`: show its parent (one level up), the focus and
    /// its siblings (current level), and the focus's children (one level down).
    /// Fetching happens in the background; nodes appear progressively. Clears the
    /// cache so this doubles as a refresh.
    fn load_tree_for(&mut self, focus: u32) {
        self.tree_cache.clear();
        self.tree_pending.clear();
        self.tree.expanded.clear();
        self.tree_focus = Some(focus);
        // Show something as soon as the focus loads; re-root onto its parent once
        // we learn it (handled in drain_tree).
        self.tree_root = Some(focus);
        self.tree_reroot_focus = Some(focus);
        self.tree_select_after = Some(focus);
        self.tree.expanded.insert(focus);
        self.tree.flat.clear();
        self.tree.selected = 0;
        self.tree_ensure_fetched();
    }

    /// Re-fetch the current tree from scratch (the cache is the source of truth
    /// between refreshes).
    fn refresh_tree(&mut self) {
        if let Some(focus) = self.tree_focus {
            self.load_tree_for(focus);
            self.status = "refreshing tree…".into();
        }
    }

    fn rebuild_tree(&mut self) {
        let mut flat = Vec::new();
        if let Some(root) = self.tree_root {
            // A "…" marker when there are ancestors above the shown root.
            if self.tree_cache.get(&root).and_then(|w| w.parent).is_some() {
                flat.push(TreeRow::MoreAbove);
            }
            self.tree_flatten(root, 0, &mut flat);
        }
        if self.tree_filter.active() {
            flat = self.apply_tree_filter(flat);
        }
        self.tree.flat = flat;
        self.clamp_tree_selection();
    }

    /// Keep only tree rows that fuzzy-match the filter, plus the ancestor path
    /// of each match (so the matches stay located in the hierarchy). Drops the
    /// "…" marker while filtering.
    fn apply_tree_filter(&self, flat: Vec<TreeRow>) -> Vec<TreeRow> {
        let shown: HashSet<u32> = flat
            .iter()
            .filter_map(|r| match r {
                TreeRow::Node { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        let mut keep: HashSet<u32> = HashSet::new();
        for id in &shown {
            let matches = self
                .tree_cache
                .get(id)
                .is_some_and(|w| self.tree_filter.matches(&Self::item_haystack(w)));
            if matches {
                keep.insert(*id);
                // Walk up the ancestor chain, within the shown set.
                let mut cur = *id;
                while let Some(p) = self.tree_cache.get(&cur).and_then(|w| w.parent) {
                    if shown.contains(&p) {
                        keep.insert(p);
                        cur = p;
                    } else {
                        break;
                    }
                }
            }
        }
        flat.into_iter()
            .filter(|r| matches!(r, TreeRow::Node { id, .. } if keep.contains(id)))
            .collect()
    }

    fn tree_flatten(&self, id: u32, depth: usize, out: &mut Vec<TreeRow>) {
        out.push(TreeRow::Node { id, depth });
        if !self.tree.expanded.contains(&id) {
            return;
        }
        if let Some(item) = self.tree_cache.get(&id) {
            for c in &item.children {
                if self.tree_cache.contains_key(c) {
                    self.tree_flatten(*c, depth + 1, out);
                }
            }
        }
    }

    /// Flat indices that are selectable `Node` rows.
    fn tree_node_positions(&self) -> Vec<usize> {
        self.tree
            .flat
            .iter()
            .enumerate()
            .filter(|(_, r)| matches!(r, TreeRow::Node { .. }))
            .map(|(i, _)| i)
            .collect()
    }

    /// Keep `tree.selected` on a `Node` row after a rebuild.
    fn clamp_tree_selection(&mut self) {
        let nodes = self.tree_node_positions();
        if nodes.is_empty() {
            self.tree.selected = 0;
        } else if !nodes.contains(&self.tree.selected) {
            self.tree.selected = *nodes
                .iter()
                .min_by_key(|p| (**p as isize - self.tree.selected as isize).abs())
                .unwrap();
        }
    }

    /// Pick the anchor for the tree (open item → list selection → first item)
    /// and (re)load it if not already shown.
    fn ensure_tree_anchored(&mut self) {
        let anchor = self
            .current
            .as_ref()
            .map(|w| w.id)
            .or_else(|| {
                self.list_state
                    .selected()
                    .and_then(|i| self.items.get(i))
                    .map(|w| w.id)
            })
            .or_else(|| self.items.first().map(|w| w.id));
        match anchor {
            Some(id) if self.tree_focus != Some(id) || self.tree_cache.is_empty() => {
                self.load_tree_for(id)
            }
            Some(_) => {}
            None => {
                self.tree_cache.clear();
                self.tree.flat.clear();
                self.tree_focus = None;
                self.tree_root = None;
            }
        }
    }

    /// Switch tabs, anchoring the relationship tree when landing on it.
    fn goto_tab(&mut self, tab: Tab) {
        self.tab = tab;
        if tab == Tab::Tree {
            self.ensure_tree_anchored();
        }
    }

    /// `v` from the work-items list: focus the tree on the selected item.
    fn view_in_tree(&mut self) {
        let Some(id) = self.selected_item().map(|w| w.id) else {
            self.status = "no item selected".into();
            return;
        };
        self.load_tree_for(id);
        self.tab = Tab::Tree;
        self.status = format!("#{id} — related items in tree view");
    }

    fn tree_selected_id(&self) -> Option<u32> {
        match self.tree.flat.get(self.tree.selected) {
            Some(TreeRow::Node { id, .. }) => Some(*id),
            _ => None,
        }
    }

    /// The work item the user is currently pointed at, for whichever item view
    /// is active (tree node, list row, or the open detail item).
    fn active_item_id(&self) -> Option<u32> {
        match self.context() {
            Context::Tree => self.tree_selected_id(),
            Context::WorkItems => self.selected_item().map(|w| w.id),
            Context::Detail => self.current.as_ref().map(|w| w.id),
            _ => None,
        }
    }

    /// Browser URL for a work item in the Azure DevOps web UI. `org_url` may be
    /// a bare org name or a full URL; the project segment is included when set.
    fn work_item_web_url(&self, id: u32) -> Option<String> {
        let org = self.config.org_url.trim().trim_end_matches('/');
        if org.is_empty() {
            return None;
        }
        let base = if org.starts_with("http://") || org.starts_with("https://") {
            org.to_string()
        } else {
            format!("https://dev.azure.com/{org}")
        };
        let project = self.config.project.trim();
        if project.is_empty() {
            Some(format!("{base}/_workitems/edit/{id}"))
        } else {
            Some(format!(
                "{base}/{}/_workitems/edit/{id}",
                project.replace(' ', "%20")
            ))
        }
    }

    /// Open the active work item in the system browser.
    fn open_in_browser(&mut self) {
        let Some(id) = self.active_item_id() else {
            self.status = "no item selected".into();
            return;
        };
        let Some(url) = self.work_item_web_url(id) else {
            self.status = "set org_url in Config to open items in the browser".into();
            return;
        };
        match open::that(&url) {
            Ok(()) => self.status = format!("opening #{id} in the browser…"),
            Err(e) => self.status = format!("could not open browser: {e}"),
        }
    }

    fn open_id(&mut self, id: u32) {
        match self.client.get(id) {
            Ok(item) => {
                self.set_current(item);
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
            Action::NextTab => {
                self.goto_tab(Tab::ORDER[(self.tab.index() + 1) % Tab::ORDER.len()])
            }
            Action::PrevTab => self
                .goto_tab(Tab::ORDER[(self.tab.index() + Tab::ORDER.len() - 1) % Tab::ORDER.len()]),
            Action::GotoTab(n) => {
                if let Some(t) = Tab::ORDER.get(n as usize) {
                    self.goto_tab(*t);
                }
            }
            Action::Down => self.move_selection(1),
            Action::Up => self.move_selection(-1),
            Action::Top => self.set_selection(0),
            Action::Bottom => self.set_selection(isize::MAX),
            Action::Reload => self.request_refresh(),
            Action::OpenIterationFilter => self.open_iteration_picker(),
            Action::OpenTypeFilter => self.open_type_picker(),
            Action::OpenTimeframeFilter => {
                self.date_range = Some(DateRangeInput::new(self.timeframe));
            }
            Action::Open => self.open(),
            Action::Back => self.tab = Tab::WorkItems,
            Action::ViewInTree => self.view_in_tree(),
            Action::FocusNext => self.detail_focus = self.detail_focus.step(1),
            Action::FocusPrev => self.detail_focus = self.detail_focus.step(-1),
            Action::TreeExpand => self.tree_set_expanded(true),
            Action::TreeCollapse => self.tree_set_expanded(false),
            Action::TreeNextSibling => self.tree_sibling(1),
            Action::TreePrevSibling => self.tree_sibling(-1),
            Action::TreeLevelIn => self.tree_level_in(),
            Action::TreeLevelOut => self.tree_level_out(),
            Action::RefreshTree => self.refresh_tree(),
            Action::Edit => self.request_field_edit("description"),
            Action::EditNotes => self.request_field_edit("notes"),
            Action::AddComment => {
                if self.current.is_some() {
                    self.pending_editor = Some(EditorRequest::Comment);
                }
            }
            Action::SimulateRemote => self.simulate_remote(),
            Action::DeleteComment => self.request_comment_delete(),
            Action::Push => self.push_pending(),
            Action::OpenInBrowser => self.open_in_browser(),
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
                    self.tree_expand_node(id); // lazily fetch this node's children
                }
            } else {
                self.tree.expanded.remove(&id);
            }
            self.rebuild_tree();
        }
    }

    /// `(id, depth)` of the currently selected tree node, if any.
    fn tree_current(&self) -> Option<(u32, usize)> {
        match self.tree.flat.get(self.tree.selected) {
            Some(TreeRow::Node { id, depth }) => Some((*id, *depth)),
            _ => None,
        }
    }

    /// Move the selection onto the row for `id` (no-op if it isn't visible).
    fn tree_select_node(&mut self, id: u32) {
        if let Some(pos) = self
            .tree
            .flat
            .iter()
            .position(|r| matches!(r, TreeRow::Node { id: i, .. } if *i == id))
        {
            self.tree.selected = pos;
        }
    }

    /// `J`/`K`: jump to the next/previous sibling (a node sharing the same
    /// parent), skipping over any expanded descendants in between.
    fn tree_sibling(&mut self, delta: isize) {
        let Some((id, _)) = self.tree_current() else {
            return;
        };
        let parent = self.tree_cache.get(&id).and_then(|w| w.parent);
        let sibs: Vec<u32> = self
            .tree
            .flat
            .iter()
            .filter_map(|r| match r {
                TreeRow::Node { id: i, .. }
                    if self.tree_cache.get(i).and_then(|w| w.parent) == parent =>
                {
                    Some(*i)
                }
                _ => None,
            })
            .collect();
        if sibs.is_empty() {
            return;
        }
        let cur = sibs.iter().position(|s| *s == id).unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, sibs.len() as isize - 1) as usize;
        self.tree_select_node(sibs[next]);
    }

    /// Select `id` now if it's visible, otherwise once it arrives (drain_tree).
    fn tree_select_or_defer(&mut self, id: u32) {
        if self
            .tree
            .flat
            .iter()
            .any(|r| matches!(r, TreeRow::Node { id: i, .. } if *i == id))
        {
            self.tree_select_node(id);
        } else {
            self.tree_select_after = Some(id);
        }
    }

    /// `L`: descend one level — select the first child of the current node
    /// (expanding it first if needed; the child may load in the background).
    fn tree_level_in(&mut self) {
        let Some((id, _)) = self.tree_current() else {
            return;
        };
        if !self.tree_has_children(id) {
            return;
        }
        let first_child = self
            .tree_cache
            .get(&id)
            .and_then(|w| w.children.first().copied());
        if !self.tree.expanded.contains(&id) {
            self.tree_expand_node(id);
            self.rebuild_tree();
        }
        if let Some(child) = first_child {
            self.tree_select_or_defer(child);
        }
    }

    /// `H`: ascend one level — select the parent of the current node. If the
    /// parent isn't shown (the current node is the root, with a "…" above), pull
    /// in the parent level so the user can keep walking the tree upwards.
    fn tree_level_out(&mut self) {
        let Some((id, _)) = self.tree_current() else {
            return;
        };
        let Some(parent) = self.tree_cache.get(&id).and_then(|w| w.parent) else {
            return;
        };
        let parent_visible = self
            .tree
            .flat
            .iter()
            .any(|r| matches!(r, TreeRow::Node { id: i, .. } if *i == parent));
        if parent_visible {
            self.tree_select_node(parent);
        } else {
            // Current node is the shown root: fetch the parent level (in the
            // background), re-root on it and move onto the parent as it arrives.
            self.tree_root = Some(parent);
            self.tree_expand_node(parent); // fetches parent + its children
            self.rebuild_tree();
            self.tree_select_or_defer(parent);
            self.status = format!("#{parent} — walking up a level…");
        }
    }

    fn move_selection(&mut self, delta: isize) {
        match self.context() {
            Context::Tree => {
                // Step between selectable Node rows (skip the "…" marker).
                let nodes = self.tree_node_positions();
                if !nodes.is_empty() {
                    let cur = nodes.iter().position(|p| *p == self.tree.selected).unwrap_or(0)
                        as isize;
                    let next = (cur + delta).clamp(0, nodes.len() as isize - 1) as usize;
                    self.tree.selected = nodes[next];
                }
            }
            Context::WorkItems => {
                let n = self.visible_item_indices().len();
                if n == 0 {
                    return;
                }
                let cur = self.list_state.selected().unwrap_or(0) as isize;
                let next = (cur + delta).clamp(0, n as isize - 1);
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
                let nodes = self.tree_node_positions();
                if !nodes.is_empty() {
                    let idx = target.clamp(0, nodes.len() as isize - 1) as usize;
                    self.tree.selected = nodes[idx];
                }
            }
            Context::WorkItems => {
                let n = self.visible_item_indices().len();
                if n > 0 {
                    let idx = target.clamp(0, n as isize - 1) as usize;
                    self.list_state.select(Some(idx));
                }
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
                if let Some(item) = self.selected_item() {
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

    /// Display value of an editable field on the open item, including any local
    /// un-pushed edit (pending overlay).
    pub fn info_field_value(&self, key: &str) -> String {
        self.effective_field_value(key)
    }

    /// The server value of a field on the open item (no pending overlay).
    pub fn server_field_value(&self, key: &str) -> String {
        self.current
            .as_ref()
            .map(|w| item_field_value(w, key))
            .unwrap_or_default()
    }

    /// The value the user currently sees: a pending local edit if present,
    /// otherwise the server value.
    pub fn effective_field_value(&self, key: &str) -> String {
        if let Some(p) = self.pending.iter().find(|p| p.field == key) {
            return p.value.clone();
        }
        self.server_field_value(key)
    }

    /// Canonicalize a field name to its `'static` key from [`EDITABLE_FIELDS`].
    fn static_field_key(field: &str) -> Option<&'static str> {
        EDITABLE_FIELDS
            .iter()
            .find(|f| f.key == field)
            .map(|f| f.key)
    }

    /// Record a local edit without pushing it. Captures the server base (and its
    /// revision) on the first edit of a field so we can detect divergence later.
    /// A no-op edit (value equals server) clears any pending edit for the field.
    pub fn set_pending(&mut self, field: &str, value: String) {
        let Some(key) = Self::static_field_key(field) else {
            return;
        };
        let Some(item) = &self.current else { return };
        let base = item_field_value(item, key);
        let base_rev = item.rev;
        if value == base {
            self.pending.retain(|p| p.field != key);
            self.field_status.remove(key);
            self.status = format!("{key} unchanged");
            return;
        }
        if let Some(p) = self.pending.iter_mut().find(|p| p.field == key) {
            p.value = value;
        } else {
            self.pending.push(PendingEdit {
                field: key,
                base,
                base_rev,
                value,
            });
        }
        self.status = format!("{} pending edit(s) — press p to push", self.pending.len());
    }

    /// Whether the open item has any un-pushed local edits.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty() || !self.pending_comments.is_empty()
    }

    /// Number of un-pushed local edits (for the status-bar ● indicator).
    pub fn pending_count(&self) -> usize {
        self.pending.len() + self.pending_comments.len()
    }

    /// Live-feed status marker for a field, if any.
    pub fn field_status(&self, field: &str) -> Option<FieldStatus> {
        self.field_status.get(field).copied()
    }

    /// Whether a field has an un-pushed local edit awaiting push.
    pub fn field_pending(&self, field: &str) -> bool {
        self.pending.iter().any(|p| p.field == field)
    }

    /// Overlay text for an existing comment if it has an un-pushed local edit.
    pub fn pending_comment_edit(&self, comment_id: u32) -> Option<&str> {
        self.pending_comments.iter().rev().find_map(|c| match c {
            PendingComment::Edit { comment_id: cid, text } if *cid == comment_id => {
                Some(text.as_str())
            }
            _ => None,
        })
    }

    /// Un-pushed, newly-added comments (author, text) awaiting push.
    pub fn pending_added_comments(&self) -> Vec<(&str, &str)> {
        self.pending_comments
            .iter()
            .filter_map(|c| match c {
                PendingComment::Add { author, text } => Some((author.as_str(), text.as_str())),
                _ => None,
            })
            .collect()
    }

    /// Whether an existing comment is marked for deletion on the next push.
    pub fn pending_comment_deleted(&self, comment_id: u32) -> bool {
        self.pending_comments
            .iter()
            .any(|c| matches!(c, PendingComment::Delete { comment_id: cid } if *cid == comment_id))
    }

    /// Mark the selected comment for deletion (deferred until push). Toggles off
    /// if it was already pending deletion.
    pub fn request_comment_delete(&mut self) {
        if self.detail_focus != DetailFocus::Comments {
            return;
        }
        let Some(item) = &self.current else { return };
        let Some(c) = item.comments.get(self.comment_selected) else {
            return;
        };
        let comment_id = c.id;
        if self.pending_comment_deleted(comment_id) {
            self.pending_comments.retain(
                |pc| !matches!(pc, PendingComment::Delete { comment_id: cid } if *cid == comment_id),
            );
            self.status = "deletion cancelled".into();
            return;
        }
        // Drop any pending edit on this comment; a delete supersedes it.
        self.pending_comments.retain(
            |pc| !matches!(pc, PendingComment::Edit { comment_id: cid, .. } if *cid == comment_id),
        );
        self.pending_comments
            .push(PendingComment::Delete { comment_id });
        self.status =
            format!("{} pending change(s) — press p to push", self.pending_count());
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
        // A field flagged as conflicted by the live-feed opens the resolution
        // menu instead of letting the user blindly edit over a divergence.
        if self.field_status.get(field.key) == Some(&FieldStatus::Conflicted) {
            self.open_resolution_for(field.key);
            return;
        }
        match field.kind {
            FieldKind::Line => {
                let value = self.effective_field_value(field.key);
                if let Some(ed) = &mut self.info_editor {
                    ed.editing = Some(TextInput::new(&value));
                }
            }
            FieldKind::State => {
                // Prefer a picker over the backend-supplied valid states; fall
                // back to cycling the canonical enum when none are available.
                let states: Vec<String> = self
                    .current
                    .as_ref()
                    .map(|w| w.available_states.clone())
                    .unwrap_or_default();
                if states.is_empty() {
                    if let Some(item) = &self.current {
                        let current = crate::api::models::WorkItemState::from_label(
                            &self.effective_field_value("state"),
                        )
                        .unwrap_or(item.state);
                        let next = current.next();
                        self.set_pending("state", next.label().to_string());
                    }
                } else {
                    let mut picker = Picker::new(states.clone());
                    let cur = self.effective_field_value("state");
                    if let Some(idx) = states.iter().position(|s| *s == cur) {
                        picker.selected = idx;
                    }
                    self.state_picker = Some(picker);
                }
            }
            FieldKind::Tags => {
                let tags = self
                    .current
                    .as_ref()
                    .map(|w| w.tags.clone())
                    .unwrap_or_default();
                let known = self.client.list_tags();
                self.tags_editor = Some(TagsEditor::new(tags, known));
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
        self.set_pending(key, value);
        if let Some(ed) = &mut self.info_editor {
            ed.editing = None;
        }
    }

    pub fn info_cancel_edit(&mut self) {
        if let Some(ed) = &mut self.info_editor {
            ed.editing = None;
        }
    }

    /// Apply the highlighted state from the floating state picker.
    pub fn state_picker_select(&mut self) {
        if let Some(p) = &self.state_picker
            && let Some(choice) = p.current()
        {
            self.set_pending("state", choice);
        }
        self.state_picker = None;
    }

    pub fn state_picker_cancel(&mut self) {
        self.state_picker = None;
    }

    /// Commit the tags editor's working set as a pending `tags` edit.
    pub fn tags_editor_commit(&mut self) {
        if let Some(t) = &self.tags_editor {
            let value = t.value();
            self.set_pending("tags", value);
        }
        self.tags_editor = None;
    }

    pub fn tags_editor_cancel(&mut self) {
        self.tags_editor = None;
    }

    /// Open the iteration (sprint) multi-select filter.
    pub fn open_iteration_picker(&mut self) {
        self.iteration_picker = Some(IterationPicker::new(
            self.iterations.clone(),
            self.selected_iterations.clone(),
        ));
    }

    /// Iteration-based and timeframe-based filtering are mutually exclusive.
    /// Setting a real timeframe window clears any iteration selection; choosing
    /// `All` leaves iterations untouched (it is the neutral "no time filter").
    fn set_timeframe(&mut self, tf: Timeframe) {
        self.timeframe = tf;
        if !tf.is_empty() {
            self.selected_iterations.clear();
        }
    }

    pub fn iteration_picker_commit(&mut self) {
        if let Some(p) = &self.iteration_picker {
            self.selected_iterations = p.value();
        }
        self.iteration_picker = None;
        // Iteration-based filtering replaces any active timeframe window.
        if !self.selected_iterations.is_empty() {
            self.timeframe = Timeframe::default();
        }
        self.request_refresh();
    }

    pub fn iteration_picker_cancel(&mut self) {
        self.iteration_picker = None;
    }

    pub fn open_type_picker(&mut self) {
        let options = crate::api::models::WORK_ITEM_TYPES
            .iter()
            .map(|s| s.to_string())
            .collect();
        self.type_picker = Some(TypeFilter::new(options, self.item_types.clone()));
    }

    pub fn type_picker_commit(&mut self) {
        if let Some(p) = &self.type_picker {
            self.item_types = p.value();
        }
        self.type_picker = None;
        // The user has made an explicit choice; don't let the first async load
        // overwrite it with the default.
        self.item_types_initialized = true;
        self.request_refresh();
    }

    pub fn type_picker_cancel(&mut self) {
        self.type_picker = None;
    }

    pub fn date_range_commit(&mut self) {
        if let Some(d) = &self.date_range
            && let Some(tf) = d.value()
        {
            self.set_timeframe(tf); // custom window clears iteration filter
            self.date_range = None;
            self.request_refresh();
            return;
        }
        self.date_range = None;
    }

    pub fn date_range_cancel(&mut self) {
        self.date_range = None;
    }

    fn request_comment_edit(&mut self) {
        if let Some(item) = &self.current
            && let Some(c) = item.comments.get(self.comment_selected) {
                // Seed with any un-pushed local edit so further edits stack.
                let initial = self
                    .pending_comment_edit(c.id)
                    .unwrap_or(&c.text)
                    .to_string();
                self.pending_editor = Some(EditorRequest::EditComment {
                    comment_id: c.id,
                    initial,
                });
            }
    }

    fn request_field_edit(&mut self, field: &'static str) {
        if self.current.is_some() {
            // A live-feed conflict on this field routes to the resolution menu.
            if self.field_status.get(field) == Some(&FieldStatus::Conflicted) {
                self.open_resolution_for(field);
                return;
            }
            let initial = self.effective_field_value(field);
            self.pending_editor = Some(EditorRequest::Field { field, initial });
        }
    }

    /// Called by the main loop after `$EDITOR` returns.
    pub fn apply_editor_result(&mut self, req: EditorRequest, text: String) {
        let text = text.trim_end().to_string();
        match req {
            EditorRequest::Field { field, .. } => {
                // Deferred: record locally, push later with `p`.
                self.set_pending(field, text);
            }
            EditorRequest::Merge { id, field } => {
                // The merged text becomes the new local value; record it as a
                // pending edit rebased onto the latest server revision, then drop
                // this field from the active resolution.
                self.set_pending(field, text);
                if let Some(p) = self.pending.iter_mut().find(|p| p.field == field)
                    && let Ok(remote) = self.client.get(id)
                {
                    p.base = item_field_value(&remote, field);
                    p.base_rev = remote.rev;
                }
                self.field_status.remove(field);
                self.resolve_finish_field(field);
                self.refresh_current();
                self.reload_items();
            }
            EditorRequest::Comment => {
                if text.is_empty() {
                    self.status = "empty comment discarded".into();
                } else {
                    // Deferred: held locally until the user pushes.
                    let author = self.auth.account().unwrap_or("you@example.com").to_string();
                    self.pending_comments
                        .push(PendingComment::Add { author, text });
                    self.status =
                        format!("{} pending change(s) — press p to push", self.pending_count());
                }
            }
            // Comment edits are deferred too (last-write-wins on push).
            EditorRequest::EditComment { comment_id, .. } => {
                self.pending_comments
                    .push(PendingComment::Edit { comment_id, text });
                self.status =
                    format!("{} pending change(s) — press p to push", self.pending_count());
            }
        }
    }

    fn refresh_current(&mut self) {
        if let Some(cur) = &self.current {
            let id = cur.id;
            if let Ok(item) = self.client.get(id) {
                self.set_current(item);
            }
        }
    }

    // --- conflict handling ---
    fn simulate_remote(&mut self) {
        if let Some(cur) = &self.current {
            let id = cur.id;
            match self.client.simulate_remote_edit(id) {
                Ok(()) => {
                    self.status =
                        format!("simulated a teammate editing #{id} — watch for ⚠/✓ markers");
                    // Route through the live-feed diff so the changed field gets
                    // a ⚠ (if it clashes with a pending edit) or ✓ marker.
                    if let Ok(item) = self.client.get(id) {
                        self.apply_remote_current(item);
                    }
                    self.reload_items();
                }
                Err(e) => self.status = format!("{e}"),
            }
        }
    }

    // --- manual push & conflict resolution ---

    /// Push all pending local edits in the background so the UI stays responsive.
    /// A worker re-fetches the server copy, checks for divergence, and either
    /// uploads everything or reports conflicts (picked up by [`drain_push`]).
    pub fn push_pending(&mut self) {
        if self.pushing {
            self.status = "a push is already in progress…".into();
            return;
        }
        if !self.has_pending() {
            self.status = "nothing to push".into();
            return;
        }
        self.spawn_push(false);
    }

    /// Take the pending edits as an in-flight snapshot and upload them on a
    /// worker thread. `force` skips the divergence pre-check (used by the
    /// resolution menu's force-push).
    fn spawn_push(&mut self, force: bool) {
        let Some(cur) = &self.current else { return };
        let id = cur.id;
        // Take the edits out as an in-flight snapshot; new edits made while the
        // push runs accumulate fresh and are pushed on the next `p`.
        let pending = std::mem::take(&mut self.pending);
        let comments = std::mem::take(&mut self.pending_comments);
        self.pushing = true;
        self.status = "pushing…".into();
        let client = self.client.clone_box();
        let tx = self.push_tx.clone();
        std::thread::spawn(move || {
            let outcome = run_push(client, id, pending, comments, force);
            let _ = tx.send(outcome);
        });
    }

    /// Apply any completed background push. Non-blocking; called each loop.
    pub fn drain_push(&mut self) {
        while let Ok(outcome) = self.push_rx.try_recv() {
            self.pushing = false;
            match outcome {
                PushOutcome::Conflicts {
                    id,
                    conflicts,
                    pending,
                    comments,
                } => {
                    // Only meaningful if the same item is still open; otherwise
                    // the user moved on and we drop the stale edits.
                    if self.current.as_ref().map(|c| c.id) != Some(id) {
                        self.status =
                            format!("push for #{id} found conflicts but you've moved on");
                        continue;
                    }
                    self.pending = pending;
                    self.pending_comments = comments;
                    let n = conflicts.len();
                    self.resolution = Some(Resolution {
                        id,
                        conflicts,
                        selected: 0,
                    });
                    self.status = format!(
                        "{n} conflicting field(s): j/k select, m merge, f force, Esc cancel"
                    );
                }
                PushOutcome::Done { id, total, failed } => {
                    self.field_status.clear();
                    // Refresh in the background so this stays non-blocking.
                    self.request_refresh();
                    self.status = if failed == 0 {
                        format!("pushed {total} change(s) to #{id}")
                    } else {
                        format!("pushed {} change(s); {failed} failed", total - failed)
                    };
                }
                PushOutcome::Error {
                    message,
                    pending,
                    comments,
                } => {
                    // Restore the edits so nothing is lost.
                    if self.pending.is_empty() {
                        self.pending = pending;
                    }
                    if self.pending_comments.is_empty() {
                        self.pending_comments = comments;
                    }
                    self.status = format!("push failed: {message}");
                }
            }
        }
    }

    /// True while a manual push is uploading in the background.
    pub fn is_pushing(&self) -> bool {
        self.pushing
    }

    /// Open the resolution menu pre-scoped to a single field (used when the user
    /// tries to edit a field the live-feed has flagged as conflicted).
    fn open_resolution_for(&mut self, field: &'static str) {
        let Some(cur) = &self.current else { return };
        let id = cur.id;
        let remote = item_field_value(cur, field);
        let (base, local) = self
            .pending
            .iter()
            .find(|p| p.field == field)
            .map(|p| (p.base.clone(), p.value.clone()))
            .unwrap_or_else(|| (remote.clone(), remote.clone()));
        self.resolution = Some(Resolution {
            id,
            conflicts: vec![FieldConflict {
                field,
                base,
                local,
                remote,
            }],
            selected: 0,
        });
        self.status =
            format!("conflict on {field}: m to merge, f to force-push, Esc to cancel");
    }

    /// Move the selection within the resolution menu.
    pub fn resolution_nav(&mut self, delta: isize) {
        if let Some(r) = &mut self.resolution
            && !r.conflicts.is_empty()
        {
            let n = r.conflicts.len() as isize;
            r.selected = (r.selected as isize + delta).rem_euclid(n) as usize;
        }
    }

    pub fn resolution_cancel(&mut self) {
        self.resolution = None;
        self.status = "resolution cancelled (changes still pending)".into();
    }

    /// Remove a resolved field from the active menu; close it when none remain.
    fn resolve_finish_field(&mut self, field: &str) {
        if let Some(r) = &mut self.resolution {
            r.conflicts.retain(|c| c.field != field);
            r.selected = 0;
            if r.conflicts.is_empty() {
                self.resolution = None;
            }
        }
    }

    /// Merge the selected conflicting field by hand in `$EDITOR` (conflict markers).
    fn resolve_merge(&mut self) {
        if let Some(r) = &self.resolution
            && let Some(c) = r.conflicts.get(r.selected)
        {
            let body = format!(
                "<<<<<<< local (your changes)\n{}\n||||||| base (common ancestor)\n{}\n=======\n{}\n>>>>>>> remote (current server)\n",
                c.local, c.base, c.remote
            );
            self.pending_editor = Some(EditorRequest::Merge {
                id: r.id,
                field: c.field,
            });
            self.merge_seed = Some(body);
        }
    }

    /// Force-push every locally-pending field, discarding remote changes.
    fn resolve_force(&mut self) {
        self.resolution = None;
        if self.pushing {
            self.status = "a push is already in progress…".into();
            return;
        }
        if !self.has_pending() {
            self.status = "nothing to push".into();
            return;
        }
        self.spawn_push(true);
        self.status = "force-pushing local changes…".into();
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
                self.client
                    .reconfigure(&self.config.org_url, &self.config.project);
                let _ = self.config.save();
                self.wizard = None;
                self.tab = Tab::WorkItems;
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

/// Read any editable field of a work item as a display string. Single source of
/// truth for conflict detection and pending-edit comparisons.
pub fn item_field_value(w: &WorkItem, field: &str) -> String {
    match field {
        "title" => w.title.clone(),
        "state" => w.state_name.clone(),
        "assignee" => w.assigned_to.clone(),
        "iteration" => w.iteration.clone(),
        "tags" => w.tags.join(", "),
        "notes" => w.notes.clone(),
        _ => w.description.clone(),
    }
}

/// Worker body for a background push. Re-fetches the open item, checks each
/// pending field for genuine divergence (unless `force`), then uploads all
/// pending field + comment changes. Runs entirely off the UI thread.
fn run_push(
    mut client: Box<dyn WorkItemClient + Send + Sync>,
    id: u32,
    pending: Vec<PendingEdit>,
    comments: Vec<PendingComment>,
    force: bool,
) -> PushOutcome {
    if !force {
        let remote = match client.get(id) {
            Ok(item) => item,
            Err(e) => {
                return PushOutcome::Error {
                    message: e.to_string(),
                    pending,
                    comments,
                };
            }
        };
        let mut conflicts = Vec::new();
        for p in &pending {
            let remote_val = item_field_value(&remote, p.field);
            // Genuine divergence: server moved on, and its value differs from
            // both our base and our local edit.
            if remote.rev != p.base_rev && remote_val != p.base && remote_val != p.value {
                conflicts.push(FieldConflict {
                    field: p.field,
                    base: p.base.clone(),
                    local: p.value.clone(),
                    remote: remote_val,
                });
            }
        }
        if !conflicts.is_empty() {
            return PushOutcome::Conflicts {
                id,
                conflicts,
                pending,
                comments,
            };
        }
    }

    let total = pending.len() + comments.len();
    let mut failed = 0;
    for p in pending {
        if client.update_field(id, p.field, &p.value).is_err() {
            failed += 1;
        }
    }
    for c in comments {
        let ok = match c {
            PendingComment::Add { author, text } => client.add_comment(id, &author, &text).is_ok(),
            PendingComment::Edit { comment_id, text } => {
                client.update_comment(id, comment_id, &text).is_ok()
            }
            PendingComment::Delete { comment_id } => client.delete_comment(id, comment_id).is_ok(),
        };
        if !ok {
            failed += 1;
        }
    }
    PushOutcome::Done { id, total, failed }
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
        app.timeframe = Timeframe::default();
        app.reload_items();
        app
    }

    /// Drive background push + refresh workers to completion (tests only).
    impl App {
        fn settle(&mut self) {
            for _ in 0..3000 {
                self.drain_push();
                self.drain_refresh();
                self.drain_tree();
                if !self.pushing && !self.loading && !self.tree_loading() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        /// Drive just the tree fetcher to completion (tests only).
        fn settle_tree(&mut self) {
            for _ in 0..3000 {
                self.drain_tree();
                if !self.tree_loading() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }

    #[test]
    fn async_refresh_loads_without_blocking() {
        let cfg = Config {
            org_url: "https://dev.azure.com/acme".into(),
            project: "Widgets".into(),
            ..Default::default()
        };
        let mut app = App::new(cfg);
        app.timeframe = Timeframe::default();
        app.items.clear();

        // Kick off a background refresh; the call returns immediately.
        app.request_refresh();
        assert!(app.is_loading(), "refresh should mark the app as loading");

        // Wait for the worker thread to deliver results, then apply them.
        for _ in 0..100 {
            app.drain_refresh();
            if !app.is_loading() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!app.is_loading(), "loading flag should clear once results arrive");
        assert!(!app.items.is_empty(), "items should be populated after refresh");
    }

    #[test]
    fn empty_config_starts_fzf_wizard() {
        let app = App::new(Config::default());
        let w = app.wizard.as_ref().expect("wizard");
        assert_eq!(w.step, 0);
        assert!(!w.org.options.is_empty()); // orgs pre-populated for fuzzy pick
        assert_eq!(app.context(), Context::Wizard);
    }

    /// Node ids currently visible in the tree (skips the "…" marker).
    fn tree_node_ids(app: &App) -> Vec<u32> {
        app.tree
            .flat
            .iter()
            .filter_map(|r| match r {
                TreeRow::Node { id, .. } => Some(*id),
                _ => None,
            })
            .collect()
    }

    fn tree_node_row(app: &App, id: u32) -> usize {
        app.tree
            .flat
            .iter()
            .position(|r| matches!(r, TreeRow::Node { id: i, .. } if *i == id))
            .unwrap()
    }

    #[test]
    fn tree_loads_in_background_without_blocking() {
        let mut app = ready_app();
        // load_tree_for returns immediately, before any node has been fetched.
        app.load_tree_for(1001);
        assert!(app.tree_loading(), "tree should report loading right away");
        assert!(tree_node_ids(&app).is_empty(), "nodes populate progressively");
        // Draining in the background fills the tree without ever blocking.
        app.settle_tree();
        assert!(!app.tree_loading());
        assert!(tree_node_ids(&app).contains(&1001));
        assert!(tree_node_ids(&app).contains(&1002));
    }

    fn search_chars(app: &mut App, s: &str) {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        for c in s.chars() {
            app.search_handle(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
    }

    #[test]
    fn work_items_fuzzy_filter_narrows_and_clears() {
        let mut app = ready_app();
        let total = app.items.len();
        assert!(total > 1);
        // Open the search bar and type a query that matches one item's title.
        app.open_search();
        assert!(app.filter_searching());
        let target = app.items[0].title.clone();
        let word = target.split_whitespace().next().unwrap().to_string();
        search_chars(&mut app, &word);
        let narrowed = app.visible_item_indices().len();
        assert!(narrowed >= 1 && narrowed <= total);
        // Every visible item matches the live query.
        for i in app.visible_item_indices() {
            assert!(app.list_filter.matches(&App::item_haystack(&app.items[i])));
        }
        // Commit it as a tag, then clear everything.
        app.search_commit();
        assert!(!app.filter_searching());
        assert_eq!(app.list_filter.tags.len(), 1);
        app.clear_filter();
        assert!(!app.filter_active());
        assert_eq!(app.visible_item_indices().len(), total);
    }

    #[test]
    fn work_items_filter_maps_selection_to_real_item() {
        let mut app = ready_app();
        // Filter to a single known item by its id, then "open" it.
        app.open_search();
        search_chars(&mut app, "1004");
        app.search_commit();
        app.list_state.select(Some(0));
        assert_eq!(app.selected_item().map(|w| w.id), Some(1004));
    }

    #[test]
    fn tree_fuzzy_filter_keeps_matches_and_ancestors() {
        let mut app = ready_app();
        app.load_tree_for(1001);
        app.settle_tree();
        // Filter the tree to the story whose title contains "Edit work" (#1003).
        app.tree_filter.input = Some(crate::ui::input::TextInput::new("Edit work"));
        app.rebuild_tree();
        let ids = tree_node_ids(&app);
        assert!(ids.contains(&1003), "matching node kept");
        assert!(ids.contains(&1001), "ancestor kept for context");
        assert!(!ids.contains(&1002), "non-matching sibling dropped");
    }

    #[test]
    fn tree_flattens_and_collapses() {
        let mut app = ready_app();
        app.load_tree_for(1001); // anchor the tree on the epic
        app.settle_tree();
        // Epic 1001 expanded shows its two story children (one nest level).
        assert!(tree_node_ids(&app).contains(&1002));
        // Select the epic and collapse it; children disappear.
        app.tree.selected = tree_node_row(&app, 1001);
        app.apply(Action::TreeCollapse);
        assert!(!tree_node_ids(&app).contains(&1002));
        // Expanding re-fetches and shows them again.
        app.apply(Action::TreeExpand);
        app.settle_tree();
        assert!(tree_node_ids(&app).contains(&1002));
    }

    #[test]
    fn tree_lazy_loads_one_level_at_a_time() {
        let mut app = ready_app();
        app.load_tree_for(1001); // Epic → Stories (1002, 1003) shown…
        app.settle_tree();
        // …but the grandchildren (Tasks 1004/1005 under 1002) are NOT loaded yet.
        assert!(!tree_node_ids(&app).contains(&1004));
        // Walking into #1002 fetches its children on demand.
        app.tree.selected = tree_node_row(&app, 1002);
        app.apply(Action::TreeExpand);
        app.settle_tree();
        assert!(tree_node_ids(&app).contains(&1004));
    }

    #[test]
    fn tree_capital_jk_navigate_siblings() {
        let mut app = ready_app();
        app.load_tree_for(1001); // root #1001, children #1002 + #1003
        app.settle_tree();
        app.tree.selected = tree_node_row(&app, 1002);
        app.apply(Action::TreeNextSibling); // → #1003 (its sibling)
        assert_eq!(tree_selected(&app), Some(1003));
        app.apply(Action::TreePrevSibling); // → back to #1002
        assert_eq!(tree_selected(&app), Some(1002));
    }

    #[test]
    fn tree_h_at_root_walks_up_a_level() {
        let mut app = ready_app();
        app.load_tree_for(1004); // root = parent #1002, with "… more above" (#1001)
        app.settle_tree();
        // The grandparent isn't loaded yet.
        assert!(!tree_node_ids(&app).contains(&1001));
        // Select the root (#1002) and press H to pull in the parent level.
        app.tree.selected = tree_node_row(&app, 1002);
        app.apply(Action::TreeLevelOut);
        app.settle_tree();
        assert_eq!(tree_selected(&app), Some(1001)); // moved onto the grandparent
        assert!(tree_node_ids(&app).contains(&1001)); // now revealed
        assert!(tree_node_ids(&app).contains(&1002)); // old root still shown
    }

    #[test]
    fn tree_capital_hl_navigate_levels() {
        let mut app = ready_app();
        app.load_tree_for(1001);
        app.settle_tree();
        app.tree.selected = tree_node_row(&app, 1001);
        // L descends into the first child (#1002), expanding if needed.
        app.apply(Action::TreeLevelIn);
        app.settle_tree();
        assert_eq!(tree_selected(&app), Some(1002));
        // H ascends back to the parent (#1001).
        app.apply(Action::TreeLevelOut);
        assert_eq!(tree_selected(&app), Some(1001));
    }

    #[test]
    fn tree_open_navigates_to_detail() {
        let mut app = ready_app();
        app.load_tree_for(1002);
        app.settle_tree();
        app.tab = Tab::Tree;
        app.tree.selected = tree_node_row(&app, 1002);
        app.apply(Action::Open);
        assert_eq!(app.tab, Tab::Detail);
        assert_eq!(app.current.as_ref().unwrap().id, 1002);
        // Related titles are cached on open so the relations pane never queries
        // the backend while navigating panes.
        for rel in app.related_ids() {
            assert!(app.related_title(rel).is_some(), "title for #{rel} should be cached");
        }
    }

    #[test]
    fn default_landing_tab_is_work_items() {
        assert_eq!(Tab::default(), Tab::WorkItems);
        let app = ready_app();
        assert_eq!(app.tab, Tab::WorkItems);
    }

    #[test]
    fn view_in_tree_centres_on_selected_item_with_ancestors() {
        let mut app = ready_app();
        // #1004 is a Task under #1002 (User Story) under #1001 (Epic).
        let idx = app.items.iter().position(|w| w.id == 1004).unwrap();
        app.list_state.select(Some(idx));
        app.view_in_tree();
        app.settle_tree();
        assert_eq!(app.tab, Tab::Tree);
        // The view is rooted on the parent (#1002) showing the focus + siblings;
        // the focus is expanded (it has no children, so nothing nests).
        assert!(app.tree.expanded.contains(&1002), "parent expanded");
        assert!(app.tree.expanded.contains(&1004), "focus expanded");
        // The cursor lands on the focus item itself.
        assert_eq!(tree_selected(&app), Some(1004));
        // Parent (#1002) and sibling (#1005) are present in the flattened view.
        let ids = tree_node_ids(&app);
        assert!(ids.contains(&1002)); // parent / root
        assert!(ids.contains(&1005)); // sibling
    }

    fn tree_selected(app: &App) -> Option<u32> {
        match app.tree.flat.get(app.tree.selected) {
            Some(TreeRow::Node { id, .. }) => Some(*id),
            _ => None,
        }
    }

    #[test]
    fn timeframe_and_iteration_filters_are_mutually_exclusive() {
        use crate::api::models::Date;
        let window = Timeframe { from: Some(Date::today()), to: None };
        let mut app = ready_app();
        // Selecting iterations clears any active timeframe window.
        app.set_timeframe(window);
        assert_eq!(app.timeframe, window);
        app.open_iteration_picker();
        app.iteration_picker.as_mut().unwrap().selected = vec!["Proj\\Sprint 24".into()];
        app.iteration_picker_commit();
        assert_eq!(app.selected_iterations, vec!["Proj\\Sprint 24".to_string()]);
        assert!(app.timeframe.is_empty(), "iteration filter clears timeframe");

        // Setting a timeframe window clears the iteration selection.
        app.set_timeframe(window);
        assert_eq!(app.timeframe, window);
        assert!(app.selected_iterations.is_empty(), "timeframe clears iterations");

        // An empty window is the neutral state and does NOT clear iterations.
        app.selected_iterations = vec!["Proj\\Sprint 24".into()];
        app.set_timeframe(Timeframe::default());
        assert_eq!(app.selected_iterations, vec!["Proj\\Sprint 24".to_string()]);
    }

    #[test]
    fn tree_is_independent_of_timeframe_filter() {
        use crate::api::models::Date;
        let mut app = ready_app();
        // Narrow the list filter so most items drop out of the work-items list…
        app.timeframe = Timeframe { from: Some(Date::today()), to: None };
        app.reload_items();
        // …yet the relationship tree, anchored on #1004, still shows its parent
        // (#1002) and sibling (#1005, last changed 12 days ago — outside the
        // window) because the tree fetches directly via the client.
        app.load_tree_for(1004);
        app.settle_tree();
        let ids = tree_node_ids(&app);
        assert!(ids.contains(&1002), "parent shown regardless of timeframe");
        assert!(ids.contains(&1005), "sibling shown regardless of timeframe");
    }

    #[test]
    fn notes_edit_applies() {
        let mut app = ready_app();
        app.open_id(1001);
        // Editing records a pending change; the server is untouched until push.
        app.apply_editor_result(
            EditorRequest::Field { field: "notes", initial: "x".into() },
            "new notes".into(),
        );
        assert!(app.has_pending());
        assert_eq!(app.effective_field_value("notes"), "new notes");
        // Pushing uploads it and clears pending.
        app.push_pending();
        app.settle();
        assert!(!app.has_pending());
        assert_eq!(app.current.as_ref().unwrap().notes, "new notes");
    }

    #[test]
    fn work_item_web_url_builds_from_org_and_project() {
        let mut app = ready_app();
        // Bare org name + project with a space → full dev.azure.com URL.
        app.config.org_url = "contoso".into();
        app.config.project = "My Project".into();
        assert_eq!(
            app.work_item_web_url(1002).unwrap(),
            "https://dev.azure.com/contoso/My%20Project/_workitems/edit/1002"
        );
        // A full URL is used as-is (trailing slash trimmed).
        app.config.org_url = "https://dev.azure.com/contoso/".into();
        app.config.project = "Widgets".into();
        assert_eq!(
            app.work_item_web_url(7).unwrap(),
            "https://dev.azure.com/contoso/Widgets/_workitems/edit/7"
        );
        // No org configured → no URL.
        app.config.org_url = "".into();
        assert!(app.work_item_web_url(1).is_none());
    }

    #[test]
    fn remote_change_triggers_conflict_then_force_resolves() {
        let mut app = ready_app();
        app.open_id(1002);
        // We make a local edit (deferred, not yet pushed).
        app.apply_editor_result(
            EditorRequest::Field {
                field: "description",
                initial: "my original".into(),
            },
            "my edit".into(),
        );
        // Teammate edits the same field after we started editing.
        app.client.simulate_remote_edit(1002).unwrap();
        // Pushing detects the divergence and opens the resolution menu.
        app.push_pending();
        app.settle();
        assert!(app.resolution.is_some(), "expected a conflict resolution menu");
        // Force-push keeps our local value and closes the menu.
        app.apply(Action::ResolveForce);
        app.settle();
        assert!(app.resolution.is_none());
        assert!(!app.has_pending());
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
                comment_id,
                initial: "old".into(),
            },
            "edited text".into(),
        );
        // Deferred: the edit overlays locally but the server is untouched until push.
        assert_eq!(app.pending_comment_edit(comment_id), Some("edited text"));
        assert!(app.has_pending());
        app.push_pending();
        app.settle();
        assert_eq!(app.current.as_ref().unwrap().comments[0].text, "edited text");
    }

    #[test]
    fn add_comment_is_deferred_until_push() {
        let mut app = ready_app();
        app.open_id(1002);
        let before = app.current.as_ref().unwrap().comments.len();
        app.apply_editor_result(EditorRequest::Comment, "a brand new comment".into());
        // Held locally; server comment count unchanged until push.
        assert!(app.has_pending());
        assert_eq!(app.current.as_ref().unwrap().comments.len(), before);
        assert_eq!(app.pending_added_comments(), vec![("you@example.com", "a brand new comment")]);
        app.push_pending();
        app.settle();
        assert!(!app.has_pending());
        assert_eq!(app.current.as_ref().unwrap().comments.len(), before + 1);
    }

    #[test]
    fn field_pending_tracks_unpushed_edit() {
        let mut app = ready_app();
        app.open_id(1002);
        assert!(!app.field_pending("title"));
        app.set_pending("title", "edited title".into());
        // The edited field is flagged as pending (drives its yellow border) until
        // pushed; an untouched field is not.
        assert!(app.field_pending("title"));
        assert!(!app.field_pending("description"));
        app.push_pending();
        app.settle();
        assert!(!app.field_pending("title"));
    }

    #[test]
    fn push_runs_in_background_without_blocking() {
        let mut app = ready_app();
        app.open_id(1002);
        app.set_pending("title", "async title".into());
        app.push_pending();
        // The push is spawned on a worker; the flag is set immediately so the
        // status bar can show the spinner while the UI keeps responding.
        assert!(app.is_pushing());
        app.settle();
        // Once the worker finishes the flag clears and the edit is persisted on
        // the shared server copy.
        assert!(!app.is_pushing());
        assert!(!app.field_pending("title"));
        assert_eq!(app.client.get(1002).unwrap().title, "async title");
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

    #[test]
    fn delete_comment_is_deferred_until_push() {
        let mut app = ready_app();
        // #1003 has one canned comment.
        app.open_id(1003);
        app.detail_focus = DetailFocus::Comments;
        app.comment_selected = 0;
        let cid = app.current.as_ref().unwrap().comments[0].id;
        let before = app.current.as_ref().unwrap().comments.len();
        app.request_comment_delete();
        // Marked locally; the server copy is untouched until push.
        assert!(app.pending_comment_deleted(cid));
        assert_eq!(app.current.as_ref().unwrap().comments.len(), before);
        app.push_pending();
        app.settle();
        assert!(!app.pending_comment_deleted(cid));
        assert_eq!(app.current.as_ref().unwrap().comments.len(), before - 1);
    }

    #[test]
    fn delete_comment_toggles_off() {
        let mut app = ready_app();
        app.open_id(1003);
        app.detail_focus = DetailFocus::Comments;
        app.comment_selected = 0;
        let cid = app.current.as_ref().unwrap().comments[0].id;
        app.request_comment_delete();
        assert!(app.pending_comment_deleted(cid));
        // Pressing delete again cancels the pending deletion.
        app.request_comment_delete();
        assert!(!app.pending_comment_deleted(cid));
        assert!(!app.has_pending());
    }

    #[test]
    fn tags_editor_commits_as_pending() {
        let mut app = ready_app();
        app.open_id(1002); // has tag "read"
        app.open_info_editor();
        app.info_editor.as_mut().unwrap().selected = field_index("tags");
        app.info_activate();
        assert!(app.tags_editor.is_some());
        // Add a brand-new tag and commit.
        app.tags_editor.as_mut().unwrap().selected.push("urgent".into());
        app.tags_editor_commit();
        assert!(app.field_pending("tags"));
        assert!(app.effective_field_value("tags").contains("urgent"));
        app.push_pending();
        app.settle();
        assert!(app.current.as_ref().unwrap().tags.contains(&"urgent".to_string()));
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
        // Deferred: the edit is pending and visible via the effective value, but
        // the server copy only changes once pushed.
        assert_eq!(app.effective_field_value("title"), "Renamed item");
        assert!(app.has_pending());
        app.push_pending();
        app.settle();
        assert_eq!(app.current.as_ref().unwrap().title, "Renamed item");
    }

    #[test]
    fn info_editor_state_picker_sets_state() {
        let mut app = ready_app();
        app.open_id(1002);
        app.open_info_editor();
        app.info_editor.as_mut().unwrap().selected = field_index("state");
        app.info_activate();
        // The backend supplies valid states, so a picker opens instead of an
        // in-place cycle.
        assert!(app.state_picker.is_some());
        let picker = app.state_picker.as_mut().unwrap();
        let idx = picker
            .options
            .iter()
            .position(|s| s == "Resolved")
            .unwrap();
        picker.selected = idx;
        app.state_picker_select();
        // Deferred: pending until pushed.
        assert_eq!(app.effective_field_value("state"), "Resolved");
        app.push_pending();
        app.settle();
        assert_eq!(app.current.as_ref().unwrap().state_name, "Resolved");
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
    fn live_feed_marks_upstream_update_without_pending() {
        let mut app = ready_app();
        app.open_id(1002);
        // No local edits; a teammate changes the item upstream.
        app.client.simulate_remote_edit(1002).unwrap();
        let remote = app.client.get(1002).unwrap();
        app.apply_remote_current(remote);
        // The changed field is flagged as cleanly updated (✓), not conflicted.
        assert_eq!(app.field_status("description"), Some(FieldStatus::Updated));
    }

    #[test]
    fn live_feed_flags_conflict_when_pending_diverges() {
        let mut app = ready_app();
        app.open_id(1002);
        // We have an un-pushed local edit on description.
        app.set_pending("description", "my local edit".into());
        // Meanwhile a teammate changes the same field upstream.
        app.client.simulate_remote_edit(1002).unwrap();
        let remote = app.client.get(1002).unwrap();
        app.apply_remote_current(remote);
        // The field is flagged as conflicted (⚠); editing it opens the menu.
        assert_eq!(app.field_status("description"), Some(FieldStatus::Conflicted));
        app.request_field_edit("description");
        assert!(app.resolution.is_some());
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
            selected_iterations: app.selected_iterations.clone(),
            item_types: app.item_types.clone(),
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
        assert_eq!(back.timeframe, Timeframe::default());
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

    #[test]
    fn iteration_filter_restricts_list_to_selected_sprint() {
        let mut app = ready_app();
        // Mimic the real flow where iterations are already loaded.
        app.iterations = app.client.list_iterations();
        app.iterations_initialized = true;
        // Mock items all live in "Sprint 24"; selecting "Sprint 25" hides them.
        let sprint25 = app
            .iterations
            .iter()
            .find(|i| i.name == "Sprint 25")
            .unwrap()
            .clone();
        app.open_iteration_picker();
        app.iteration_picker.as_mut().unwrap().selected = vec![sprint25.path.clone()];
        app.iteration_picker_commit();
        app.settle();
        assert!(app.items.is_empty());
        // Switching to the current sprint brings them back.
        let current = app
            .iterations
            .iter()
            .find(|i| i.is_current)
            .unwrap()
            .clone();
        app.open_iteration_picker();
        app.iteration_picker.as_mut().unwrap().selected = vec![current.path.clone()];
        app.iteration_picker_commit();
        app.settle();
        assert!(!app.items.is_empty());
    }

    #[test]
    fn iteration_picker_routes_through_context() {
        let mut app = ready_app();
        app.open_iteration_picker();
        assert_eq!(app.context(), Context::IterationFilter);
        app.iteration_picker_cancel();
        assert_ne!(app.context(), Context::IterationFilter);
    }

    #[test]
    fn type_filter_restricts_list_to_selected_types() {
        let mut app = ready_app();
        // Default (sync) path shows all types, including the Epic (1001).
        assert!(app.items.iter().any(|w| w.item_type == "Epic"));
        // Selecting only "Task" hides everything that isn't a Task.
        app.open_type_picker();
        app.type_picker.as_mut().unwrap().selected = vec!["Task".into()];
        app.type_picker_commit();
        app.settle();
        assert!(!app.items.is_empty());
        assert!(app.items.iter().all(|w| w.item_type == "Task"));
        // Clearing the selection brings everything back.
        app.open_type_picker();
        app.type_picker.as_mut().unwrap().selected = Vec::new();
        app.type_picker_commit();
        app.settle();
        assert!(app.items.iter().any(|w| w.item_type == "Epic"));
    }

    #[test]
    fn type_picker_routes_through_context() {
        let mut app = ready_app();
        app.open_type_picker();
        assert_eq!(app.context(), Context::TypeFilter);
        app.type_picker_cancel();
        assert_ne!(app.context(), Context::TypeFilter);
    }

    #[test]
    fn type_default_not_applied_in_sync_path() {
        // The sync `ready_app` path must leave `item_types` empty so existing
        // tests that inspect Epic/Task items still see them.
        let app = ready_app();
        assert!(app.item_types.is_empty());
    }

    #[test]
    fn type_default_applies_on_first_async_load() {
        let cfg = Config {
            org_url: "https://dev.azure.com/acme".into(),
            project: "Widgets".into(),
            ..Default::default()
        };
        let mut app = App::new(cfg);
        app.timeframe = Timeframe::default();
        // Mimic the real startup: kick off the first async refresh and drain it.
        app.request_refresh();
        app.settle();
        assert_eq!(
            app.item_types,
            vec!["User Story".to_string(), "Feature".to_string()]
        );
        assert!(app.item_types_initialized);
    }

    #[test]
    fn opening_timeframe_filter_routes_through_context() {
        let mut app = ready_app();
        app.apply(Action::OpenTimeframeFilter);
        assert!(app.date_range.is_some());
        // The date-range modal routes through the iteration-filter context.
        assert_eq!(app.context(), Context::IterationFilter);
    }

    #[test]
    fn timeframe_window_matches_and_labels() {
        use crate::api::models::Date;
        let today = Date::today();
        // Start-only window ("on or after" today): only same-day items pass.
        let from_only = Timeframe { from: Some(today), to: None };
        assert!(from_only.matches_days_ago(0));
        assert!(!from_only.matches_days_ago(1));
        assert!(from_only.label().starts_with('≥'));

        // End-only window ("on or before" today): everything up to now passes.
        let to_only = Timeframe { from: None, to: Some(today) };
        assert!(to_only.matches_days_ago(0));
        assert!(to_only.matches_days_ago(999));
        assert!(to_only.label().starts_with('≤'));

        // Empty window = no constraint.
        assert!(Timeframe::default().is_empty());
        assert!(Timeframe::default().matches_days_ago(999));
    }

    #[test]
    fn timeframe_wiql_clause_for_windows() {
        use crate::api::models::Date;
        assert!(Timeframe::default().wiql_clause().is_none());
        let tf = Timeframe {
            from: Some(Date::new(2024, 1, 1)),
            to: Some(Date::new(2024, 2, 1)),
        };
        let clause = tf.wiql_clause().unwrap();
        assert!(clause.contains("2024-01-01") && clause.contains("2024-02-01"));
        assert!(clause.contains(">=") && clause.contains("<="));
        // Start-only window emits just the lower bound.
        let lo = Timeframe { from: Some(Date::new(2024, 1, 1)), to: None };
        let c = lo.wiql_clause().unwrap();
        assert!(c.contains(">=") && !c.contains("<="));
    }

    #[test]
    fn work_item_filter_matches_iteration_prefix() {
        use crate::api::models::WorkItemFilter;
        let app = ready_app();
        let mut item = app.client.get(1001).unwrap();
        item.iteration = "Proj\\Sprint 24".into();
        item.changed_days_ago = 0;
        // UNDER-style prefix match on the iteration path.
        let f = WorkItemFilter {
            timeframe: Timeframe::default(),
            iterations: vec!["Proj\\Sprint 24".into()],
            item_types: Vec::new(),
        };
        assert!(f.matches(&item));
        let f2 = WorkItemFilter {
            timeframe: Timeframe::default(),
            iterations: vec!["Proj\\Sprint 25".into()],
            item_types: Vec::new(),
        };
        assert!(!f2.matches(&item));
    }
}
