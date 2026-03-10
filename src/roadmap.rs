use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// An initiative from the roadmap file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Initiative {
    pub name: String,
    #[serde(default)]
    pub timeframe: Option<String>,
    #[serde(default)]
    pub repos: Vec<String>,
    pub description: String,
}

/// Top-level roadmap file structure.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Roadmap {
    #[serde(default, rename = "initiatives")]
    pub initiatives: Vec<Initiative>,
}

impl Roadmap {
    /// Path to the roadmap file (same dir as config).
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .map(|d| d.join("ceo").join("roadmap.toml"))
            .unwrap_or_else(|| PathBuf::from("roadmap.toml"))
    }

    /// Load roadmap from file. Returns empty roadmap if file doesn't exist.
    pub fn load() -> Self {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save roadmap to file.
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let toml_str = toml::to_string_pretty(self).unwrap_or_default();
        std::fs::write(&path, toml_str)
    }

    /// Get initiatives relevant to a specific repo.
    pub fn for_repo(&self, repo: &str) -> Vec<&Initiative> {
        self.initiatives
            .iter()
            .filter(|i| i.repos.iter().any(|r| r == repo))
            .collect()
    }

    /// Add an initiative. Returns error if name already exists.
    pub fn add(&mut self, initiative: Initiative) -> Result<(), String> {
        if self.initiatives.iter().any(|i| i.name == initiative.name) {
            return Err(format!("Initiative '{}' already exists", initiative.name));
        }
        self.initiatives.push(initiative);
        Ok(())
    }

    /// Remove an initiative by name. Returns error if not found.
    pub fn remove(&mut self, name: &str) -> Result<(), String> {
        let before = self.initiatives.len();
        self.initiatives.retain(|i| i.name != name);
        if self.initiatives.len() == before {
            return Err(format!("Initiative '{}' not found", name));
        }
        Ok(())
    }

    /// Template content for new roadmap files.
    pub fn template() -> &'static str {
        r#"# CEO Roadmap — Initiatives & Product Lines
#
# [[initiatives]]
# name = "Project Name"
# timeframe = "Q1 2026"           # optional: "Q1 2026", "2026", "H2 2026"
# repos = ["org/repo1", "org/repo2"]
# description = "What this initiative is about"
"#
    }
}
