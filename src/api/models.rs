use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkItemState {
    New,
    Active,
    #[allow(dead_code)] // produced by the real backend; no mock item uses it yet
    Resolved,
    Closed,
}

impl fmt::Display for WorkItemState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

impl WorkItemState {
    pub fn label(&self) -> &'static str {
        match self {
            WorkItemState::New => "New",
            WorkItemState::Active => "Active",
            WorkItemState::Resolved => "Resolved",
            WorkItemState::Closed => "Closed",
        }
    }

    /// Next state in the workflow cycle (wraps around).
    pub fn next(self) -> Self {
        match self {
            WorkItemState::New => WorkItemState::Active,
            WorkItemState::Active => WorkItemState::Resolved,
            WorkItemState::Resolved => WorkItemState::Closed,
            WorkItemState::Closed => WorkItemState::New,
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        match s {
            "New" => Some(WorkItemState::New),
            "Active" => Some(WorkItemState::Active),
            "Resolved" => Some(WorkItemState::Resolved),
            "Closed" => Some(WorkItemState::Closed),
            _ => None,
        }
    }

    /// Map an arbitrary process-template state name (e.g. "Committed", "Done",
    /// "To Do") to a colour category. Used for display tinting when the real
    /// state isn't one of the canonical four.
    pub fn category(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "new" | "to do" | "proposed" | "approved" | "open" => WorkItemState::New,
            "active" | "in progress" | "committed" | "doing" | "started" => WorkItemState::Active,
            "resolved" | "in review" | "testing" => WorkItemState::Resolved,
            "closed" | "done" | "completed" | "removed" => WorkItemState::Closed,
            _ => WorkItemState::Active,
        }
    }
}

/// A "Development" link on a work item: a pull request, commit, branch, or an
/// external hyperlink (e.g. a GitHub URL).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevLink {
    /// Human label, e.g. "Pull Request", "GitHub", "Commit".
    pub kind: String,
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug)]
pub struct Comment {
    /// Stable id so a specific comment can be edited.
    pub id: u32,
    pub author: String,
    pub when: String,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct WorkItem {
    pub id: u32,
    pub title: String,
    /// "User Story", "Task", "Bug", ...
    pub item_type: String,
    pub state: WorkItemState,
    /// Raw server state string (e.g. "Committed", "Done"), shown verbatim. The
    /// `state` enum above is only a colour category derived from this.
    pub state_name: String,
    /// Valid states for this item's type/workflow (for the state picker). Empty
    /// when the backend hasn't supplied them; a default cycle is used instead.
    pub available_states: Vec<String>,
    pub assigned_to: String,
    pub iteration: String,
    pub description: String,
    /// Free-form notes (separate from description), editable in the detail view.
    pub notes: String,
    pub tags: Vec<String>,
    pub parent: Option<u32>,
    pub children: Vec<u32>,
    /// "Development" links: pull requests, commits, branches, external URLs.
    pub dev_links: Vec<DevLink>,
    pub comments: Vec<Comment>,
    /// How many days ago this item was last changed (drives timeframe filter).
    pub changed_days_ago: u32,
    /// Server revision; bumps on every change. Drives conflict detection.
    pub rev: u32,
}

/// A plain calendar date (no time/zone), used for custom timeframe ranges and
/// for date arithmetic that the timeframe filter needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(year: i32, month: u32, day: u32) -> Self {
        Self { year, month, day }
    }

    /// Today (UTC).
    pub fn today() -> Self {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0) as i64;
        Self::from_epoch_days(secs / 86_400)
    }

    /// Parse a leading `YYYY-MM-DD` (ignores any time suffix).
    pub fn from_iso(s: &str) -> Option<Self> {
        let date = s.get(0..10)?;
        let mut p = date.split('-');
        let year = p.next()?.parse().ok()?;
        let month = p.next()?.parse().ok()?;
        let day = p.next()?.parse().ok()?;
        Some(Self { year, month, day })
    }

    pub fn to_iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// Days since the Unix epoch (Howard Hinnant's civil-from-days algorithm).
    pub fn to_epoch_days(self) -> i64 {
        let (y, m, d) = (self.year as i64, self.month as i64, self.day as i64);
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    fn from_epoch_days(z: i64) -> Self {
        let z = z + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        Self {
            year: (if m <= 2 { y + 1 } else { y }) as i32,
            month: m as u32,
            day: d as u32,
        }
    }

    /// Whole days from `self` until `other` (negative if `other` is earlier).
    pub fn days_until(self, other: Date) -> i64 {
        other.to_epoch_days() - self.to_epoch_days()
    }
}

