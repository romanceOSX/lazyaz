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
use models::{Timeframe, WorkItem};

pub trait WorkItemClient {
    /// Items assigned to the current user, filtered to the timeframe.
    fn list_assigned(&self, timeframe: Timeframe) -> Result<Vec<WorkItem>>;
    fn get(&self, id: u32) -> Result<WorkItem>;
    fn add_comment(&mut self, id: u32, author: &str, text: &str) -> Result<()>;
    fn update_comment(&mut self, id: u32, comment_id: u32, text: &str) -> Result<()>;
    fn update_field(&mut self, id: u32, field: &str, value: &str) -> Result<()>;
    /// Projects available under `org` for the current user (wizard fzf source).
    fn list_projects(&self, org: &str) -> Vec<String>;

    /// Demo-only: fake a remote edit so the conflict flow can be exercised.
    /// Real backends don't support this.
    fn simulate_remote_edit(&mut self, _id: u32) -> Result<()> {
        Err(anyhow!("simulate is only available with the mock backend"))
    }
}
