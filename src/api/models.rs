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
    pub assigned_to: String,
    pub iteration: String,
    pub description: String,
    /// Free-form notes (separate from description), editable in the detail view.
    pub notes: String,
    pub tags: Vec<String>,
    pub parent: Option<u32>,
    pub children: Vec<u32>,
    pub comments: Vec<Comment>,
    /// How many days ago this item was last changed (drives timeframe filter).
    pub changed_days_ago: u32,
    /// Server revision; bumps on every change. Drives conflict detection.
    pub rev: u32,
}

/// Timeframe filter applied to the assigned-items list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Timeframe {
    Today,
    ThisWeek,
    ThisSprint,
    #[default]
    All,
}

impl Timeframe {
    pub const ALL: [Timeframe; 4] = [
        Timeframe::Today,
        Timeframe::ThisWeek,
        Timeframe::ThisSprint,
        Timeframe::All,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Timeframe::Today => "Today",
            Timeframe::ThisWeek => "This week",
            Timeframe::ThisSprint => "This sprint",
            Timeframe::All => "All",
        }
    }

    /// Inclusive upper bound (in days) for `changed_days_ago`.
    pub fn max_days(&self) -> u32 {
        match self {
            Timeframe::Today => 0,
            Timeframe::ThisWeek => 7,
            Timeframe::ThisSprint => 14,
            Timeframe::All => u32::MAX,
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
