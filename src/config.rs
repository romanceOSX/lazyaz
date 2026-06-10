use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Persisted user configuration, stored as TOML at the platform config dir.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// e.g. https://dev.azure.com/my-org
    #[serde(default)]
    pub org_url: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub team: String,
    /// The signed-in account (filled by auth). Display only.
    #[serde(default)]
    pub account: String,
}

impl Config {
    /// True once we have the minimum needed to talk to a (real) backend.
    pub fn is_complete(&self) -> bool {
        !self.org_url.trim().is_empty() && !self.project.trim().is_empty()
    }

    pub fn path() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "lazyaz")
            .context("could not determine config directory")?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    /// Load config; returns a default (empty) config if the file doesn't exist.
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let cfg = toml::from_str(&text).context("parsing config.toml")?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let text = toml::to_string_pretty(self).context("serializing config")?;
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}