/// Timeframe filter applied to the assigned-items list. The first four are
/// quick presets cycled with `f`/`F`; `Custom` is an explicit date range.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Timeframe {
    Today,
    ThisWeek,
    ThisSprint,
    #[default]
    All,
    /// An explicit `[from, to]` change-date window.
    Custom { from: Date, to: Date },
}

impl Timeframe {
    /// The cycle of quick presets reachable via `f`/`F` (excludes `Custom`).
    pub const ALL: [Timeframe; 4] = [
        Timeframe::Today,
        Timeframe::ThisWeek,
        Timeframe::ThisSprint,
        Timeframe::All,
    ];

    pub fn label(&self) -> String {
        match self {
            Timeframe::Today => "Today".into(),
            Timeframe::ThisWeek => "This week".into(),
            Timeframe::ThisSprint => "This sprint".into(),
            Timeframe::All => "All".into(),
            Timeframe::Custom { from, to } => format!("{}…{}", from.to_iso(), to.to_iso()),
        }
    }

    /// True if an item last changed `days_ago` days ago falls in this window.
    /// Used by the in-memory (mock) backend; the real backend pushes the
    /// equivalent clause into WIQL instead (see [`Timeframe::wiql_clause`]).
    pub fn matches_days_ago(&self, days_ago: u32) -> bool {
        match self {
            Timeframe::Today => days_ago == 0,
            Timeframe::ThisWeek => days_ago <= 7,
            Timeframe::ThisSprint => days_ago <= 14,
            Timeframe::All => true,
            Timeframe::Custom { from, to } => {
                // changed-date = today - days_ago; keep it within [from, to].
                let today = Date::today();
                let lo = to.days_until(today).max(0); // newest bound → smallest days-ago
                let hi = from.days_until(today); // oldest bound → largest days-ago
                let days_ago = days_ago as i64;
                days_ago >= lo && days_ago <= hi
            }
        }
    }

    /// The `[System.ChangedDate]` WIQL clause for this timeframe, or `None` for
    /// `All` (no constraint).
    pub fn wiql_clause(&self) -> Option<String> {
        match self {
            Timeframe::Today => Some("[System.ChangedDate] >= @Today".into()),
            Timeframe::ThisWeek => Some("[System.ChangedDate] >= @Today - 7".into()),
            Timeframe::ThisSprint => Some("[System.ChangedDate] >= @Today - 14".into()),
            Timeframe::All => None,
            Timeframe::Custom { from, to } => Some(format!(
                "[System.ChangedDate] >= '{}' AND [System.ChangedDate] <= '{}'",
                from.to_iso(),
                to.to_iso()
            )),
        }
    }

    pub fn next(&self) -> Timeframe {
        let idx = Self::ALL.iter().position(|t| t == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn prev(&self) -> Timeframe {
        let idx = Self::ALL.iter().position(|t| t == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// A team iteration (sprint): a node in the `System.IterationPath` hierarchy,
/// optionally flagged as the team's current iteration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Iteration {
    /// The `System.IterationPath` value (e.g. `Proj\Release 1\Sprint 9`).
    pub path: String,
    /// Short display name (the leaf node, e.g. `Sprint 9`).
    pub name: String,
    /// True for the team's current (active) iteration.
    pub is_current: bool,
}

/// The full filter applied to the assigned-items query: a timeframe window plus
/// an optional set of iteration paths (empty = all iterations) and work-item
/// types (empty = all types).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkItemFilter {
    pub timeframe: Timeframe,
    /// Selected iteration paths; empty means "no iteration constraint".
    pub iterations: Vec<String>,
    /// Selected work-item types (e.g. "User Story", "Feature"); empty means
    /// "no type constraint".
    pub item_types: Vec<String>,
}

/// The work-item types the type-filter menu offers, in display order.
pub const WORK_ITEM_TYPES: [&str; 5] = ["Task", "User Story", "Feature", "Capability", "Epic"];

/// The types checked by default (only User Stories and Features).
pub const DEFAULT_WORK_ITEM_TYPES: [&str; 2] = ["User Story", "Feature"];

impl WorkItemFilter {
    /// True if `item` passes the timeframe, iteration, and type constraints.
    /// Used by the in-memory backend; the real backend filters server-side.
    pub fn matches(&self, item: &WorkItem) -> bool {
        if !self.timeframe.matches_days_ago(item.changed_days_ago) {
            return false;
        }
        if !self.item_types.is_empty()
            && !self
                .item_types
                .iter()
                .any(|t| t.eq_ignore_ascii_case(&item.item_type))
        {
            return false;
        }
        if self.iterations.is_empty() {
            return true;
        }
        // UNDER semantics: an item matches a selected node or any descendant.
        self.iterations.iter().any(|sel| {
            item.iteration == *sel
                || item.iteration.starts_with(&format!("{sel}\\"))
                || item.iteration.starts_with(&format!("{sel}/"))
        })
    }
}

