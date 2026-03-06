use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub agent: AgentConfig,
    pub repos: Vec<RepoConfig>,
    #[serde(default)]
    pub team: Vec<TeamMember>,
}

#[derive(Debug, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_agent_command")]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            command: default_agent_command(),
            args: Vec::new(),
            timeout_secs: default_timeout(),
        }
    }
}

fn default_agent_command() -> String {
    "claude".to_string()
}

fn default_timeout() -> u64 {
    120
}

#[derive(Debug, Deserialize)]
pub struct RepoConfig {
    pub name: String,
    #[serde(default)]
    pub labels_required: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TeamMember {
    pub github: String,
    pub name: String,
    #[serde(default)]
    pub role: String,
}

impl Config {
    /// Parse a TOML string into a Config.
    pub fn load_from_str(s: &str) -> Result<Self> {
        let config: Config =
            toml::from_str(s).context("Failed to parse TOML config")?;
        Ok(config)
    }

    /// Load config by checking (in order):
    /// 1. $CEO_CONFIG environment variable
    /// 2. ~/.config/ceo/config.toml
    /// 3. ./ceo.toml
    pub fn load() -> Result<Self> {
        let path = Self::find_config_path()
            .context("No config file found. Set $CEO_CONFIG, create ~/.config/ceo/config.toml, or create ./ceo.toml")?;
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        Self::load_from_str(&contents)
    }

    fn find_config_path() -> Option<PathBuf> {
        // 1. $CEO_CONFIG
        if let Ok(env_path) = std::env::var("CEO_CONFIG") {
            let p = PathBuf::from(env_path);
            if p.exists() {
                return Some(p);
            }
        }

        // 2. ~/.config/ceo/config.toml
        if let Some(config_dir) = dirs::config_dir() {
            let p = config_dir.join("ceo").join("config.toml");
            if p.exists() {
                return Some(p);
            }
        }

        // 3. ./ceo.toml
        let p = PathBuf::from("ceo.toml");
        if p.exists() {
            return Some(p);
        }

        None
    }
}
