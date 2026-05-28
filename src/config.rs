use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Default)]
pub struct GuardConfig {
    #[serde(default)]
    pub rules: Vec<GuardRule>,
}

#[derive(Debug, Deserialize)]
pub struct GuardRule {
    pub name: String,
    pub when: String,
    pub threshold: Option<usize>,
}

impl GuardConfig {
    pub fn load(repo_root: &Path) -> Result<Self> {
        let path = repo_root.join(".causari").join("guard.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)?;
        let config: GuardConfig = toml::from_str(&content)?;
        Ok(config)
    }
}
