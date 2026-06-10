//! Lightweight session persistence: remembers the view you left off in (tab,
//! timeframe, open item, tree expand/selection) and restores it next launch.
//!
//! Single-item restore only. See `docs/future-improvements.md` for the
//! multi-buffer idea.

use crate::api::models::Timeframe;
use crate::app::Tab;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SessionState {
    pub tab: Tab,
    pub timeframe: Timeframe,
    pub current_id: Option<u32>,
    pub tree_expanded: Vec<u32>,
    pub tree_selected: usize,
    pub list_selected: usize,
    pub detail_selected: usize,
}

impl SessionState {
    fn path() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "lazyaz")
            .context("could not determine config directory")?;
        Ok(dirs.config_dir().join("session.toml"))
    }

    /// Load the saved session, or `None` if absent/unreadable.
    pub fn load() -> Option<Self> {
        let path = Self::path().ok()?;
        let text = std::fs::read_to_string(path).ok()?;
        toml::from_str(&text).ok()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self)?)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}
