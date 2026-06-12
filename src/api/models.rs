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
    #[cfg_attr(not(test), allow(dead_code))]
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

    /// This date shifted by `n` days (negative shifts backwards).
    pub fn add_days(self, n: i64) -> Date {
        Date::from_epoch_days(self.to_epoch_days() + n)
    }

    /// Day of week with Monday = 0 … Sunday = 6.
    pub fn weekday_mon0(self) -> u32 {
        // Epoch day 0 (1970-01-01) was a Thursday (= 3 with Monday = 0).
        (self.to_epoch_days().rem_euclid(7) as u32 + 3) % 7
    }

    /// The first day of this date's month.
    pub fn first_of_month(self) -> Date {
        Date::new(self.year, self.month, 1)
    }

    /// Number of days in this date's month.
    pub fn days_in_month(self) -> u32 {
        let next = if self.month == 12 {
            Date::new(self.year + 1, 1, 1)
        } else {
            Date::new(self.year, self.month + 1, 1)
        };
        self.first_of_month().days_until(next) as u32
    }

    /// This date with the month shifted by `delta` (clamping the day to the
    /// target month's length).
    pub fn add_months(self, delta: i32) -> Date {
        let mut m = self.month as i32 - 1 + delta;
        let mut y = self.year;
        y += m.div_euclid(12);
        m = m.rem_euclid(12);
        let month = (m + 1) as u32;
        let dim = Date::new(y, month, 1).days_in_month();
        Date::new(y, month, self.day.min(dim))
    }
}

/// Short month name for display (`Jan` … `Dec`).
pub fn month_name(month: u32) -> &'static str {
    const NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    NAMES[(month.clamp(1, 12) - 1) as usize]
}

/// A change-date window for the timeframe filter. `from` and `to` are each
/// optional: a start-only window means "on or after", an end-only window "on or
/// before", and both unset means no constraint (the neutral "All").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Timeframe {
    pub from: Option<Date>,
    pub to: Option<Date>,
}

impl Timeframe {
    /// No date constraint (the neutral state — lets the iteration filter own
    /// time filtering instead).
    pub fn is_empty(&self) -> bool {
        self.from.is_none() && self.to.is_none()
    }

    pub fn label(&self) -> String {
        match (self.from, self.to) {
            (None, None) => "All".into(),
            (Some(f), None) => format!("≥ {}", f.to_iso()),
            (None, Some(t)) => format!("≤ {}", t.to_iso()),
            (Some(f), Some(t)) => format!("{}…{}", f.to_iso(), t.to_iso()),
        }
    }

    /// True if an item last changed `days_ago` days ago falls in this window.
    /// Used by the in-memory (mock) backend; the real backend pushes the
    /// equivalent clause into WIQL instead (see [`Timeframe::wiql_clause`]).
    pub fn matches_days_ago(&self, days_ago: u32) -> bool {
        if self.is_empty() {
            return true;
        }
        let changed = Date::today().add_days(-(days_ago as i64)).to_epoch_days();
        if let Some(f) = self.from
            && changed < f.to_epoch_days() {
                return false;
            }
        if let Some(t) = self.to
            && changed > t.to_epoch_days() {
                return false;
            }
        true
    }

    /// The `[System.ChangedDate]` WIQL clause for this window, or `None` when
    /// empty (no constraint).
    pub fn wiql_clause(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(f) = self.from {
            parts.push(format!("[System.ChangedDate] >= '{}'", f.to_iso()));
        }
        if let Some(t) = self.to {
            parts.push(format!("[System.ChangedDate] <= '{}'", t.to_iso()));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" AND "))
        }
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


#[cfg(test)]
mod date_tests {
    use super::*;

    #[test]
    fn days_in_month_handles_leap_years() {
        assert_eq!(Date::new(2026, 2, 1).days_in_month(), 28);
        assert_eq!(Date::new(2024, 2, 1).days_in_month(), 29); // leap
        assert_eq!(Date::new(2026, 6, 1).days_in_month(), 30);
        assert_eq!(Date::new(2026, 12, 1).days_in_month(), 31);
    }

    #[test]
    fn add_months_clamps_day_and_wraps_year() {
        // Jan 31 + 1 month → Feb 28 (clamped).
        assert_eq!(Date::new(2026, 1, 31).add_months(1), Date::new(2026, 2, 28));
        // Dec → Jan of next year.
        assert_eq!(Date::new(2026, 12, 15).add_months(1), Date::new(2027, 1, 15));
        assert_eq!(Date::new(2026, 1, 15).add_months(-1), Date::new(2025, 12, 15));
    }

    #[test]
    fn weekday_is_monday_zero() {
        // 2026-06-10 is a Wednesday → 2 with Monday = 0.
        assert_eq!(Date::new(2026, 6, 10).weekday_mon0(), 2);
    }
}
