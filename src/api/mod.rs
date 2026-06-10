//! Work-item backend seam.
//!
//! The skeleton uses [`mock::MockClient`]; the real Azure DevOps REST client
//! (WIQL queries + work-item GET/PATCH + comments API) lands later behind the
//! same [`WorkItemClient`] trait. Methods are synchronous for now since the
//! mock has no I/O; the real client can run blocking requests or be adapted.

pub mod azure;
pub mod mock;
pub mod models;

use anyhow::{anyhow, Result};
use models::{Iteration, WorkItem, WorkItemFilter};

pub trait WorkItemClient {
    /// Items assigned to the current user, filtered to the given timeframe and
    /// iteration set.
    fn list_assigned(&self, filter: &WorkItemFilter) -> Result<Vec<WorkItem>>;
    fn get(&self, id: u32) -> Result<WorkItem>;
    fn add_comment(&mut self, id: u32, author: &str, text: &str) -> Result<()>;
    fn update_comment(&mut self, id: u32, comment_id: u32, text: &str) -> Result<()>;
    fn delete_comment(&mut self, id: u32, comment_id: u32) -> Result<()>;
    fn update_field(&mut self, id: u32, field: &str, value: &str) -> Result<()>;
    /// Projects available under `org` for the current user (wizard fzf source).
    fn list_projects(&self, org: &str) -> Vec<String>;

    /// The team's iterations (sprints), for the iteration filter picker.
    /// Default: none (no iteration filtering offered).
    fn list_iterations(&self) -> Vec<Iteration> {
        Vec::new()
    }

    /// All tags known to the project, for the tag editor's fuzzy autocomplete.
    /// Default: none (callers fall back to tags seen on loaded items).
    fn list_tags(&self) -> Vec<String> {
        Vec::new()
    }

    /// Valid workflow states for a given work-item type. Default: none, so the
    /// caller uses a canonical New→Active→Resolved→Closed cycle.
    fn list_states(&self, _item_type: &str) -> Vec<String> {
        Vec::new()
    }

    /// Point the client at a new org/project once the wizard has picked them.
    /// No-op for backends (like the mock) that don't talk to a real org.
    fn reconfigure(&mut self, _org_url: &str, _project: &str) {}

    /// Clone the client into a `Send + Sync` box so read-only refreshes can run
    /// on a background thread without blocking the UI.
    fn clone_box(&self) -> Box<dyn WorkItemClient + Send + Sync>;

    /// Demo-only: fake a remote edit so the conflict flow can be exercised.
    /// Real backends don't support this.
    fn simulate_remote_edit(&mut self, _id: u32) -> Result<()> {
        Err(anyhow!("simulate is only available with the mock backend"))
    }
}
